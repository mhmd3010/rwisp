use serde_json::{Value, json};
use sqlx::{MySqlPool, Row};

use rwisp_server::accounts::{self, NewAccount};
use rwisp_server::db::init_pool;
use rwisp_server::packages::{PackageInput, create_package, delete_package};
use rwisp_server::radius::accounting;
use rwisp_server::radius::attributes::RadiusRequest;
use rwisp_server::radius::error::RadiusError;

fn enabled() -> bool {
    std::env::var("RWISP_RUN_DB_CRUD_TESTS").as_deref() == Ok("1")
        && std::env::var("DATABASE_HOST").is_ok()
}

fn request(value: Value) -> RadiusRequest {
    RadiusRequest::from_json(&value).expect("radius request")
}

/// FreeRADIUS `rlm_rest` `body = 'json'` shape: each attribute as an object with
/// `type`/`value`, enumerated attributes sent numerically.
fn rest_json(status: Value, session: &str, username: &str, extras: Value) -> Value {
    let mut base = json!({
        "Acct-Status-Type": { "attr_num": 40, "type": "integer", "value": [status] },
        "User-Name": { "type": "string", "value": [username] },
        "Acct-Session-Id": { "type": "string", "value": [session] },
        "NAS-IP-Address": { "type": "ipaddr", "value": ["10.0.0.1"] },
        "Calling-Station-Id": { "type": "string", "value": ["AA:BB:CC:DD:EE:FF"] },
        "Framed-IP-Address": { "type": "ipaddr", "value": ["10.5.0.2"] }
    });
    if let Some(map) = base.as_object_mut()
        && let Some(extra_map) = extras.as_object()
    {
        for (k, v) in extra_map {
            map.insert(k.clone(), v.clone());
        }
    }
    base
}

#[tokio::test]
async fn fails_closed_on_unknown_accounting_status() {
    dotenvy::dotenv().ok();
    if !enabled() {
        eprintln!("skipping accounting test: set RWISP_RUN_DB_CRUD_TESTS=1 and DATABASE_*");
        return;
    }
    let pool = init_pool().await.expect("pool");
    let error = accounting::process(
        &pool,
        &request(rest_json(json!("99"), "unknown-1", "nobody", json!({}))),
    )
    .await
    .expect_err("unknown status must fail explicitly");
    assert!(matches!(error, RadiusError::BadRequest { .. }));
}

