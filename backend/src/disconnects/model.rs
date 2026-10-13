use std::collections::HashMap;

use crate::disconnects::error::DisconnectError;

pub const DEFAULT_PAGE_SIZE: u32 = 50;
pub const MAX_PAGE_SIZE: u32 = 500;
pub const DEFAULT_MAX_ATTEMPTS: u32 = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisconnectStatus {
    Pending,
    Success,
    Failed,
    Unsupported,
}

impl DisconnectStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Success => "success",
            Self::Failed => "failed",
            Self::Unsupported => "unsupported",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "pending" => Some(Self::Pending),
            "success" => Some(Self::Success),
            "failed" => Some(Self::Failed),
            "unsupported" => Some(Self::Unsupported),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisconnectMethod {
    Coa,
    RouterOs,
    None,
}

impl DisconnectMethod {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Coa => "coa",
            Self::RouterOs => "routeros",
            Self::None => "none",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "coa" => Some(Self::Coa),
            "routeros" => Some(Self::RouterOs),
            "none" => Some(Self::None),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisconnectRecord {
    pub id: u64,
    pub username: String,
    pub account_id: Option<u64>,
    pub voucher_id: Option<u64>,
    pub session_row_id: Option<u64>,
    pub router_id: Option<u64>,
    pub nas_ip: Option<String>,
    pub acct_session_id: Option<String>,
    pub reason: String,
    pub method: DisconnectMethod,
    pub status: DisconnectStatus,
    pub attempts: u32,
    pub max_attempts: u32,
    pub last_error: Option<String>,
    pub next_attempt_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub completed_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewDisconnect {
    pub username: String,
    pub account_id: Option<u64>,
    pub voucher_id: Option<u64>,
    pub session_row_id: Option<u64>,
    pub router_id: Option<u64>,
    pub nas_ip: Option<String>,
    pub acct_session_id: Option<String>,
    pub reason: String,
    pub method: DisconnectMethod,
    pub max_attempts: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisconnectFilter {
    pub status: Option<DisconnectStatus>,
    pub username: Option<String>,
    pub router_id: Option<u64>,
    pub account_id: Option<u64>,
    pub voucher_id: Option<u64>,
    pub limit: u32,
    pub offset: u32,
}

impl Default for DisconnectFilter {
    fn default() -> Self {
        Self {
            status: None,
            username: None,
            router_id: None,
            account_id: None,
            voucher_id: None,
            limit: DEFAULT_PAGE_SIZE,
            offset: 0,
        }
    }
}

impl DisconnectFilter {
    pub fn from_query(params: &HashMap<String, String>) -> Result<Self, DisconnectError> {
        let status =
            match params.get("status") {
                Some(raw) => Some(DisconnectStatus::parse(raw).ok_or_else(|| {
                    DisconnectError::Validation {
                        field: "status",
                        message: "must be one of pending, success, failed, unsupported".to_string(),
                    }
                })?),
                None => None,
            };
        let limit = match params.get("limit") {
            Some(raw) => raw
                .trim()
                .parse::<u32>()
                .map_err(|_| DisconnectError::Validation {
                    field: "limit",
                    message: "must be a positive integer".to_string(),
                })?
                .clamp(1, MAX_PAGE_SIZE),
            None => DEFAULT_PAGE_SIZE,
        };
        let offset = match params.get("offset") {
            Some(raw) => raw
                .trim()
                .parse::<u32>()
                .map_err(|_| DisconnectError::Validation {
                    field: "offset",
                    message: "must be a non-negative integer".to_string(),
                })?,
            None => 0,
        };

        Ok(Self {
            status,
            username: params
                .get("username")
                .map(|v| v.trim())
                .filter(|v| !v.is_empty())
                .map(str::to_string),
            router_id: optional_u64(params, "router_id")?,
            account_id: optional_u64(params, "account_id")?,
            voucher_id: optional_u64(params, "voucher_id")?,
            limit,
            offset,
        })
    }
}

fn optional_u64(
    params: &HashMap<String, String>,
    field: &'static str,
) -> Result<Option<u64>, DisconnectError> {
    match params.get(field) {
        None => Ok(None),
        Some(raw) => raw
            .trim()
            .parse::<u64>()
            .map(Some)
            .map_err(|_| DisconnectError::Validation {
                field,
                message: "must be a positive integer".to_string(),
            }),
    }
}

/// Outcome of attempting to deliver a disconnect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DisconnectOutcome {
    /// Confirmed by Disconnect-ACK (CoA) or a successful RouterOS removal.
    Success,
    /// Not delivered; retry with backoff.
    Retry { error: String },
    /// No reachable method (e.g. CGNAT with no CoA endpoint and no management
    /// API). The operation is preserved rather than falsely reported.
    Unsupported { reason: String },
}

#[derive(Debug, Clone, Default)]
pub struct DisconnectStats {
    pub pending: u64,
    pub success: u64,
    pub failed: u64,
    pub unsupported: u64,
}
