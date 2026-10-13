use std::fmt;

#[derive(Debug)]
pub enum SessionError {
    Database(sqlx::Error),
    NotFound {
        id: u64,
    },
    Validation {
        field: &'static str,
        message: String,
    },
}

impl fmt::Display for SessionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Database(error) => write!(f, "session database error: {error}"),
            Self::NotFound { id } => write!(f, "session {id} was not found"),
            Self::Validation { field, message } => {
                write!(f, "invalid session field `{field}`: {message}")
            }
        }
    }
}

impl std::error::Error for SessionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Database(error) => Some(error),
            _ => None,
        }
    }
}

impl From<sqlx::Error> for SessionError {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(error)
    }
}
