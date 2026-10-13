mod admin;
mod api_error;
mod auth;
mod handlers;
mod state;

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::middleware;
use axum::routing::{get, post, put};
use rwisp_server::crypto::EncryptionKey;
use rwisp_server::db::init_pool;
use rwisp_server::disconnects::SenderConfig;
use rwisp_server::scheduler::{self, SchedulerConfig};
use state::AppState;
use tokio::net::TcpListener;
use tokio::sync::watch;

const DEFAULT_ROUTER_API_TIMEOUT_SECS: u64 = 15;
const ADMIN_RATE_LIMIT_PER_MIN: u32 = 600;

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

    let radius_auth = auth::AdminAuthConfig::from_token_env(auth::RADIUS_TOKEN_ENV).map(Arc::new);
    if radius_auth.is_none() {
        eprintln!("RADIUS internal authentication is not configured (set RADIUS_INTERNAL_TOKEN)");
    }

    let rate_limiter = Some(Arc::new(admin::RateLimiter::new(
        std::env::var("ADMIN_RATE_LIMIT_PER_MIN")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(ADMIN_RATE_LIMIT_PER_MIN),
        60,
    )));

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
        radius_auth,
        rate_limiter,
        router_timeout,
    };

    let routers = Router::new()
        .route(
            "/api/routers",
            get(handlers::routers::list).post(handlers::routers::create),
        )
        .route(
            "/api/routers/{id}",
            get(handlers::routers::detail)
                .put(handlers::routers::update)
                .delete(handlers::routers::delete),
        )
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
        .route(
            "/api/accounts",
            get(handlers::accounts::list).post(handlers::accounts::create),
        )
        .route(
            "/api/accounts/{id}",
            get(handlers::accounts::detail)
                .put(handlers::accounts::update)
                .delete(handlers::accounts::delete),
        )
        .route(
            "/api/accounts/{id}/password",
            post(handlers::accounts::reset_password),
        )
        .route(
            "/api/accounts/{id}/suspend",
            post(handlers::accounts::suspend),
        )
        .route(
            "/api/accounts/{id}/reactivate",
            post(handlers::accounts::reactivate),
        )
        .route("/api/accounts/{id}/renew", post(handlers::accounts::renew))
        .route(
            "/api/accounts/{id}/packages",
            get(handlers::accounts::packages),
        )
        .route(
            "/api/routers/{id}/secrets",
            post(handlers::routers::set_secrets),
        )
        .route(
            "/api/routers/{id}/status/history",
            get(handlers::routers::status_history),
        )
        .route(
            "/api/routers/{id}/provision/plan",
            get(handlers::routers::provisioning_plan),
        )
        .route(
            "/api/routers/{id}/provision/apply",
            post(handlers::routers::provisioning_apply),
        )
        .route("/api/sessions", get(handlers::sessions::list))
        .route("/api/sessions/stats", get(handlers::sessions::stats))
        .route("/api/sessions/stale", get(handlers::sessions::stale))
        .route("/api/sessions/history", get(handlers::sessions::history))
        .route(
            "/api/sessions/reconcile",
            post(handlers::sessions::reconcile),
        )
        .route("/api/sessions/{id}", get(handlers::sessions::detail))
        .route(
            "/api/sessions/{id}/disconnect",
            post(handlers::sessions::disconnect),
        )
        .route(
            "/api/disconnects",
            get(handlers::disconnects::list).post(handlers::disconnects::create),
        )
        .route("/api/disconnects/{id}", get(handlers::disconnects::detail))
        .route(
            "/api/disconnects/{id}/retry",
            post(handlers::disconnects::retry),
        )
        .route("/api/dashboard", get(handlers::dashboard::dashboard))
        .route("/api/reports/traffic", get(handlers::dashboard::traffic))
        .route(
            "/api/reports/sessions",
            get(handlers::dashboard::sessions_report),
        )
        .route("/api/scheduler/jobs", get(handlers::scheduler::jobs))
        .route(
            "/api/admin/tokens",
            get(handlers::admin::list_tokens).post(handlers::admin::create_token),
        )
        .route("/api/admin/tokens/{id}", put(handlers::admin::revoke_token))
        .route("/api/admin/audit", get(handlers::admin::audit_log))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            auth::require_admin,
        ));

    let scheduler_pool = state.db.clone();
    let scheduler_key = state.encryption_key.clone();

    let internal = Router::new()
        .route(
            "/internal/radius/authorize",
            post(handlers::radius::authorize),
        )
        .route(
            "/internal/radius/authenticate",
            post(handlers::radius::authenticate),
        )
        .route(
            "/internal/radius/accounting",
            post(handlers::radius::accounting),
        )
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            auth::require_internal,
        ));

    let app = Router::new()
        .route("/api/health", get(handlers::health::health))
        .route("/api/health/db", get(handlers::health::health_db))
        .merge(routers)
        .merge(internal)
        .with_state(state);

    let addr = SocketAddr::from(([127, 0, 0, 1], 3100));
    let listener = TcpListener::bind(addr).await.expect("Failed to bind port");

    println!("RWISP API running on 127.0.0.1:3100");

    // Background scheduler (multi-instance safe via DB leases). Disable with
    // SCHEDULER_ENABLED=false to run multiple API instances against one worker.
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let scheduler_enabled = std::env::var("SCHEDULER_ENABLED")
        .map(|v| !matches!(v.as_str(), "0" | "false" | "no"))
        .unwrap_or(true);

    if scheduler_enabled {
        let config = SchedulerConfig {
            owner: std::env::var("SCHEDULER_OWNER")
                .unwrap_or_else(|_| format!("rwisp-{}", std::process::id())),
            tick_secs: std::env::var("SCHEDULER_TICK_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(15),
            lease_secs: std::env::var("SCHEDULER_LEASE_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(120),
            disconnect_batch: std::env::var("DISCONNECT_BATCH")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(50),
            router_status_retention_days: std::env::var("ROUTER_STATUS_RETENTION_DAYS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(30),
            encryption_key: scheduler_key.clone(),
            sender: SenderConfig {
                radclient_path: std::env::var("RADCLIENT_PATH")
                    .unwrap_or_else(|_| "radclient".to_string()),
                coa_secret_fallback: std::env::var("RADIUS_COA_SECRET").ok(),
                timeout: Duration::from_secs(5),
                encryption_key: scheduler_key,
            },
        };
        let pool = scheduler_pool.clone();
        tokio::spawn(async move {
            scheduler::run(pool, config, shutdown_rx).await;
        });
    } else {
        println!("RWISP scheduler disabled (SCHEDULER_ENABLED=false)");
    }

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .expect("Server failed");

    let _ = shutdown_tx.send(true);
    println!("RWISP shutdown complete");
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {}
        _ = terminate => {}
    }
}
