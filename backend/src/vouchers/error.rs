use std::fmt;

#[derive(Debug)]
pub enum VoucherError {
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

impl fmt::Display for VoucherError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Database(error) => write!(f, "voucher database error: {error}"),
            Self::NotFound { id } => write!(f, "voucher {id} was not found"),
            Self::Conflict { message } => write!(f, "voucher conflict: {message}"),
            Self::Validation { field, message } => {
                write!(f, "invalid voucher field `{field}`: {message}")
            }
        }
    }
}

impl std::error::Error for VoucherError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Database(error) => Some(error),
            _ => None,
        }
    }
}

impl From<sqlx::Error> for VoucherError {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(error)
    }
}
