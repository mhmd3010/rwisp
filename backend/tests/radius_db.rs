use md5::{Digest, Md5};
use serde_json::json;
use sqlx::{MySqlPool, Row};

use rwisp_server::accounts::{self, AccountStatus, NewAccount, RenewalRequest};
use rwisp_server::db::init_pool;
use rwisp_server::packages::{PackageInput, create_package, delete_package};
use rwisp_server::radius::attributes::RadiusRequest;
use rwisp_server::radius::{accounting, authenticate, authorize, run_maintenance};
use rwisp_server::vouchers::{self, BatchRequest};

fn enabled() -> bool {
    std::env::var("RWISP_RUN_DB_CRUD_TESTS").as_deref() == Ok("1")
        && std::env::var("DATABASE_HOST").is_ok()
}

fn request(attributes: serde_json::Value) -> RadiusRequest {
    RadiusRequest::from_json(&attributes).expect("radius request")
}

fn attr<'a>(body: &'a serde_json::Value, name: &str) -> Option<&'a str> {
    body.get(name)?.get("value")?.get(0)?.as_str()
}

async fn cleanup(pool: &MySqlPool, username: &str, package_ids: &[u64], batch_ids: &[u64]) {
    let _ = sqlx::query("DELETE FROM tbl_radius_sessions WHERE username = ?")
        .bind(username)
        .execute(pool)
        .await;
    let _ = sqlx::query("DELETE FROM tbl_accounts WHERE username = ?")
        .bind(username)
        .execute(pool)
        .await;
    for package_id in package_ids {
        let _ = sqlx::query("DELETE FROM tbl_vouchers WHERE package_id = ?")
            .bind(package_id)
            .execute(pool)
            .await;
    }
    for batch_id in batch_ids {
        let _ = sqlx::query("DELETE FROM tbl_voucher_batches WHERE id = ?")
            .bind(batch_id)
            .execute(pool)
            .await;
    }
    for package_id in package_ids {
        let _ = delete_package(pool, *package_id).await;
    }
}

