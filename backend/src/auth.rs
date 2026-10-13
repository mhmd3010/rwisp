use std::fmt;

use axum::extract::{Request, State};
use axum::http::header::AUTHORIZATION;
use axum::middleware::Next;
use axum::response::Response;
use subtle::ConstantTimeEq;

use crate::api_error::ApiError;
use crate::state::AppState;

pub const ADMIN_TOKEN_ENV: &str = "ADMIN_API_TOKEN";
pub const RADIUS_TOKEN_ENV: &str = "RADIUS_INTERNAL_TOKEN";
const MIN_TOKEN_LEN: usize = 32;

pub struct AdminAuthConfig {
    token: String,
}

impl AdminAuthConfig {
    pub fn from_env() -> Option<Self> {
        Self::from_token_env(ADMIN_TOKEN_ENV)
    }

    pub fn from_token_env(variable: &str) -> Option<Self> {
        let token = std::env::var(variable).ok()?;
        let token = token.trim();
        if token.len() < MIN_TOKEN_LEN {
            return None;
        }
        Some(Self {
            token: token.to_string(),
        })
    }

    pub fn verify(&self, candidate: &str) -> bool {
        constant_time_eq(self.token.as_bytes(), candidate.as_bytes())
    }
}

impl fmt::Debug for AdminAuthConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AdminAuthConfig(<redacted>)")
    }
}

pub async fn require_admin(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Result<Response, ApiError> {
    let candidate = request
        .headers()
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(bearer_token)
        .ok_or_else(ApiError::unauthorized)?;

    // Bootstrap token (backward compatible) is always role admin; otherwise
    // resolve against the DB admin-token store.
    let identity = match state.admin_auth.as_deref() {
        Some(config) if config.verify(candidate) => Some(crate::admin::AdminIdentity {
            actor: "bootstrap".to_string(),
            role: crate::admin::Role::Admin,
        }),
        _ => crate::admin::verify_db_token(&state.db, candidate).await?,
    };

    let identity = identity.ok_or_else(ApiError::unauthorized)?;

    if let Some(limiter) = &state.rate_limiter
        && !limiter.check(&identity.actor).await
    {
        return Err(ApiError::forbidden("rate limit exceeded"));
    }

    request.extensions_mut().insert(identity);
    Ok(next.run(request).await)
}

pub async fn require_internal(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Result<Response, ApiError> {
    require_token(
        state.radius_auth.as_deref(),
        "RADIUS internal authentication is not configured",
        request,
        next,
    )
    .await
}

async fn require_token(
    config: Option<&AdminAuthConfig>,
    unconfigured_message: &str,
    request: Request,
    next: Next,
) -> Result<Response, ApiError> {
    let config = config.ok_or_else(|| ApiError::service_unavailable(unconfigured_message))?;

    let candidate = request
        .headers()
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(bearer_token)
        .ok_or_else(ApiError::unauthorized)?;

    if !config.verify(candidate) {
        return Err(ApiError::unauthorized());
    }

    Ok(next.run(request).await)
}

fn bearer_token(header: &str) -> Option<&str> {
    let mut parts = header.splitn(2, ' ');
    let scheme = parts.next()?;
    let token = parts.next()?.trim();
    if scheme.eq_ignore_ascii_case("bearer") && !token.is_empty() {
        Some(token)
    } else {
        None
    }
}

fn constant_time_eq(expected: &[u8], candidate: &[u8]) -> bool {
    let length = expected.len().max(candidate.len());

    let mut expected_padded = vec![0u8; length];
    expected_padded[..expected.len()].copy_from_slice(expected);
    let mut candidate_padded = vec![0u8; length];
    candidate_padded[..candidate.len()].copy_from_slice(candidate);

    let expected_len = expected.len() as u64;
    let candidate_len = candidate.len() as u64;

    bool::from(
        expected_padded
            .as_slice()
            .ct_eq(candidate_padded.as_slice())
            & expected_len.ct_eq(&candidate_len),
    )
}

#[cfg(test)]
mod tests {
    use super::constant_time_eq;

    #[test]
    fn matches_only_identical_tokens() {
        assert!(constant_time_eq(b"abcdef", b"abcdef"));
        assert!(!constant_time_eq(b"abcdef", b"abcdeg"));
        assert!(!constant_time_eq(b"abcdef", b"abcde"));
        assert!(!constant_time_eq(b"abcde", b"abcdef"));
        assert!(!constant_time_eq(b"", b"a"));
        assert!(constant_time_eq(b"", b""));
    }
}
