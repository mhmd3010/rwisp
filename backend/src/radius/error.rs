use std::fmt;

#[derive(Debug)]
pub enum RadiusError {
    Database(sqlx::Error),
    BadRequest { message: String },
    Internal { message: String },
}

impl fmt::Display for RadiusError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Database(error) => write!(f, "radius database error: {error}"),
            Self::BadRequest { message } => write!(f, "invalid radius request: {message}"),
            Self::Internal { message } => write!(f, "radius internal error: {message}"),
        }
    }
}

impl std::error::Error for RadiusError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Database(error) => Some(error),
            _ => None,
        }
    }
}

impl From<sqlx::Error> for RadiusError {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(error)
    }
}

impl From<crate::accounts::AccountError> for RadiusError {
    fn from(error: crate::accounts::AccountError) -> Self {
        Self::Internal {
            message: error.to_string(),
        }
    }
}

impl From<crate::vouchers::VoucherError> for RadiusError {
    fn from(error: crate::vouchers::VoucherError) -> Self {
        Self::Internal {
            message: error.to_string(),
        }
    }
}

impl From<crate::packages::PackageError> for RadiusError {
    fn from(error: crate::packages::PackageError) -> Self {
        Self::Internal {
            message: error.to_string(),
        }
    }
}
