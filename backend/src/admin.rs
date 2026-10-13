use std::collections::HashMap;
use std::time::{Duration, Instant};

use sqlx::{MySqlPool, Row};
use tokio::sync::Mutex;

use rwisp_server::accounts::verify_password;

use crate::api_error::ApiError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Role {
    Readonly,
    Operator,
    Admin,
}

impl Role {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Readonly => "readonly",
            Self::Operator => "operator",
            Self::Admin => "admin",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "readonly" => Some(Self::Readonly),
            "operator" => Some(Self::Operator),
            "admin" => Some(Self::Admin),
            _ => None,
        }
    }
}

/// Authenticated administrator identity attached to each request.
#[derive(Debug, Clone)]
pub struct AdminIdentity {
    pub actor: String,
    pub role: Role,
}

impl AdminIdentity {
    pub fn require(&self, minimum: Role) -> Result<(), ApiError> {
        if self.role >= minimum {
            Ok(())
        } else {
            Err(ApiError::forbidden(format!(
                "requires role {} or higher",
                minimum.as_str()
            )))
        }
    }
}

/// Resolve a bearer token against the DB admin-token store (Argon2id hashes).
/// The bootstrap token is handled separately by the auth middleware.
pub async fn verify_db_token(
    pool: &MySqlPool,
    token: &str,
) -> Result<Option<AdminIdentity>, sqlx::Error> {
    let rows =
        sqlx::query("SELECT id, name, token_hash, role FROM tbl_admin_tokens WHERE enabled = 1")
            .fetch_all(pool)
            .await?;

    for row in rows {
        let hash: String = row.try_get("token_hash")?;
        if verify_password(token, &hash).unwrap_or(false) {
            let name: String = row.try_get("name")?;
            let role_raw: String = row.try_get("role")?;
            let role = Role::parse(&role_raw).unwrap_or(Role::Readonly);
            let id: u64 = row.try_get("id")?;
            sqlx::query("UPDATE tbl_admin_tokens SET last_used_at = NOW() WHERE id = ?")
                .bind(id)
                .execute(pool)
                .await?;
            return Ok(Some(AdminIdentity { actor: name, role }));
        }
    }
    Ok(None)
}

pub async fn create_db_token(
    pool: &MySqlPool,
    name: &str,
    token: &str,
    role: Role,
) -> Result<u64, String> {
    let hash = rwisp_server::accounts::hash_password(token).map_err(|e| e.to_string())?;
    let result = sqlx::query(
        "INSERT INTO tbl_admin_tokens (name, token_hash, role) VALUES (?, ?, ?) \
         ON DUPLICATE KEY UPDATE token_hash = VALUES(token_hash), role = VALUES(role), enabled = 1",
    )
    .bind(name)
    .bind(hash)
    .bind(role.as_str())
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(result.last_insert_id())
}

pub async fn revoke_db_token(pool: &MySqlPool, id: u64) -> Result<bool, sqlx::Error> {
    let result = sqlx::query("UPDATE tbl_admin_tokens SET enabled = 0 WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn audit(
    pool: &MySqlPool,
    actor: &str,
    role: Role,
    action: &str,
    target_type: Option<&str>,
    target_id: Option<&str>,
    detail: Option<&str>,
) {
    let _ = sqlx::query(
        "INSERT INTO tbl_admin_audit (actor, role, action, target_type, target_id, detail) \
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(actor)
    .bind(role.as_str())
    .bind(action)
    .bind(target_type)
    .bind(target_id)
    .bind(detail)
    .execute(pool)
    .await;
}

/// Minimal in-memory rate limiter (per actor). Bounded to a fixed number of
/// actors to cap memory usage.
pub struct RateLimiter {
    state: Mutex<HashMap<String, (Instant, u32)>>,
    max_requests: u32,
    window: Duration,
}

impl RateLimiter {
    pub fn new(max_requests: u32, window_secs: u64) -> Self {
        Self {
            state: Mutex::new(HashMap::new()),
            max_requests,
            window: Duration::from_secs(window_secs.max(1)),
        }
    }

    /// Returns true when the request is allowed.
    pub async fn check(&self, key: &str) -> bool {
        let mut state = self.state.lock().await;
        if state.len() > 4096 {
            state.clear();
        }
        let now = Instant::now();
        let entry = state.entry(key.to_string()).or_insert((now, 0));
        if now.duration_since(entry.0) >= self.window {
            *entry = (now, 1);
            return true;
        }
        entry.1 += 1;
        entry.1 <= self.max_requests
    }
}

/// Generate a cryptographically secure admin token (64 hex characters).
pub fn generate_secret() -> String {
    use aes_gcm::aead::OsRng;
    use aes_gcm::aead::rand_core::RngCore;
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::{Role, generate_secret};

    #[test]
    fn roles_are_ordered() {
        assert!(Role::Admin >= Role::Operator);
        assert!(Role::Operator >= Role::Readonly);
        assert!(Role::Readonly < Role::Admin);
        assert_eq!(Role::parse("ADMIN"), Some(Role::Admin));
        assert_eq!(Role::parse("nope"), None);
    }

    #[test]
    fn generated_secret_is_strong_and_unique() {
        let first = generate_secret();
        let second = generate_secret();
        assert_eq!(first.len(), 64);
        assert_ne!(first, second);
        assert!(first.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
