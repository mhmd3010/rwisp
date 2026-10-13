use rwisp_server::db::init_pool;
use rwisp_server::disconnects::{
    self, DisconnectFilter, DisconnectMethod, DisconnectStatus, NewDisconnect, backoff_secs,
};
use rwisp_server::sessions::{self, SessionFilter};

fn enabled() -> bool {
    std::env::var("RWISP_RUN_DB_CRUD_TESTS").as_deref() == Ok("1")
        && std::env::var("DATABASE_HOST").is_ok()
}

#[test]
fn backoff_is_bounded_and_monotonic() {
    assert_eq!(backoff_secs(1), 10);
    assert_eq!(backoff_secs(2), 20);
    assert!(backoff_secs(6) <= 300);
    assert!(backoff_secs(100) <= 300);
}

#[test]
fn disconnect_filter_rejects_bad_status() {
    let mut params = std::collections::HashMap::new();
    params.insert("status".to_string(), "bogus".to_string());
    assert!(DisconnectFilter::from_query(&params).is_err());
    params.insert("status".to_string(), "pending".to_string());
    let filter = DisconnectFilter::from_query(&params).expect("ok");
    assert_eq!(filter.status, Some(DisconnectStatus::Pending));
}

#[test]
fn session_filter_parses_online_flag() {
    let mut params = std::collections::HashMap::new();
    params.insert("online".to_string(), "true".to_string());
    params.insert("mac".to_string(), "AA:BB:CC:DD:EE:FF".to_string());
    let filter = SessionFilter::from_query(&params).expect("ok");
    assert!(filter.online_only);
    assert_eq!(
        filter.calling_station_id.as_deref(),
        Some("AA:BB:CC:DD:EE:FF")
    );
}

#[tokio::test]
async fn sessions_and_disconnects() {
    dotenvy::dotenv().ok();
    if !enabled() {
        eprintln!("skipping phase2 DB test: set RWISP_RUN_DB_CRUD_TESTS=1 and DATABASE_*");
        return;
    }

    let pool = init_pool().await.expect("pool");
    let tag = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let user = format!("phase2-{}-{tag}", std::process::id());

    // Online + stopped + stale sessions.
    sqlx::query(
        "INSERT INTO tbl_radius_sessions \
         (username, acct_session_id, nas_ip, status, session_time, input_octets, output_octets, \
          started_at, updated_at, stopped_at) VALUES \
         (?, ?, '10.9.9.9', 'start', 30, 100, 200, NOW(), NOW(), NULL), \
         (?, ?, '10.9.9.9', 'stop', 60, 1000, 2000, NOW(), NOW(), NOW()), \
         (?, ?, '10.9.9.9', 'start', 5, 1, 1, DATE_SUB(NOW(), INTERVAL 2 HOUR), \
          DATE_SUB(NOW(), INTERVAL 2 HOUR), NULL)",
    )
    .bind(&user)
    .bind(format!("{tag}-a"))
    .bind(&user)
    .bind(format!("{tag}-b"))
    .bind(&user)
    .bind(format!("{tag}-c"))
    .execute(&pool)
    .await
    .expect("insert sessions");

    let filter = SessionFilter {
        username: Some(user.clone()),
        ..SessionFilter::default()
    };
    let list = sessions::list_sessions(&pool, &filter).await.expect("list");
    assert_eq!(list.len(), 3);

    let online_filter = SessionFilter {
        username: Some(user.clone()),
        online_only: true,
        ..SessionFilter::default()
    };
    let online = sessions::list_sessions(&pool, &online_filter)
        .await
        .expect("online");
    assert_eq!(online.len(), 2, "stopped session must not count as online");

    let stale = sessions::stale_sessions(&pool).await.expect("stale");
    assert!(
        stale.iter().any(|s| s.username == user),
        "the stale session should be detected"
    );

    assert_eq!(
        sessions::open_count_for_username(&pool, &user)
            .await
            .expect("count"),
        2
    );

    // Idempotent enqueue: a second enqueue for the same session returns the same id.
    let request = NewDisconnect {
        username: user.clone(),
        account_id: None,
        voucher_id: None,
        session_row_id: None,
        router_id: None,
        nas_ip: Some("10.9.9.9".to_string()),
        acct_session_id: Some(format!("{tag}-a")),
        reason: "test".to_string(),
        method: DisconnectMethod::None,
        max_attempts: 1,
    };
    let first = disconnects::enqueue(&pool, &request)
        .await
        .expect("enqueue");
    let second = disconnects::enqueue(&pool, &request)
        .await
        .expect("enqueue again");
    assert_eq!(first, second, "pending disconnects must be idempotent");

    let due = disconnects::list_due(&pool, 100).await.expect("due");
    assert!(due.iter().any(|d| d.id == first));

    // max_attempts=1 => a single retry marks it failed, then requeue resets it.
    disconnects::mark_retry(&pool, first, "boom", backoff_secs(1))
        .await
        .expect("retry");
    let after = disconnects::get(&pool, first)
        .await
        .expect("get")
        .expect("exists");
    assert_eq!(after.status, DisconnectStatus::Failed);

    assert!(disconnects::requeue(&pool, first).await.expect("requeue"));
    let after = disconnects::get(&pool, first)
        .await
        .expect("get")
        .expect("exists");
    assert_eq!(after.status, DisconnectStatus::Pending);

    // Cleanup (test-only rows).
    sqlx::query("DELETE FROM tbl_disconnects WHERE username = ?")
        .bind(&user)
        .execute(&pool)
        .await
        .expect("cleanup disconnects");
    sqlx::query("DELETE FROM tbl_radius_sessions WHERE username = ?")
        .bind(&user)
        .execute(&pool)
        .await
        .expect("cleanup sessions");
}
