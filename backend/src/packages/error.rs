use std::fmt;

#[derive(Debug)]
pub enum PackageError {
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
}

impl fmt::Display for PackageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Database(error) => write!(f, "package database error: {error}"),
            Self::NotFound { id } => write!(f, "package {id} was not found"),
            Self::Conflict { message } => write!(f, "package conflict: {message}"),
            Self::Validation { field, message } => {
                write!(f, "invalid package field `{field}`: {message}")
            }
        }
    }
}

impl std::error::Error for PackageError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Database(error) => Some(error),
            _ => None,
        }
    }
}

impl From<sqlx::Error> for PackageError {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(error)
    }
}
