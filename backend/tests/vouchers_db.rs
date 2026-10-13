use serde_json::json;

use rwisp_server::db::init_pool;
use rwisp_server::packages::{PackageInput, create_package, delete_package};
use rwisp_server::vouchers::{
    self, ActivationOutcome, BatchRequest, RevokeOutcome, VoucherFilter, VoucherStatus,
};

fn enabled() -> bool {
    std::env::var("RWISP_RUN_DB_CRUD_TESTS").as_deref() == Ok("1")
        && std::env::var("DATABASE_HOST").is_ok()
}

#[tokio::test]
async fn voucher_lifecycle_and_atomic_activation() {
    dotenvy::dotenv().ok();
    if !enabled() {
        eprintln!("skipping voucher repository test: set RWISP_RUN_DB_CRUD_TESTS=1 and DATABASE_*");
        return;
    }

    let pool = init_pool().await.expect("database pool");
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_nanos();

    let package_input = PackageInput::from_json(&json!({
        "name": format!("voucher-pkg-{}-{unique}", std::process::id()),
        "price_cents": 100,
        "validity_secs": 3600
    }))
    .expect("package input");
    let package_id = create_package(&pool, &package_input)
        .await
        .expect("create package");

    let router_id = rwisp_server::routers::list_routers(&pool, true)
        .await
        .expect("list routers")
        .first()
        .map(|router| router.id);

    let request = BatchRequest::from_json(&json!({
        "package_id": package_id,
        "quantity": 5,
        "router_id": router_id,
        "note": "db test batch"
    }))
    .expect("batch request");

    let batch = vouchers::create_batch(&pool, &request)
        .await
        .expect("create batch");
    assert_eq!(batch.codes.len(), 5);

    // Validity must not start at generation.
    let first = vouchers::get_voucher_by_code(&pool, &batch.codes[0])
        .await
        .expect("get by code")
        .expect("voucher exists");
    assert_eq!(first.status, VoucherStatus::Unused);
    assert_eq!(first.activated_at, None);
    assert_eq!(first.expires_at, None);

    // Listing and filtering.
    let filter = VoucherFilter {
        package_id: Some(package_id),
        batch_id: Some(batch.batch_id),
        ..VoucherFilter::default()
    };
    let listed = vouchers::list_vouchers(&pool, &filter).await.expect("list");
    assert_eq!(listed.len(), 5);

    let codes = vouchers::list_batch_codes(&pool, batch.batch_id)
        .await
        .expect("batch codes");
    assert_eq!(codes.len(), 5);

    // Concurrent first logins: exactly one activation wins.
    let code = batch.codes[1].clone();
    let mut handles = Vec::new();
    for _ in 0..10 {
        let pool = pool.clone();
        let code = code.clone();
        handles.push(tokio::spawn(async move {
            vouchers::activate_voucher(&pool, &code).await
        }));
    }

    let mut activated = 0;
    for handle in handles {
        match handle.await.expect("task").expect("activation") {
            ActivationOutcome::Activated(_) => activated += 1,
            ActivationOutcome::NotUnused { .. } => {}
            ActivationOutcome::NotFound => panic!("voucher disappeared"),
        }
    }
    assert_eq!(activated, 1, "exactly one concurrent activation may win");

    let activated_voucher = vouchers::get_voucher_by_code(&pool, &code)
        .await
        .expect("get")
        .expect("exists");
    assert_eq!(activated_voucher.status, VoucherStatus::Active);
    assert!(activated_voucher.activated_at.is_some());
    assert!(
        activated_voucher.expires_at.is_some(),
        "validity 3600 should set expiry"
    );

    // Re-activation is rejected and does not extend validity.
    assert!(matches!(
        vouchers::activate_voucher(&pool, &code)
            .await
            .expect("activate"),
        ActivationOutcome::NotUnused {
            status: VoucherStatus::Active
        }
    ));

    // Revocation states.
    assert!(matches!(
        vouchers::revoke_voucher(&pool, activated_voucher.id)
            .await
            .expect("revoke"),
        RevokeOutcome::Revoked(_)
    ));
    assert!(matches!(
        vouchers::revoke_voucher(&pool, activated_voucher.id)
            .await
            .expect("revoke again"),
        RevokeOutcome::NotRevocable {
            status: VoucherStatus::Revoked
        }
    ));

    // Expiry maintenance transitions overdue active vouchers.
    let overdue_code = &batch.codes[2];
    vouchers::activate_voucher(&pool, overdue_code)
        .await
        .expect("activate overdue");
    sqlx::query(
        "UPDATE tbl_vouchers SET expires_at = DATE_SUB(NOW(), INTERVAL 1 MINUTE) WHERE code = ?",
    )
    .bind(overdue_code)
    .execute(&pool)
    .await
    .expect("force expiry");
    assert!(vouchers::expire_overdue(&pool).await.expect("expire") >= 1);
    let expired = vouchers::get_voucher_by_code(&pool, overdue_code)
        .await
        .expect("get")
        .expect("exists");
    assert_eq!(expired.status, VoucherStatus::Expired);

    // Cleanup (vouchers reference the package with RESTRICT, so remove them first).
    sqlx::query("DELETE FROM tbl_vouchers WHERE package_id = ?")
        .bind(package_id)
        .execute(&pool)
        .await
        .expect("cleanup vouchers");
    sqlx::query("DELETE FROM tbl_voucher_batches WHERE id = ?")
        .bind(batch.batch_id)
        .execute(&pool)
        .await
        .expect("cleanup batch");
    assert!(
        delete_package(&pool, package_id)
            .await
            .expect("cleanup package")
    );
}
