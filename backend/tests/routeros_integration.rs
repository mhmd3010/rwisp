use std::time::Duration;

use rwisp_server::crypto::EncryptionKey;
use rwisp_server::db::init_pool;
use rwisp_server::routers::{self, NewRouter, RouterError, RouterUpdate};
use rwisp_server::rwispros::{RouterClient, RouterConfig, RouterOsError};
use sqlx::Row;

const ERROR_TIMEOUT_MS: u64 = 1_000;

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|value| !value.is_empty())
}

macro_rules! live_router_client {
    () => {
        match live_router_client().await {
            Ok(Some(client)) => client,
            Ok(None) => {
                eprintln!(
                    "skipping live RouterOS test: set ROUTER_TEST_ID, DATABASE_* and \
                     ROUTER_ENCRYPTION_KEY to run it"
                );
                return;
            }
            Err(error) => panic!("failed to prepare live RouterOS test: {error}"),
        }
    };
}

async fn live_router_client() -> Result<Option<RouterClient>, String> {
    dotenvy::dotenv().ok();

    let id_raw = match env("ROUTER_TEST_ID") {
        Some(value) => value,
        None => return Ok(None),
    };

    if env("DATABASE_HOST").is_none() || env("ROUTER_ENCRYPTION_KEY").is_none() {
        return Ok(None);
    }

    let id: u64 = id_raw
        .trim()
        .parse()
        .map_err(|_| "ROUTER_TEST_ID must be a positive integer".to_string())?;

    let pool = init_pool()
        .await
        .map_err(|error| format!("database connection failed: {error}"))?;
    let key = EncryptionKey::from_env().map_err(|error| error.to_string())?;

    match routers::connect_router(&pool, id, &key).await {
        Ok(client) => Ok(Some(client)),
        Err(RouterError::Disabled { .. }) => {
            eprintln!("skipping live RouterOS test: router {id} is disabled");
            Ok(None)
        }
        Err(RouterError::NotFound { .. }) => {
            Err(format!("no router with id {id} exists in tbl_routers"))
        }
        Err(error) => Err(error.to_string()),
    }
}

#[tokio::test]
async fn loads_router_record_from_database() {
    dotenvy::dotenv().ok();

    let id_raw = match env("ROUTER_TEST_ID") {
        Some(value) => value,
        None => {
            eprintln!("skipping database router test: set ROUTER_TEST_ID and DATABASE_* to run it");
            return;
        }
    };
    if env("DATABASE_HOST").is_none() {
        eprintln!("skipping database router test: set ROUTER_TEST_ID and DATABASE_* to run it");
        return;
    }

    let id: u64 = id_raw
        .trim()
        .parse()
        .expect("ROUTER_TEST_ID must be a positive integer");
    let pool = init_pool()
        .await
        .expect("expected to connect to the RWISP database");

    let record = routers::get_router(&pool, id)
        .await
        .expect("expected to query tbl_routers")
        .expect("expected the configured ROUTER_TEST_ID to exist in tbl_routers");

    assert_eq!(record.id, id);
    assert!(
        !record.name.trim().is_empty(),
        "router name should not be empty"
    );
    assert!(
        !record.host.trim().is_empty(),
        "router host should not be empty"
    );

    let debug_output = format!("{record:?}");
    assert!(
        !debug_output.contains("password"),
        "router record debug output must not expose credentials"
    );
}

#[tokio::test]
async fn connects_and_authenticates_via_database() {
    let client = live_router_client!();
    assert!(!client.host().trim().is_empty());
}

#[tokio::test]
async fn reads_router_identity() {
    let client = live_router_client!();

    let identity = client
        .get_identity()
        .await
        .expect("expected to read the router identity");

    assert!(
        !identity.name.trim().is_empty(),
        "router identity name should not be empty"
    );
}

#[tokio::test]
async fn reads_system_resources() {
    let client = live_router_client!();

    let resources = client
        .get_resources()
        .await
        .expect("expected to read system resources");

    assert!(
        resources.version.is_some(),
        "RouterOS should report a version"
    );

    if let Some(total_memory) = resources.total_memory {
        assert!(total_memory > 0, "total memory should be greater than zero");
    }
}

#[tokio::test]
async fn reports_unreachable_router_clearly() {
    let config = RouterConfig::new("127.0.0.1", "rwisp-test", "rwisp-test-credential")
        .with_port(1)
        .with_timeout(Duration::from_secs(2));

    let error = RouterClient::connect(config)
        .await
        .expect_err("connecting to a closed port should fail");

    assert!(
        matches!(
            error,
            RouterOsError::Unreachable { .. }
                | RouterOsError::Io { .. }
                | RouterOsError::Timeout { .. }
        ),
        "unexpected error variant: {error:?}"
    );
    assert!(!error.to_string().is_empty());
}

