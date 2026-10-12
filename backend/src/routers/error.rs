use std::fmt;

use crate::crypto::CryptoError;
use crate::rwispros::RouterOsError;

#[derive(Debug)]
pub enum RouterError {
    Database(sqlx::Error),
    Encryption(CryptoError),
    RouterOs(RouterOsError),
    NotFound { id: u64 },
    Disabled { id: u64 },
}

impl fmt::Display for RouterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Database(error) => write!(f, "router database error: {error}"),
            Self::Encryption(error) => write!(f, "router credential error: {error}"),
            Self::RouterOs(error) => write!(f, "router connection error: {error}"),
            Self::NotFound { id } => write!(f, "router {id} was not found"),
            Self::Disabled { id } => write!(f, "router {id} is disabled"),
        }
    }
}

impl std::error::Error for RouterError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Database(error) => Some(error),
            Self::Encryption(error) => Some(error),
            Self::RouterOs(error) => Some(error),
            _ => None,
        }
    }
}

impl From<sqlx::Error> for RouterError {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(error)
    }
}

impl From<CryptoError> for RouterError {
    fn from(error: CryptoError) -> Self {
        Self::Encryption(error)
    }
}

impl From<RouterOsError> for RouterError {
    fn from(error: RouterOsError) -> Self {
        Self::RouterOs(error)
    }
}
