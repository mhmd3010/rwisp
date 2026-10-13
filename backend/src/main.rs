mod api_error;
mod auth;
mod handlers;
mod state;

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::middleware;
use axum::routing::{get, post};
use rwisp_server::crypto::EncryptionKey;
use rwisp_server::db::init_pool;
use state::AppState;
use tokio::net::TcpListener;

const DEFAULT_ROUTER_API_TIMEOUT_SECS: u64 = 15;

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    let db = match init_pool().await {
        Ok(pool) => {
            println!("RWISP MariaDB connection established");
            pool
        }
        Err(error) => {
            println!("Failed to initialize database pool: {}", error);
            std::process::exit(1);
        }
    };

    let encryption_key = match EncryptionKey::from_env() {
        Ok(key) => Some(Arc::new(key)),
        Err(error) => {
            eprintln!("Router credential encryption is unavailable: {error}");
            None
        }
    };

    let admin_auth = auth::AdminAuthConfig::from_env().map(Arc::new);
    if admin_auth.is_none() {
        eprintln!("Admin API authentication is not configured (set ADMIN_API_TOKEN)");
    }

    let router_timeout = Duration::from_secs(
        std::env::var("ROUTER_API_TIMEOUT_SECS")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(DEFAULT_ROUTER_API_TIMEOUT_SECS),
    );

    let state = AppState {
        db,
        encryption_key,
        admin_auth,
        router_timeout,
    };

    let routers = Router::new()
        .route("/api/routers", get(handlers::routers::list))
        .route("/api/routers/{id}", get(handlers::routers::detail))
        .route("/api/routers/{id}/status", get(handlers::routers::status))
        .route(
            "/api/routers/{id}/resources",
            get(handlers::routers::resources),
        )
        .route(
            "/api/routers/{id}/hotspot/servers",
            get(handlers::hotspot::servers),
        )
        .route(
            "/api/routers/{id}/hotspot/profiles",
            get(handlers::hotspot::profiles),
        )
        .route(
            "/api/routers/{id}/hotspot/users",
            get(handlers::hotspot::users),
        )
        .route(
            "/api/routers/{id}/hotspot/active",
            get(handlers::hotspot::active),
        )
        .route(
            "/api/routers/{id}/hotspot/ip-bindings",
            get(handlers::hotspot::ip_bindings),
        )
        .route(
            "/api/routers/{id}/hotspot/hosts",
            get(handlers::hotspot::hosts),
        )
        .route(
            "/api/packages",
            get(handlers::packages::list).post(handlers::packages::create),
        )
        .route(
            "/api/packages/{id}",
            get(handlers::packages::detail)
                .put(handlers::packages::update)
                .delete(handlers::packages::delete),
        )
        .route(
            "/api/vouchers",
            get(handlers::vouchers::list).post(handlers::vouchers::create),
        )
        .route("/api/vouchers/{id}", get(handlers::vouchers::detail))
        .route(
            "/api/vouchers/{id}/revoke",
            post(handlers::vouchers::revoke),
        )
        .route(
            "/api/vouchers/batches/{batch_id}/codes",
            get(handlers::vouchers::batch_codes),
        )
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            auth::require_admin,
        ));

    let app = Router::new()
        .route("/api/health", get(handlers::health::health))
        .route("/api/health/db", get(handlers::health::health_db))
        .merge(routers)
        .with_state(state);

    let addr = SocketAddr::from(([127, 0, 0, 1], 3100));
    let listener = TcpListener::bind(addr).await.expect("Failed to bind port");

    println!("RWISP API running on 127.0.0.1:3100");

    axum::serve(listener, app).await.expect("Server failed");
}
