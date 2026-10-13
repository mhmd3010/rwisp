use std::sync::Arc;
use std::time::Duration;

use sqlx::MySqlPool;

use rwisp_server::crypto::EncryptionKey;

use crate::auth::AdminAuthConfig;

#[derive(Clone)]
pub struct AppState {
    pub db: MySqlPool,
    pub encryption_key: Option<Arc<EncryptionKey>>,
    pub admin_auth: Option<Arc<AdminAuthConfig>>,
    pub radius_auth: Option<Arc<AdminAuthConfig>>,
    pub router_timeout: Duration,
}