#[test]
fn encrypts_and_decrypts_passwords() {
    let key = EncryptionKey::from_hex(&"ab".repeat(32)).expect("expected a valid 32-byte key");
    let plaintext = "router-super-secret";

    let encrypted = key
        .encrypt(plaintext)
        .expect("expected encryption to succeed");
    assert_ne!(
        encrypted,
        plaintext.as_bytes(),
        "stored value must not be the plaintext password"
    );

    let again = key
        .encrypt(plaintext)
        .expect("expected encryption to succeed");
    assert_ne!(encrypted, again, "each encryption should use a fresh nonce");

    let decrypted = key
        .decrypt(&encrypted)
        .expect("expected decryption to succeed");
    assert_eq!(decrypted, plaintext);
}

#[test]
fn rejects_invalid_encryption_keys() {
    assert!(EncryptionKey::from_hex("").is_err());
    assert!(EncryptionKey::from_hex("not-hex").is_err());
    assert!(EncryptionKey::from_hex(&"ab".repeat(16)).is_err());
    assert!(EncryptionKey::from_hex(&"a".repeat(63)).is_err());
}

#[test]
fn redacts_encryption_key_in_debug() {
    let key = EncryptionKey::from_hex(&"cd".repeat(32)).expect("expected a valid key");
    assert_eq!(format!("{key:?}"), "EncryptionKey(<redacted>)");
}

#[tokio::test]
async fn times_out_on_unresponsive_endpoint() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("failed to bind a local test listener");
    let port = listener
        .local_addr()
        .expect("failed to read local listener address")
        .port();

    let server = tokio::spawn(async move {
        if let Ok((stream, _)) = listener.accept().await {
            tokio::time::sleep(Duration::from_secs(5)).await;
            drop(stream);
        }
    });

    let config = RouterConfig::new("127.0.0.1", "rwisp-test", "rwisp-test-credential")
        .with_port(port)
        .with_timeout(Duration::from_millis(ERROR_TIMEOUT_MS));

    let error = RouterClient::connect(config)
        .await
        .expect_err("an unresponsive endpoint should time out");

    assert!(
        matches!(error, RouterOsError::Timeout { .. }),
        "unexpected error variant: {error:?}"
    );

    server.abort();
}

#[tokio::test]
async fn repository_round_trips_a_router_record() {
    dotenvy::dotenv().ok();

    if env("RWISP_RUN_DB_CRUD_TESTS").as_deref() != Some("1") {
        eprintln!("skipping router CRUD test: set RWISP_RUN_DB_CRUD_TESTS=1 to enable");
        return;
    }
    if env("DATABASE_HOST").is_none() {
        eprintln!("skipping router CRUD test: set DATABASE_* to run it");
        return;
    }

    let pool = init_pool()
        .await
        .expect("expected to connect to the database");
    let key = EncryptionKey::from_hex(&"ef".repeat(32)).expect("expected a valid test key");
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock should be after the unix epoch")
        .as_nanos();
    let name = format!("rwisp-crud-test-{}-{unique}", std::process::id());
    let plaintext = "crud-test-credential";

    let new = NewRouter::new(&name, "127.0.0.1", "rwisp-test", plaintext)
        .with_port(8728)
        .enabled(false);
    let id = routers::create_router(&pool, &new, &key)
        .await
        .expect("expected to create the test router");

    let stored: Vec<u8> = sqlx::query("SELECT password_encrypted FROM tbl_routers WHERE id = ?")
        .bind(id)
        .fetch_one(&pool)
        .await
        .expect("expected to read the stored credential")
        .try_get("password_encrypted")
        .expect("password_encrypted column should be readable");
    assert_ne!(
        stored,
        plaintext.as_bytes(),
        "password must not be stored in plaintext"
    );

    let record = routers::get_router(&pool, id)
        .await
        .expect("expected to read the test router")
        .expect("test router should exist");
    assert_eq!(record.name, name);
    assert_eq!(record.host, "127.0.0.1");
    assert!(!record.enabled);

    let mut update = RouterUpdate::from_record(&record);
    update.host = "127.0.0.2".to_string();
    let updated = routers::update_router(&pool, id, &update, &key)
        .await
        .expect("expected to update the test router");
    assert!(updated);

    let records = routers::list_routers(&pool, true)
        .await
        .expect("expected to list routers");
    assert!(
        records
            .iter()
            .any(|item| item.id == id && item.host == "127.0.0.2")
    );

    let deleted = routers::delete_router(&pool, id)
        .await
        .expect("expected to delete the test router");
    assert!(deleted);

    let gone = routers::get_router(&pool, id)
        .await
        .expect("expected to query for the deleted router");
    assert!(gone.is_none());
}
