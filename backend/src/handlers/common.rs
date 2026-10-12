use std::future::Future;

use serde_json::Value;

use rwisp_server::crypto::EncryptionKey;
use rwisp_server::routers::{self, RouterRecord};

use crate::api_error::ApiError;
use crate::state::AppState;

pub(crate) fn parse_id(raw: &str) -> Result<u64, ApiError> {
    raw.trim()
        .parse::<u64>()
        .map_err(|_| ApiError::bad_request(format!("invalid router id: {raw}")))
}

pub(crate) async fn load_router(state: &AppState, id: u64) -> Result<RouterRecord, ApiError> {
    let record = routers::get_router(&state.db, id)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("router {id} was not found")))?;

    if !record.enabled {
        return Err(ApiError::service_unavailable("router is disabled"));
    }

    Ok(record)
}

pub(crate) fn encryption_key(state: &AppState) -> Result<&EncryptionKey, ApiError> {
    state.encryption_key.as_deref().ok_or_else(|| {
        ApiError::service_unavailable("router credential encryption is not configured")
    })
}

pub(crate) async fn with_router_timeout<F>(state: &AppState, future: F) -> Result<Value, ApiError>
where
    F: Future<Output = Result<Value, ApiError>>,
{
    match tokio::time::timeout(state.router_timeout, future).await {
        Ok(result) => result,
        Err(_) => Err(ApiError::service_unavailable(
            "router did not respond in time",
        )),
    }
}