#[tokio::test]
async fn policy_rejection_does_not_activate() {
    dotenvy::dotenv().ok();
    if !enabled() {
        eprintln!("skipping radius policy test: set RWISP_RUN_DB_CRUD_TESTS=1 and DATABASE_*");
        return;
    }

    let pool = init_pool().await.expect("pool");
    let tag = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let username = format!("radius-reject-{}-{tag}", std::process::id());

    // Tiny data limit so accumulated usage rejects the request at authorize time.
    let package_id = create_package(
        &pool,
        &PackageInput::from_json(&json!({
            "name": format!("radius-reject-pkg-{}-{tag}", std::process::id()),
            "validity_secs": 3600,
            "data_limit_bytes": 100
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
    let account_id = accounts::get_account_by_username(&pool, &username)
        .await
        .expect("get")
        .expect("exists")
        .id;

    // Accumulate usage beyond the limit (account is still pending / not activated).
    accounting::process(
        &pool,
        &request(json!({
            "Acct-Status-Type": "Stop",
            "User-Name": username,
            "Acct-Session-Id": "reject-1",
            "NAS-IP-Address": "10.0.0.9",
            "Acct-Session-Time": 5,
            "Acct-Input-Octets": 500u64
        })),
    )
    .await
    .expect("accounting");

    // authorize must reject (401) and must not activate.
    assert_eq!(
        authorize(&pool, &request(json!({ "User-Name": username })))
            .await
            .expect("authorize")
            .status,
        401,
        "usage over the data limit must be rejected"
    );
    let account = accounts::get_account(&pool, account_id)
        .await
        .expect("get")
        .expect("exists");
    assert_eq!(
        account.status,
        AccountStatus::Pending,
        "rejected request must not activate"
    );

    cleanup(&pool, &username, &[package_id], &[]).await;
}

#[tokio::test]
async fn radius_authorize_authenticate_accounting_maintenance() {
    dotenvy::dotenv().ok();
    if !enabled() {
        eprintln!("skipping radius test: set RWISP_RUN_DB_CRUD_TESTS=1 and DATABASE_*");
        return;
    }

    let pool = init_pool().await.expect("pool");
    let tag = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let username = format!("radius-acct-{}-{tag}", std::process::id());

    let package_id = create_package(
        &pool,
        &PackageInput::from_json(&json!({
            "name": format!("radius-pkg-{}-{tag}", std::process::id()),
            "validity_secs": 3600,
            "upload_rate_bps": 5_000_000u64,
            "download_rate_bps": 2_000_000u64,
            "device_limit": 1,
            "data_limit_bytes": 1_000_000_000u64,
            "usage_limit_secs": 7200
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

    // ---- authorize is policy-only: sets Auth-Type=rest, never Accept, no activation ----
    let outcome = authorize(
        &pool,
        &request(json!({ "User-Name": username, "User-Password": "correct horse battery" })),
    )
    .await
    .expect("authorize");
    assert_eq!(outcome.status, 200, "body: {}", outcome.body);
    assert_eq!(
        attr(&outcome.body, "control:Auth-Type"),
        Some("rest"),
        "must not use Auth-Type=Accept (bypass)"
    );
    assert_eq!(
        attr(&outcome.body, "reply:Mikrotik-Rate-Limit"),
        Some("5M/2M")
    );
    assert_eq!(attr(&outcome.body, "control:Simultaneous-Use"), Some("1"));
    assert_eq!(attr(&outcome.body, "reply:Max-Octets"), Some("1000000000"));
    assert_eq!(attr(&outcome.body, "reply:Max-All-Session"), Some("7200"));

    let account = accounts::get_account_by_username(&pool, &username)
        .await
        .expect("get")
        .expect("exists");
    assert_eq!(
        account.status,
        AccountStatus::Pending,
        "authorize must not activate"
    );

    // authorize does not check the password, so it still returns 200 for a wrong one.
    assert_eq!(
        authorize(
            &pool,
            &request(json!({ "User-Name": username, "User-Password": "nope" }))
        )
        .await
        .expect("authorize")
        .status,
        200
    );

    // ---- authenticate verifies and then activates ----
    assert_eq!(
        authenticate(
            &pool,
            &request(json!({ "User-Name": username, "User-Password": "nope" }))
        )
        .await
        .expect("authenticate")
        .status,
        401,
        "wrong password rejected by authenticate"
    );
    let account = accounts::get_account_by_username(&pool, &username)
        .await
        .expect("get")
        .expect("exists");
    assert_eq!(
        account.status,
        AccountStatus::Pending,
        "failed auth must not activate"
    );

    assert_eq!(
        authenticate(
            &pool,
            &request(json!({ "User-Name": username, "User-Password": "correct horse battery" }))
        )
        .await
        .expect("authenticate")
        .status,
        200
    );
    let account = accounts::get_account_by_username(&pool, &username)
        .await
        .expect("get")
        .expect("exists");
    assert_eq!(account.status, AccountStatus::Active);
    assert!(account.activated_at.is_some());

    // Subscription start is exposed for subscription-scoped counters.
    let outcome = authorize(&pool, &request(json!({ "User-Name": username })))
        .await
        .expect("authorize");
    let start_before = attr(&outcome.body, "reply:RWISP-Subscription-Start")
        .expect("subscription start")
        .to_string();

    let account_id = account.id;

    // ---- rejection: suspended / expired ----
    accounts::suspend_account(&pool, account_id)
        .await
        .expect("suspend");
    assert_eq!(
        authorize(&pool, &request(json!({ "User-Name": username })))
            .await
            .expect("a")
            .status,
        403
    );
    assert_eq!(
        authenticate(
            &pool,
            &request(json!({ "User-Name": username, "User-Password": "correct horse battery" }))
        )
        .await
        .expect("a")
        .status,
        403
    );
    accounts::reactivate_account(&pool, account_id)
        .await
        .expect("reactivate");

    sqlx::query(
        "UPDATE tbl_accounts SET expires_at = DATE_SUB(NOW(), INTERVAL 1 MINUTE) WHERE id = ?",
    )
    .bind(account_id)
    .execute(&pool)
    .await
    .expect("expire");
    assert_eq!(
        authorize(&pool, &request(json!({ "User-Name": username })))
            .await
            .expect("a")
            .status,
        401,
        "expired subscription rejected"
    );

    // ---- renewal: subscription start moves forward ----
    let renewal = RenewalRequest::from_json(&json!({
        "package_id": package_id,
        "activation": "immediate"
    }))
    .expect("renewal");
    accounts::renew_account(&pool, account_id, &renewal)
        .await
        .expect("renew");
    let outcome = authorize(&pool, &request(json!({ "User-Name": username })))
        .await
        .expect("a");
    assert_eq!(outcome.status, 200);
    let start_after = attr(&outcome.body, "reply:RWISP-Subscription-Start")
        .expect("subscription start")
        .to_string();
    assert!(
        start_after >= start_before,
        "renewal should advance the subscription start ({start_after} >= {start_before})"
    );

    // ---- concurrent authentication: exactly one activation survives ----
    let concurrent_name = format!("radius-conc-{}-{tag}", std::process::id());
    accounts::create_account(
        &pool,
        &NewAccount::from_json(&json!({
            "username": concurrent_name,
            "password": "correct horse battery",
            "package_id": package_id
        }))
        .expect("account"),
    )
    .await
    .expect("create");
    let mut handles = Vec::new();
    for _ in 0..10 {
        let pool = pool.clone();
        let username = concurrent_name.clone();
        handles.push(tokio::spawn(async move {
            authenticate(
                &pool,
                &RadiusRequest::from_json(&json!({
                    "User-Name": username,
                    "User-Password": "correct horse battery"
                }))
                .expect("req"),
            )
            .await
        }));
    }
    for handle in handles {
        assert_eq!(handle.await.expect("task").expect("auth").status, 200);
    }
    let concurrent = accounts::get_account_by_username(&pool, &concurrent_name)
        .await
        .expect("get")
        .expect("exists");
    assert_eq!(concurrent.status, AccountStatus::Active);
    let active_assignments: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM tbl_account_packages WHERE account_id = ? AND status = 'active'",
    )
    .bind(concurrent.id)
    .fetch_one(&pool)
    .await
    .expect("count");
    assert_eq!(active_assignments, 1, "only one activation may win");

    // ---- voucher authorization + authentication (PAP + CHAP) ----
    let batch = vouchers::create_batch(
        &pool,
        &BatchRequest::from_json(&json!({ "package_id": package_id, "quantity": 1 }))
            .expect("batch"),
    )
    .await
    .expect("batch");
    let code = batch.codes[0].clone();

    let outcome = authorize(&pool, &request(json!({ "User-Name": code })))
        .await
        .expect("a");
    assert_eq!(outcome.status, 200);
    assert_eq!(attr(&outcome.body, "control:Auth-Type"), Some("rest"));
    assert_eq!(
        authenticate(
            &pool,
            &request(json!({ "User-Name": code, "User-Password": "WRONG" }))
        )
        .await
        .expect("a")
        .status,
        401
    );
    assert_eq!(
        authenticate(
            &pool,
            &request(json!({ "User-Name": code, "User-Password": code }))
        )
        .await
        .expect("a")
        .status,
        200
    );

    let chap_batch = vouchers::create_batch(
        &pool,
        &BatchRequest::from_json(&json!({ "package_id": package_id, "quantity": 1 }))
            .expect("batch"),
    )
    .await
    .expect("batch");
    let chap_code = chap_batch.codes[0].clone();
    let chap_id = 3u8;
    let challenge = [9u8, 8, 7, 6, 5, 4, 3, 2];
    let mut hasher = Md5::new();
    hasher.update([chap_id]);
    hasher.update(chap_code.as_bytes());
    hasher.update(challenge);
    let digest = hasher.finalize();
    let mut packet = vec![chap_id];
    packet.extend_from_slice(&digest);
    let hex = |bytes: &[u8]| bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
    assert_eq!(
        authenticate(
            &pool,
            &request(json!({
                "User-Name": chap_code,
                "CHAP-Password": hex(&packet),
                "CHAP-Challenge": hex(&challenge)
            }))
        )
        .await
        .expect("chap")
        .status,
        200
    );

    // ---- accounting: 64-bit gigawords + idempotent (no double count) ----
    accounting::process(
        &pool,
        &request(json!({
            "Acct-Status-Type": "Start",
            "User-Name": username,
            "Acct-Session-Id": "sess-1",
            "NAS-IP-Address": "10.0.0.1"
        })),
    )
    .await
    .expect("start");

    let stop = json!({
        "Acct-Status-Type": "Stop",
        "User-Name": username,
        "Acct-Session-Id": "sess-1",
        "NAS-IP-Address": "10.0.0.1",
        "Acct-Session-Time": 60,
        "Acct-Input-Octets": 1_000u64,
        "Acct-Input-Gigawords": 2u64,
        "Acct-Output-Octets": 500u64
    });
    accounting::process(&pool, &request(stop.clone()))
        .await
        .expect("stop");
    accounting::process(&pool, &request(stop))
        .await
        .expect("stop replay");

    let row = sqlx::query(
        "SELECT input_octets, output_octets, session_time FROM tbl_radius_sessions \
         WHERE username = ? AND acct_session_id = 'sess-1'",
    )
    .bind(&username)
    .fetch_one(&pool)
    .await
    .expect("session");
    let input: u64 = row.try_get("input_octets").unwrap();
    let output: u64 = row.try_get("output_octets").unwrap();
    let secs: u64 = row.try_get("session_time").unwrap();
    assert_eq!(input, 1_000 + 2 * (1u64 << 32), "64-bit gigawords combined");
    assert_eq!(output, 500);
    assert_eq!(secs, 60);

    // Replaying the same Stop must not double-count the account totals.
    let account = accounts::get_account(&pool, account_id)
        .await
        .expect("get")
        .expect("exists");
    assert_eq!(account.bytes_in, 1_000 + 2 * (1u64 << 32));
    assert_eq!(account.bytes_out, 500);
    assert_eq!(account.usage_secs, 60);

    // ---- maintenance promotes a due scheduled renewal ----
    accounts::renew_account(
        &pool,
        account_id,
        &RenewalRequest::from_json(
            &json!({ "package_id": package_id, "activation": "next_expiry" }),
        )
        .expect("renewal"),
    )
    .await
    .expect("renew");
    sqlx::query(
        "UPDATE tbl_account_packages SET starts_at = DATE_SUB(NOW(), INTERVAL 1 MINUTE) \
         WHERE account_id = ? AND status = 'scheduled'",
    )
    .bind(account_id)
    .execute(&pool)
    .await
    .expect("force due");
    let report = run_maintenance(&pool).await.expect("maintenance");
    assert!(report.renewals_activated >= 1);

    cleanup(
        &pool,
        &username,
        &[package_id],
        &[batch.batch_id, chap_batch.batch_id],
    )
    .await;
    cleanup(&pool, &concurrent_name, &[], &[]).await;
}