#[tokio::test]
async fn accounting_persists_numeric_start_interim_stop_with_gigawords() {
    dotenvy::dotenv().ok();
    if !enabled() {
        eprintln!("skipping accounting test: set RWISP_RUN_DB_CRUD_TESTS=1 and DATABASE_*");
        return;
    }

    let pool = init_pool().await.expect("pool");
    let tag = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let username = format!("acct-stream-{}-{tag}", std::process::id());

    let package_id = create_package(
        &pool,
        &PackageInput::from_json(&json!({
            "name": format!("acct-stream-pkg-{}-{tag}", std::process::id()),
            "validity_secs": 3600
        }))
        .expect("package"),
    )
    .await
    .expect("create package");
    accounts::create_account(
        &pool,
        &NewAccount::from_json(&json!({
            "username": username,
            "password": "correct horse battery",
            "package_id": package_id
        }))
        .expect("account"),
    )
    .await
    .expect("create account");

    // Start (numeric 1) with an initial 64-bit counter.
    accounting::process(
        &pool,
        &request(rest_json(
            json!("1"),
            "sess-num",
            &username,
            json!({
                "Acct-Session-Time": { "type": "integer", "value": [10] },
                "Acct-Input-Octets": { "type": "integer", "value": [1000] },
                "Acct-Input-Gigawords": { "type": "integer", "value": [2] },
                "Acct-Output-Octets": { "type": "integer", "value": [500] }
            }),
        )),
    )
    .await
    .expect("start");

    let row = fetch_session(&pool, &username, "sess-num")
        .await
        .expect("session exists after Start");
    assert_eq!(row.0, "start");
    assert_eq!(row.1, 10);
    assert_eq!(
        row.2,
        1000 + 2 * (1u64 << 32),
        "64-bit input with gigawords"
    );
    assert_eq!(row.3, 500);

    // Interim (numeric 3) with grown counters.
    accounting::process(
        &pool,
        &request(rest_json(
            json!("3"),
            "sess-num",
            &username,
            json!({
                "Acct-Session-Time": { "type": "integer", "value": [40] },
                "Acct-Input-Octets": { "type": "integer", "value": [3000] },
                "Acct-Input-Gigawords": { "type": "integer", "value": [3] },
                "Acct-Output-Octets": { "type": "integer", "value": [900] }
            }),
        )),
    )
    .await
    .expect("interim");

    // Stop (numeric 2) with final counters.
    accounting::process(
        &pool,
        &request(rest_json(
            json!("2"),
            "sess-num",
            &username,
            json!({
                "Acct-Session-Time": { "type": "integer", "value": [75] },
                "Acct-Input-Octets": { "type": "integer", "value": [5000] },
                "Acct-Input-Gigawords": { "type": "integer", "value": [4] },
                "Acct-Output-Octets": { "type": "integer", "value": [2500] },
                "Acct-Terminate-Cause": { "type": "string", "value": ["User-Request"] }
            }),
        )),
    )
    .await
    .expect("stop");

    let row = fetch_session(&pool, &username, "sess-num")
        .await
        .expect("session");
    assert_eq!(row.0, "stop");
    assert_eq!(row.1, 75);
    assert_eq!(row.2, 5000 + 4 * (1u64 << 32));
    assert_eq!(row.3, 2500);

    // Idempotent replay of Stop must not double-count or regress.
    accounting::process(
        &pool,
        &request(rest_json(
            json!("2"),
            "sess-num",
            &username,
            json!({
                "Acct-Session-Time": { "type": "integer", "value": [5] },
                "Acct-Input-Octets": { "type": "integer", "value": [1] },
                "Acct-Output-Octets": { "type": "integer", "value": [1] }
            }),
        )),
    )
    .await
    .expect("stop replay");

    let account = accounts::get_account_by_username(&pool, &username)
        .await
        .expect("get")
        .expect("exists");
    assert_eq!(account.bytes_in, 5000 + 4 * (1u64 << 32));
    assert_eq!(account.bytes_out, 2500);
    assert_eq!(account.usage_secs, 75);

    // Textual status for a second session must also be stored.
    accounting::process(
        &pool,
        &request(rest_json(
            json!("Start"),
            "sess-text",
            &username,
            json!({ "Acct-Input-Octets": { "type": "integer", "value": [7] } }),
        )),
    )
    .await
    .expect("textual start");
    assert!(fetch_session(&pool, &username, "sess-text").await.is_some());

    // Non-session event (Accounting-On = 7) is acknowledged but stored as nothing.
    accounting::process(
        &pool,
        &request(rest_json(json!("7"), "sess-none", &username, json!({}))),
    )
    .await
    .expect("non-session ok");
    assert!(fetch_session(&pool, &username, "sess-none").await.is_none());

    // Cleanup.
    sqlx::query("DELETE FROM tbl_radius_sessions WHERE username = ?")
        .bind(&username)
        .execute(&pool)
        .await
        .expect("cleanup sessions");
    sqlx::query("DELETE FROM tbl_accounts WHERE username = ?")
        .bind(&username)
        .execute(&pool)
        .await
        .expect("cleanup account");
    let _ = delete_package(&pool, package_id).await;
}

async fn fetch_session(
    pool: &MySqlPool,
    username: &str,
    session_id: &str,
) -> Option<(String, u64, u64, u64)> {
    let row = sqlx::query(
        "SELECT status, session_time, input_octets, output_octets \
         FROM tbl_radius_sessions WHERE username = ? AND acct_session_id = ?",
    )
    .bind(username)
    .bind(session_id)
    .fetch_optional(pool)
    .await
    .expect("query session")?;
    Some((
        row.try_get("status").unwrap(),
        row.try_get("session_time").unwrap(),
        row.try_get("input_octets").unwrap(),
        row.try_get("output_octets").unwrap(),
    ))
}
