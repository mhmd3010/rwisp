use std::fmt;

#[derive(Debug)]
pub enum DisconnectError {
    Database(sqlx::Error),
    NotFound {
        id: u64,
    },
    Validation {
        field: &'static str,
        message: String,
    },
}

impl fmt::Display for DisconnectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Database(error) => write!(f, "disconnect database error: {error}"),
            Self::NotFound { id } => write!(f, "disconnect {id} was not found"),
            Self::Validation { field, message } => {
                write!(f, "invalid disconnect field `{field}`: {message}")
            }
        }
    }
}

impl std::error::Error for DisconnectError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Database(error) => Some(error),
            _ => None,
        }
    }
}

impl From<sqlx::Error> for DisconnectError {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(error)
    }
}
