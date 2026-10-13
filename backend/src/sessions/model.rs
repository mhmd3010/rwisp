use std::collections::HashMap;

use crate::sessions::error::SessionError;

pub const DEFAULT_PAGE_SIZE: u32 = 50;
pub const MAX_PAGE_SIZE: u32 = 500;
pub const DEFAULT_STALE_SECS: i64 = 900;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionStatus {
    Start,
    Interim,
    Stop,
}

impl SessionStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Interim => "interim",
            Self::Stop => "stop",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "start" => Some(Self::Start),
            "interim" => Some(Self::Interim),
            "stop" => Some(Self::Stop),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRecord {
    pub id: u64,
    pub username: String,
    pub acct_session_id: String,
    pub acct_unique_id: Option<String>,
    pub nas_ip: String,
    pub nas_identifier: Option<String>,
    pub calling_station_id: Option<String>,
    pub framed_ip: Option<String>,
    pub account_id: Option<u64>,
    pub voucher_id: Option<u64>,
    pub status: SessionStatus,
    pub session_time: u64,
    pub input_octets: u64,
    pub output_octets: u64,
    pub terminate_cause: Option<String>,
    pub started_at: Option<String>,
    pub updated_at: Option<String>,
    pub stopped_at: Option<String>,
    pub created_at: String,
    /// Open (no Stop received) and not considered stale.
    pub online: bool,
    /// Open but no accounting update within the stale window.
    pub stale: bool,
}

impl SessionRecord {
    pub fn total_octets(&self) -> u64 {
        self.input_octets.saturating_add(self.output_octets)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionFilter {
    pub username: Option<String>,
    pub account_id: Option<u64>,
    pub voucher_id: Option<u64>,
    pub nas_ip: Option<String>,
    pub framed_ip: Option<String>,
    pub calling_station_id: Option<String>,
    pub status: Option<SessionStatus>,
    pub online_only: bool,
    pub limit: u32,
    pub offset: u32,
}

impl Default for SessionFilter {
    fn default() -> Self {
        Self {
            username: None,
            account_id: None,
            voucher_id: None,
            nas_ip: None,
            framed_ip: None,
            calling_station_id: None,
            status: None,
            online_only: false,
            limit: DEFAULT_PAGE_SIZE,
            offset: 0,
        }
    }
}

impl SessionFilter {
    pub fn from_query(params: &HashMap<String, String>) -> Result<Self, SessionError> {
        let status = match params.get("status") {
            Some(raw) => {
                Some(
                    SessionStatus::parse(raw).ok_or_else(|| SessionError::Validation {
                        field: "status",
                        message: "must be one of start, interim, stop".to_string(),
                    })?,
                )
            }
            None => None,
        };
        let online_only = matches!(
            params.get("online").map(String::as_str),
            Some("1" | "true" | "yes")
        );

        let limit = match params.get("limit") {
            Some(raw) => raw
                .trim()
                .parse::<u32>()
                .map_err(|_| SessionError::Validation {
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
                .map_err(|_| SessionError::Validation {
                    field: "offset",
                    message: "must be a non-negative integer".to_string(),
                })?,
            None => 0,
        };

        Ok(Self {
            username: non_empty(params.get("username")),
            account_id: optional_u64(params, "account_id")?,
            voucher_id: optional_u64(params, "voucher_id")?,
            nas_ip: non_empty(params.get("nas_ip")),
            framed_ip: non_empty(params.get("ip")),
            calling_station_id: non_empty(params.get("mac")),
            status,
            online_only,
            limit,
            offset,
        })
    }
}

fn non_empty(value: Option<&String>) -> Option<String> {
    value
        .map(|v| v.trim())
        .filter(|v| !v.is_empty())
        .map(str::to_string)
}

fn optional_u64(
    params: &HashMap<String, String>,
    field: &'static str,
) -> Result<Option<u64>, SessionError> {
    match params.get(field) {
        None => Ok(None),
        Some(raw) => raw
            .trim()
            .parse::<u64>()
            .map(Some)
            .map_err(|_| SessionError::Validation {
                field,
                message: "must be a positive integer".to_string(),
            }),
    }
}

/// Reconciliation result comparing RWISP sessions with RouterOS active sessions.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ReconcileReport {
    pub rwisp_online: usize,
    pub routeros_active: usize,
    /// Online in RWISP but absent on RouterOS (likely missed Stop).
    pub missing_stop: Vec<String>,
    /// Active on RouterOS but unknown to RWISP.
    pub unknown_on_router: Vec<String>,
}
