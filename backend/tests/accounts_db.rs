use serde_json::json;

use rwisp_server::accounts::{
    self, AccountFilter, AccountPackageStatus, AccountStatus, AccountUpdate, ActivationOutcome,
    NewAccount, RenewalRequest, StatusOutcome, SubscriptionStatus, verify_account_password,
};
use rwisp_server::db::init_pool;
use rwisp_server::packages::{PackageInput, create_package, delete_package};
use sqlx::MySqlPool;

fn enabled() -> bool {
    std::env::var("RWISP_RUN_DB_CRUD_TESTS").as_deref() == Ok("1")
        && std::env::var("DATABASE_HOST").is_ok()
}

async fn seconds_between(pool: &MySqlPool, from: &str, to: &str) -> i64 {
    sqlx::query_scalar("SELECT TIMESTAMPDIFF(SECOND, ?, ?)")
        .bind(from)
        .bind(to)
        .fetch_one(pool)
        .await
        .expect("timestamp diff")
}

async fn create_test_package(pool: &MySqlPool, label: &str, validity: u32, tag: u128) -> u64 {
    let input = PackageInput::from_json(&json!({
        "name": format!("{label}-{}-{tag}", std::process::id()),
        "price_cents": 100,
        "validity_secs": validity
    }))
    .expect("package input");
    create_package(pool, &input).await.expect("create package")
}

