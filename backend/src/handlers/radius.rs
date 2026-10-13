use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use serde_json::Value;

use rwisp_server::radius::{self, RadiusRequest};

use crate::api_error::ApiError;
use crate::state::AppState;

/// `rlm_rest` authorize hook (policy only): returns reply/control attributes
/// including `control:Auth-Type := rest`. It performs no password check and no
/// activation, so authentication cannot be bypassed and nothing is activated
/// before the credential is proven.
pub async fn authorize(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let request = RadiusRequest::from_json(&body)?;
    let outcome = radius::authorize(&state.db, &request).await?;
    let status = StatusCode::from_u16(outcome.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    Ok((status, Json(outcome.body)))
}

/// `rlm_rest` authenticate hook (`Auth-Type rest`): verifies the credential and
/// only then performs atomic first-login activation.
pub async fn authenticate(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let request = RadiusRequest::from_json(&body)?;
    let outcome = radius::authenticate(&state.db, &request).await?;
    let status = StatusCode::from_u16(outcome.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    Ok((status, Json(outcome.body)))
}

/// `rlm_rest` accounting hook (Start/Interim/Stop), idempotent.
pub async fn accounting(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> Result<StatusCode, ApiError> {
    let request = RadiusRequest::from_json(&body)?;
    radius::accounting::process(&state.db, &request).await?;
    Ok(StatusCode::NO_CONTENT)
}
