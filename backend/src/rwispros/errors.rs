use std::fmt;
use std::io;
use std::time::Duration;

use mikrotik_rs::DeviceError;

pub type Result<T> = std::result::Result<T, RouterOsError>;

#[derive(Debug)]
pub enum RouterOsError {
    Unreachable { host: String, source: io::Error },
    Timeout { host: String, timeout: Duration },
    Authentication { message: String },
    Command { message: String },
    Tls { message: String },
    Protocol { message: String },
    ConnectionClosed,
    Config { message: String },
    Io { source: io::Error },
}

impl fmt::Display for RouterOsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unreachable { host, source } => {
                write!(f, "router {host} is unreachable: {source}")
            }
            Self::Timeout { host, timeout } => {
                write!(f, "router {host} did not respond within {timeout:?}")
            }
            Self::Authentication { message } => {
                write!(f, "router authentication failed: {message}")
            }
            Self::Command { message } => write!(f, "router command failed: {message}"),
            Self::Tls { message } => write!(f, "router TLS error: {message}"),
            Self::Protocol { message } => write!(f, "router protocol error: {message}"),
            Self::ConnectionClosed => write!(f, "router connection closed unexpectedly"),
            Self::Config { message } => write!(f, "router configuration error: {message}"),
            Self::Io { source } => write!(f, "router I/O error: {source}"),
        }
    }
}

impl std::error::Error for RouterOsError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Unreachable { source, .. } => Some(source),
            Self::Io { source } => Some(source),
            _ => None,
        }
    }
}

impl From<DeviceError> for RouterOsError {
    fn from(error: DeviceError) -> Self {
        match error {
            DeviceError::Connection(source) => Self::Io { source },
            DeviceError::Authentication { response } => Self::Authentication {
                message: response.message,
            },
            DeviceError::Login(error) => Self::Authentication {
                message: error.to_string(),
            },
            DeviceError::Protocol(error) => Self::Protocol {
                message: error.to_string(),
            },
            DeviceError::ConnectionState(error) => Self::Protocol {
                message: error.to_string(),
            },
            DeviceError::Actor(error) => Self::Protocol {
                message: error.to_string(),
            },
            DeviceError::ConnectionClosed => Self::ConnectionClosed,
            DeviceError::Tls(error) => Self::Tls {
                message: error.to_string(),
            },
        }
    }
}