#[tokio::test]
async fn account_lifecycle_hardening() {
    dotenvy::dotenv().ok();
    if !enabled() {
        eprintln!("skipping account repository test: set RWISP_RUN_DB_CRUD_TESTS=1 and DATABASE_*");
        return;
    }

    let pool = init_pool().await.expect("database pool");
    let tag = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_nanos();

    let base_pkg = create_test_package(&pool, "acct-base", 3600, tag).await;
    let next_pkg = create_test_package(&pool, "acct-next", 1800, tag).await;

    // ---- Pending account: validity must not start before first login. ----
    let username = format!("acct-{}-{tag}", std::process::id());
    let new_account = NewAccount::from_json(&json!({
        "username": username,
        "password": "correct horse battery",
        "package_id": base_pkg
    }))
    .expect("account input");
    let id = accounts::create_account(&pool, &new_account)
        .await
        .expect("create account");

    let account = accounts::get_account(&pool, id)
        .await
        .expect("get")
        .expect("exists");
    assert_eq!(account.status, AccountStatus::Pending);
    assert_eq!(account.subscription_status, SubscriptionStatus::Scheduled);
    assert_eq!(account.activated_at, None);
    assert_eq!(account.expires_at, None);

    // Initial assignment is recorded for history even before login.
    let history = accounts::list_account_packages(&pool, id)
        .await
        .expect("history");
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].status, AccountPackageStatus::Scheduled);

    // Duplicate username conflict.
    assert!(accounts::create_account(&pool, &new_account).await.is_err());

    // Password verification.
    assert!(
        verify_account_password(&pool, &username, "correct horse battery")
            .await
            .expect("verify")
            .is_some()
    );

    // ---- Atomic first login: exactly one concurrent caller wins. ----
    let mut handles = Vec::new();
    for _ in 0..10 {
        let pool = pool.clone();
        handles.push(tokio::spawn(async move {
            accounts::activate_account(&pool, id).await
        }));
    }
    let mut activated = 0;
    for handle in handles {
        match handle.await.expect("task").expect("activate") {
            ActivationOutcome::Activated(_) => activated += 1,
            ActivationOutcome::NotPending { .. } => {}
            ActivationOutcome::NotFound => panic!("account disappeared"),
        }
    }
    assert_eq!(activated, 1, "exactly one concurrent activation may win");

    let account = accounts::get_account(&pool, id)
        .await
        .expect("get")
        .expect("exists");
    assert_eq!(account.status, AccountStatus::Active);
    assert_eq!(account.subscription_status, SubscriptionStatus::Active);
    assert!(account.activated_at.is_some());
    let expiry_after_activation = account.expires_at.clone().expect("expiry set");
    let history = accounts::list_account_packages(&pool, id)
        .await
        .expect("history");
    assert_eq!(history[0].status, AccountPackageStatus::Active);

    // ---- Immediate renewal preserves unused validity (extends). ----
    let renewal = RenewalRequest::from_json(&json!({
        "package_id": next_pkg,
        "activation": "immediate"
    }))
    .expect("renewal");
    let result = accounts::renew_account(&pool, id, &renewal)
        .await
        .expect("renew immediate");
    assert_eq!(result.assignment.status, AccountPackageStatus::Active);
    assert_eq!(result.assignment.package_id, next_pkg);
    let extended = result.account.expires_at.clone().expect("expiry");
    let added = seconds_between(&pool, &expiry_after_activation, &extended).await;
    assert_eq!(
        added, 1800,
        "immediate renewal must extend by the new package validity"
    );

    // Previous active assignment is completed, newest is active.
    let history = accounts::list_account_packages(&pool, id)
        .await
        .expect("history");
    let active: Vec<_> = history
        .iter()
        .filter(|a| a.status == AccountPackageStatus::Active)
        .collect();
    assert_eq!(active.len(), 1);

    // ---- Next-expiry renewal stays scheduled and does not change expiry. ----
    let scheduled = RenewalRequest::from_json(&json!({
        "package_id": base_pkg,
        "activation": "next_expiry"
    }))
    .expect("renewal");
    let result = accounts::renew_account(&pool, id, &scheduled)
        .await
        .expect("renew next_expiry");
    assert_eq!(result.assignment.status, AccountPackageStatus::Scheduled);
    assert_eq!(
        result.account.expires_at.as_deref(),
        Some(extended.as_str())
    );

    // ---- Suspended account stays suspended across renewal. ----
    assert!(matches!(
        accounts::suspend_account(&pool, id).await.expect("suspend"),
        StatusOutcome::Updated(_)
    ));
    let immediate = RenewalRequest::from_json(&json!({
        "package_id": next_pkg,
        "activation": "immediate"
    }))
    .expect("renewal");
    let result = accounts::renew_account(&pool, id, &immediate)
        .await
        .expect("renew suspended");
    assert_eq!(
        result.account.status,
        AccountStatus::Suspended,
        "suspended stays suspended"
    );
    assert_eq!(
        result.account.subscription_status,
        SubscriptionStatus::Active
    );

    // Reactivate returns to active because the account already activated.
    assert!(matches!(
        accounts::reactivate_account(&pool, id)
            .await
            .expect("reactivate"),
        StatusOutcome::Updated(_)
    ));

    // ---- Derived expiry: status stays account-active, subscription expires. ----
    sqlx::query(
        "UPDATE tbl_account_packages SET expires_at = DATE_SUB(NOW(), INTERVAL 1 MINUTE) \
         WHERE account_id = ? AND status = 'active'",
    )
    .bind(id)
    .execute(&pool)
    .await
    .expect("force expiry");
    let account = accounts::get_account(&pool, id)
        .await
        .expect("get")
        .expect("exists");
    assert_eq!(
        account.status,
        AccountStatus::Active,
        "account lifecycle unchanged"
    );
    assert_eq!(account.subscription_status, SubscriptionStatus::Expired);
    assert!(
        accounts::expire_overdue_subscriptions(&pool)
            .await
            .expect("expire")
            >= 1
    );

    // Renewing an expired subscription restarts from now (reactivates to active).
    let result = accounts::renew_account(&pool, id, &immediate)
        .await
        .expect("renew expired");
    assert_eq!(result.account.status, AccountStatus::Active);
    assert_eq!(
        result.account.subscription_status,
        SubscriptionStatus::Active
    );

    // Account details can be updated without touching credentials.
    let update = AccountUpdate::from_json(&json!({ "full_name": "Renamed" })).expect("update");
    assert!(
        accounts::update_account(&pool, id, &update)
            .await
            .expect("update")
    );

    // ---- Pending renewal replaces the pending assignment, keeps history. ----
    let pending_name = format!("acct-pending-{}-{tag}", std::process::id());
    let pending = NewAccount::from_json(&json!({
        "username": pending_name,
        "password": "correct horse battery",
        "package_id": base_pkg
    }))
    .expect("account input");
    let pending_id = accounts::create_account(&pool, &pending)
        .await
        .expect("create");

    let replace = RenewalRequest::from_json(&json!({
        "package_id": next_pkg,
        "activation": "immediate"
    }))
    .expect("renewal");
    let result = accounts::renew_account(&pool, pending_id, &replace)
        .await
        .expect("renew pending");
    assert_eq!(result.account.status, AccountStatus::Pending);
    assert_eq!(result.account.activated_at, None);
    assert_eq!(result.account.expires_at, None);
    assert_eq!(result.assignment.status, AccountPackageStatus::Scheduled);
    assert_eq!(result.assignment.package_id, next_pkg);

    let history = accounts::list_account_packages(&pool, pending_id)
        .await
        .expect("history");
    assert_eq!(history.len(), 2);
    assert!(
        history
            .iter()
            .any(|a| a.status == AccountPackageStatus::Cancelled)
    );
    assert!(
        history
            .iter()
            .any(|a| a.status == AccountPackageStatus::Scheduled)
    );

    // Filtering still works.
    let filter = AccountFilter {
        username: Some(format!("acct-pending-{}-{tag}", std::process::id())),
        ..AccountFilter::default()
    };
    assert_eq!(
        accounts::list_accounts(&pool, &filter)
            .await
            .expect("list")
            .len(),
        1
    );

    // Disabled accounts cannot be renewed.
    sqlx::query("UPDATE tbl_accounts SET status = 'disabled' WHERE id = ?")
        .bind(pending_id)
        .execute(&pool)
        .await
        .expect("disable");
    assert!(
        accounts::renew_account(&pool, pending_id, &replace)
            .await
            .is_err()
    );

    // ---- Cleanup: deleting the account cascades its history. ----
    assert!(accounts::delete_account(&pool, id).await.expect("delete"));
    assert!(
        accounts::delete_account(&pool, pending_id)
            .await
            .expect("delete")
    );
    assert!(delete_package(&pool, base_pkg).await.expect("cleanup"));
    assert!(delete_package(&pool, next_pkg).await.expect("cleanup"));
}
