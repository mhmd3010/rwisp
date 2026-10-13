use serde_json::json;

use rwisp_server::db::init_pool;
use rwisp_server::packages::{
    self, PackageError, PackageInput, assign_package, create_package, delete_package, get_package,
    list_packages, list_router_package_ids, package_is_global, unassign_package, update_package,
};

fn enabled() -> bool {
    std::env::var("RWISP_RUN_DB_CRUD_TESTS").as_deref() == Ok("1")
        && std::env::var("DATABASE_HOST").is_ok()
}

#[tokio::test]
async fn package_repository_round_trip() {
    dotenvy::dotenv().ok();
    if !enabled() {
        eprintln!("skipping package repository test: set RWISP_RUN_DB_CRUD_TESTS=1 and DATABASE_*");
        return;
    }

    let pool = init_pool().await.expect("database pool");
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let name = format!("pkg-test-{}-{unique}", std::process::id());

    let input = PackageInput::from_json(&json!({
        "name": name,
        "description": "integration test package",
        "price_cents": 2500,
        "validity_secs": 86400,
        "data_limit_bytes": 1073741824u64,
        "upload_rate_bps": 1000000,
        "download_rate_bps": 2000000,
        "device_limit": 1,
        "usage_limit_secs": 0
    }))
    .expect("valid input");

    let id = create_package(&pool, &input).await.expect("create package");

    let record = get_package(&pool, id)
        .await
        .expect("get package")
        .expect("package exists");
    assert_eq!(record.name, name);
    assert_eq!(record.price_cents, 2500);
    assert_eq!(record.validity_secs, Some(86400));
    assert_eq!(record.usage_limit_secs, None);
    assert_eq!(record.device_limit, Some(1));

    assert!(
        list_packages(&pool, true)
            .await
            .expect("list")
            .iter()
            .any(|package| package.id == id)
    );

    // Duplicate name must be reported as a conflict, not a 500.
    let error = create_package(&pool, &input)
        .await
        .expect_err("duplicate name should conflict");
    assert!(matches!(error, PackageError::Conflict { .. }));

    let mut updated = input.clone();
    updated.enabled = false;
    updated.price_cents = 3000;
    assert!(update_package(&pool, id, &updated).await.expect("update"));
    let record = get_package(&pool, id).await.expect("get").expect("exists");
    assert!(!record.enabled);
    assert_eq!(record.price_cents, 3000);

    // A package with no mapping rows is global.
    assert!(package_is_global(&pool, id).await.expect("global check"));

    if let Some(router_id) = rwisp_server::routers::list_routers(&pool, true)
        .await
        .expect("list routers")
        .first()
        .map(|router| router.id)
    {
        assign_package(&pool, router_id, id)
            .await
            .expect("assign package");
        assert!(
            list_router_package_ids(&pool, router_id)
                .await
                .expect("list ids")
                .contains(&id)
        );
        assert!(
            packages::list_packages_for_router(&pool, router_id, true)
                .await
                .expect("list for router")
                .iter()
                .any(|package| package.id == id)
        );
        assert!(!package_is_global(&pool, id).await.expect("global check"));

        assert!(
            unassign_package(&pool, router_id, id)
                .await
                .expect("unassign")
        );
        assert!(package_is_global(&pool, id).await.expect("global check"));
    } else {
        eprintln!("no routers registered; skipping package assignment checks");
    }

    assert!(delete_package(&pool, id).await.expect("delete"));
    assert!(get_package(&pool, id).await.expect("get").is_none());
}
