use std::collections::HashMap;

use serde_json::{Map, Value};

use crate::vouchers::error::VoucherError;

pub const MAX_BATCH_QUANTITY: u32 = 1000;
pub const DEFAULT_PAGE_SIZE: u32 = 50;
pub const MAX_PAGE_SIZE: u32 = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoucherStatus {
    Unused,
    Active,
    Expired,
    Revoked,
}

impl VoucherStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Unused => "unused",
            Self::Active => "active",
            Self::Expired => "expired",
            Self::Revoked => "revoked",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "unused" => Some(Self::Unused),
            "active" => Some(Self::Active),
            "expired" => Some(Self::Expired),
            "revoked" => Some(Self::Revoked),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Voucher {
    pub id: u64,
    pub batch_id: Option<u64>,
    pub code: String,
    pub package_id: u64,
    pub router_id: Option<u64>,
    pub status: VoucherStatus,
    pub created_at: String,
    pub activated_at: Option<String>,
    pub expires_at: Option<String>,
    pub last_used_at: Option<String>,
    pub usage_secs: u64,
    pub bytes_in: u64,
    pub bytes_out: u64,
    pub revoked_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatchRequest {
    pub package_id: u64,
    pub quantity: u32,
    pub router_id: Option<u64>,
    pub note: Option<String>,
}

impl BatchRequest {
    pub fn from_json(value: &Value) -> Result<Self, VoucherError> {
        let object = value.as_object().ok_or_else(|| VoucherError::Validation {
            field: "body",
            message: "expected a JSON object".to_string(),
        })?;

        let package_id =
            required_u64(object, "package_id")?.ok_or_else(|| VoucherError::Validation {
                field: "package_id",
                message: "is required".to_string(),
            })?;
        let quantity =
            required_u64(object, "quantity")?.ok_or_else(|| VoucherError::Validation {
                field: "quantity",
                message: "is required".to_string(),
            })?;
        let quantity = u32::try_from(quantity).map_err(|_| VoucherError::Validation {
            field: "quantity",
            message: "is too large".to_string(),
        })?;
        if quantity == 0 || quantity > MAX_BATCH_QUANTITY {
            return Err(VoucherError::Validation {
                field: "quantity",
                message: format!("must be between 1 and {MAX_BATCH_QUANTITY}"),
            });
        }

        let router_id = required_u64(object, "router_id")?;
        let note = optional_string(object, "note", 255)?;

        Ok(Self {
            package_id,
            quantity,
            router_id,
            note,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedBatch {
    pub batch_id: u64,
    pub quantity: u32,
    pub codes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActivationOutcome {
    Activated(Box<Voucher>),
    NotFound,
    NotUnused { status: VoucherStatus },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RevokeOutcome {
    Revoked(Box<Voucher>),
    NotFound,
    NotRevocable { status: VoucherStatus },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoucherFilter {
    pub package_id: Option<u64>,
    pub router_id: Option<u64>,
    pub batch_id: Option<u64>,
    pub status: Option<VoucherStatus>,
    pub limit: u32,
    pub offset: u32,
}

impl Default for VoucherFilter {
    fn default() -> Self {
        Self {
            package_id: None,
            router_id: None,
            batch_id: None,
            status: None,
            limit: DEFAULT_PAGE_SIZE,
            offset: 0,
        }
    }
}

impl VoucherFilter {
    pub fn from_query(params: &HashMap<String, String>) -> Result<Self, VoucherError> {
        let status = match params.get("status") {
            Some(raw) => {
                Some(
                    VoucherStatus::parse(raw).ok_or_else(|| VoucherError::Validation {
                        field: "status",
                        message: "must be one of unused, active, expired, revoked".to_string(),
                    })?,
                )
            }
            None => None,
        };

        let limit = match params.get("limit") {
            Some(raw) => {
                let value = raw
                    .trim()
                    .parse::<u32>()
                    .map_err(|_| VoucherError::Validation {
                        field: "limit",
                        message: "must be a positive integer".to_string(),
                    })?;
                value.clamp(1, MAX_PAGE_SIZE)
            }
            None => DEFAULT_PAGE_SIZE,
        };

        let offset = match params.get("offset") {
            Some(raw) => raw
                .trim()
                .parse::<u32>()
                .map_err(|_| VoucherError::Validation {
                    field: "offset",
                    message: "must be a non-negative integer".to_string(),
                })?,
            None => 0,
        };

        Ok(Self {
            package_id: optional_query_u64(params, "package_id")?,
            router_id: optional_query_u64(params, "router_id")?,
            batch_id: optional_query_u64(params, "batch_id")?,
            status,
            limit,
            offset,
        })
    }
}

fn optional_query_u64(
    params: &HashMap<String, String>,
    field: &'static str,
) -> Result<Option<u64>, VoucherError> {
    match params.get(field) {
        None => Ok(None),
        Some(raw) => raw
            .trim()
            .parse::<u64>()
            .map(Some)
            .map_err(|_| VoucherError::Validation {
                field,
                message: "must be a positive integer".to_string(),
            }),
    }
}

fn required_u64(
    object: &Map<String, Value>,
    field: &'static str,
) -> Result<Option<u64>, VoucherError> {
    match object.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value_to_u64(value, field).map(Some),
    }
}

fn value_to_u64(value: &Value, field: &'static str) -> Result<u64, VoucherError> {
    if let Some(number) = value.as_u64() {
        return Ok(number);
    }
    if let Some(text) = value.as_str()
        && let Ok(number) = text.trim().parse::<u64>()
    {
        return Ok(number);
    }
    Err(VoucherError::Validation {
        field,
        message: "must be a non-negative integer".to_string(),
    })
}

fn optional_string(
    object: &Map<String, Value>,
    field: &'static str,
    max_len: usize,
) -> Result<Option<String>, VoucherError> {
    match object.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => {
            let text = value.as_str().ok_or_else(|| VoucherError::Validation {
                field,
                message: "must be a string".to_string(),
            })?;
            let text = text.trim();
            if text.is_empty() {
                return Ok(None);
            }
            if text.chars().count() > max_len {
                return Err(VoucherError::Validation {
                    field,
                    message: format!("must be at most {max_len} characters"),
                });
            }
            Ok(Some(text.to_string()))
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use serde_json::json;

    use super::{BatchRequest, DEFAULT_PAGE_SIZE, MAX_PAGE_SIZE, VoucherFilter, VoucherStatus};
    use crate::vouchers::error::VoucherError;

    #[test]
    fn parses_valid_batch_request() {
        let request = BatchRequest::from_json(&json!({
            "package_id": 3,
            "quantity": 25,
            "router_id": "7",
            "note": "  front desk  "
        }))
        .expect("valid");

        assert_eq!(request.package_id, 3);
        assert_eq!(request.quantity, 25);
        assert_eq!(request.router_id, Some(7));
        assert_eq!(request.note.as_deref(), Some("front desk"));
    }

    #[test]
    fn rejects_invalid_batch_requests() {
        assert!(matches!(
            BatchRequest::from_json(&json!({ "quantity": 5 })),
            Err(VoucherError::Validation {
                field: "package_id",
                ..
            })
        ));
        assert!(matches!(
            BatchRequest::from_json(&json!({ "package_id": 1, "quantity": 0 })),
            Err(VoucherError::Validation {
                field: "quantity",
                ..
            })
        ));
        assert!(matches!(
            BatchRequest::from_json(&json!({ "package_id": 1, "quantity": 5000 })),
            Err(VoucherError::Validation {
                field: "quantity",
                ..
            })
        ));
    }

    #[test]
    fn parses_filter_defaults_and_overrides() {
        let defaults = VoucherFilter::from_query(&HashMap::new()).expect("defaults");
        assert_eq!(defaults.limit, DEFAULT_PAGE_SIZE);
        assert_eq!(defaults.offset, 0);
        assert_eq!(defaults.status, None);

        let mut params = HashMap::new();
        params.insert("status".to_string(), "ACTIVE".to_string());
        params.insert("package_id".to_string(), "4".to_string());
        params.insert("limit".to_string(), "9999".to_string());
        let filter = VoucherFilter::from_query(&params).expect("filter");
        assert_eq!(filter.status, Some(VoucherStatus::Active));
        assert_eq!(filter.package_id, Some(4));
        assert_eq!(filter.limit, MAX_PAGE_SIZE);

        let mut bad = HashMap::new();
        bad.insert("status".to_string(), "bogus".to_string());
        assert!(VoucherFilter::from_query(&bad).is_err());
    }
}
