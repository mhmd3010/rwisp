use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::json;

use rwisp_server::accounts::AccountError;
use rwisp_server::packages::PackageError;
use rwisp_server::radius::RadiusError;
use rwisp_server::routers::RouterError;
use rwisp_server::rwispros::RouterOsError;
use rwisp_server::vouchers::VoucherError;

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

    pub fn conflict(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::CONFLICT,
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

impl From<PackageError> for ApiError {
    fn from(error: PackageError) -> Self {
        match error {
            PackageError::Validation { field, message } => {
                Self::bad_request(format!("invalid `{field}`: {message}"))
            }
            PackageError::NotFound { id } => Self::not_found(format!("package {id} was not found")),
            PackageError::Conflict { message } => Self::conflict(message),
            PackageError::Database(error) => {
                eprintln!("package API database error: {error}");
                Self::internal()
            }
        }
    }
}

impl From<VoucherError> for ApiError {
    fn from(error: VoucherError) -> Self {
        match error {
            VoucherError::Validation { field, message } => {
                Self::bad_request(format!("invalid `{field}`: {message}"))
            }
            VoucherError::NotFound { id } => Self::not_found(format!("voucher {id} was not found")),
            VoucherError::Conflict { message } => Self::conflict(message),
            VoucherError::Database(error) => {
                eprintln!("voucher API database error: {error}");
                Self::internal()
            }
        }
    }
}

impl From<AccountError> for ApiError {
    fn from(error: AccountError) -> Self {
        match error {
            AccountError::Validation { field, message } => {
                Self::bad_request(format!("invalid `{field}`: {message}"))
            }
            AccountError::NotFound { id } => Self::not_found(format!("account {id} was not found")),
            AccountError::Conflict { message } => Self::conflict(message),
            AccountError::Password { message } => {
                eprintln!("account password error: {message}");
                Self::internal()
            }
            AccountError::Database(error) => {
                eprintln!("account API database error: {error}");
                Self::internal()
            }
        }
    }
}

impl From<RadiusError> for ApiError {
    fn from(error: RadiusError) -> Self {
        match error {
            RadiusError::BadRequest { message } => Self::bad_request(message),
            RadiusError::Database(error) => {
                eprintln!("radius database error: {error}");
                Self::internal()
            }
            RadiusError::Internal { message } => {
                eprintln!("radius internal error: {message}");
                Self::internal()
            }
        }
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
