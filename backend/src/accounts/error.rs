use std::fmt;

#[derive(Debug)]
pub enum AccountError {
    Database(sqlx::Error),
    NotFound {
        id: u64,
    },
    Conflict {
        message: String,
    },
    Validation {
        field: &'static str,
        message: String,
    },
    Password {
        message: String,
    },
}

impl fmt::Display for AccountError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Database(error) => write!(f, "account database error: {error}"),
            Self::NotFound { id } => write!(f, "account {id} was not found"),
            Self::Conflict { message } => write!(f, "account conflict: {message}"),
            Self::Validation { field, message } => {
                write!(f, "invalid account field `{field}`: {message}")
            }
            Self::Password { message } => write!(f, "account password error: {message}"),
        }
    }
}

impl std::error::Error for AccountError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Database(error) => Some(error),
            _ => None,
        }
    }
}

impl From<sqlx::Error> for AccountError {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(error)
    }
}
