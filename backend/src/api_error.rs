use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::json;

use rwisp_server::routers::RouterError;
use rwisp_server::rwispros::RouterOsError;

#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    pub fn unauthorized() -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            message: "unauthorized".to_string(),
        }
    }

    pub fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
        }
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: message.into(),
        }
    }

    pub fn service_unavailable(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::SERVICE_UNAVAILABLE,
            message: message.into(),
        }
    }

    pub fn internal() -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: "internal server error".to_string(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = Json(json!({ "error": self.message }));
        (self.status, body).into_response()
    }
}

impl From<RouterError> for ApiError {
    fn from(error: RouterError) -> Self {
        match error {
            RouterError::NotFound { id } => Self::not_found(format!("router {id} was not found")),
            RouterError::Disabled { .. } => Self::service_unavailable("router is disabled"),
            RouterError::RouterOs(error) => Self::from_router_os(error),
            RouterError::Database(error) => {
                eprintln!("router API database error: {error}");
                Self::internal()
            }
            RouterError::Encryption(error) => {
                eprintln!("router API credential error: {error}");
                Self::internal()
            }
        }
    }
}

impl From<RouterOsError> for ApiError {
    fn from(error: RouterOsError) -> Self {
        Self::from_router_os(error)
    }
}

impl ApiError {
    fn from_router_os(error: RouterOsError) -> Self {
        eprintln!("router API connection error: {error}");
        match error {
            RouterOsError::Authentication { .. } => {
                Self::service_unavailable("router authentication failed")
            }
            RouterOsError::Timeout { .. } => {
                Self::service_unavailable("router did not respond in time")
            }
            RouterOsError::Unreachable { .. } | RouterOsError::Io { .. } => {
                Self::service_unavailable("router is unreachable")
            }
            RouterOsError::Command { .. }
            | RouterOsError::Protocol { .. }
            | RouterOsError::ConnectionClosed
            | RouterOsError::Tls { .. }
            | RouterOsError::Config { .. } => Self::service_unavailable("router is unavailable"),
        }
    }
}
