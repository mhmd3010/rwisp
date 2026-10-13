use serde_json::{Map, Value};

use crate::packages::error::PackageError;

const MAX_NAME_LEN: usize = 100;
const MAX_DESCRIPTION_LEN: usize = 255;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Package {
    pub id: u64,
    pub name: String,
    pub description: Option<String>,
    pub price_cents: u64,
    pub enabled: bool,
    pub validity_secs: Option<u64>,
    pub usage_limit_secs: Option<u64>,
    pub data_limit_bytes: Option<u64>,
    pub upload_rate_bps: Option<u64>,
    pub download_rate_bps: Option<u64>,
    pub device_limit: Option<u32>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageInput {
    pub name: String,
    pub description: Option<String>,
    pub price_cents: u64,
    pub enabled: bool,
    pub validity_secs: Option<u32>,
    pub usage_limit_secs: Option<u32>,
    pub data_limit_bytes: Option<u64>,
    pub upload_rate_bps: Option<u64>,
    pub download_rate_bps: Option<u64>,
    pub device_limit: Option<u32>,
}

impl PackageInput {
    pub fn from_json(value: &Value) -> Result<Self, PackageError> {
        let object = value.as_object().ok_or_else(|| PackageError::Validation {
            field: "body",
            message: "expected a JSON object".to_string(),
        })?;

        Ok(Self {
            name: required_name(object)?,
            description: optional_string(object, "description", MAX_DESCRIPTION_LEN)?,
            price_cents: optional_unsigned(object, "price_cents")?.unwrap_or(0),
            enabled: optional_bool(object, "enabled")?.unwrap_or(true),
            validity_secs: optional_u32_limit(object, "validity_secs")?,
            usage_limit_secs: optional_u32_limit(object, "usage_limit_secs")?,
            data_limit_bytes: optional_positive_u64(object, "data_limit_bytes")?,
            upload_rate_bps: optional_positive_u64(object, "upload_rate_bps")?,
            download_rate_bps: optional_positive_u64(object, "download_rate_bps")?,
            device_limit: optional_u32_limit(object, "device_limit")?,
        })
    }
}

fn required_name(object: &Map<String, Value>) -> Result<String, PackageError> {
    let value = object.get("name").ok_or_else(|| PackageError::Validation {
        field: "name",
        message: "is required".to_string(),
    })?;
    let name = value
        .as_str()
        .ok_or_else(|| PackageError::Validation {
            field: "name",
            message: "must be a string".to_string(),
        })?
        .trim()
        .to_string();

    if name.is_empty() {
        return Err(PackageError::Validation {
            field: "name",
            message: "must not be empty".to_string(),
        });
    }
    if name.chars().count() > MAX_NAME_LEN {
        return Err(PackageError::Validation {
            field: "name",
            message: format!("must be at most {MAX_NAME_LEN} characters"),
        });
    }

    Ok(name)
}

fn optional_string(
    object: &Map<String, Value>,
    field: &'static str,
    max_len: usize,
) -> Result<Option<String>, PackageError> {
    match object.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => {
            let text = value.as_str().ok_or_else(|| PackageError::Validation {
                field,
                message: "must be a string".to_string(),
            })?;
            let text = text.trim();
            if text.is_empty() {
                return Ok(None);
            }
            if text.chars().count() > max_len {
                return Err(PackageError::Validation {
                    field,
                    message: format!("must be at most {max_len} characters"),
                });
            }
            Ok(Some(text.to_string()))
        }
    }
}

fn optional_bool(
    object: &Map<String, Value>,
    field: &'static str,
) -> Result<Option<bool>, PackageError> {
    match object.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_bool()
            .map(Some)
            .ok_or_else(|| PackageError::Validation {
                field,
                message: "must be a boolean".to_string(),
            }),
    }
}

fn optional_unsigned(
    object: &Map<String, Value>,
    field: &'static str,
) -> Result<Option<u64>, PackageError> {
    match object.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value_to_u64(value, field).map(Some),
    }
}

fn optional_positive_u64(
    object: &Map<String, Value>,
    field: &'static str,
) -> Result<Option<u64>, PackageError> {
    match optional_unsigned(object, field)? {
        Some(0) | None => Ok(None),
        Some(value) => Ok(Some(value)),
    }
}

fn optional_u32_limit(
    object: &Map<String, Value>,
    field: &'static str,
) -> Result<Option<u32>, PackageError> {
    match optional_positive_u64(object, field)? {
        None => Ok(None),
        Some(value) => u32::try_from(value)
            .map(Some)
            .map_err(|_| PackageError::Validation {
                field,
                message: "is too large".to_string(),
            }),
    }
}

fn value_to_u64(value: &Value, field: &'static str) -> Result<u64, PackageError> {
    if let Some(number) = value.as_u64() {
        return Ok(number);
    }
    if let Some(text) = value.as_str()
        && let Ok(number) = text.trim().parse::<u64>()
    {
        return Ok(number);
    }
    Err(PackageError::Validation {
        field,
        message: "must be a non-negative integer".to_string(),
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::PackageInput;
    use crate::packages::error::PackageError;

    #[test]
    fn parses_minimal_package() {
        let input = PackageInput::from_json(&json!({ "name": "  Daily 100M  " })).expect("valid");
        assert_eq!(input.name, "Daily 100M");
        assert_eq!(input.price_cents, 0);
        assert!(input.enabled);
        assert_eq!(input.validity_secs, None);
        assert_eq!(input.data_limit_bytes, None);
        assert_eq!(input.device_limit, None);
    }

    #[test]
    fn parses_full_package_and_normalises_unlimited() {
        let input = PackageInput::from_json(&json!({
            "name": "Weekly",
            "description": "7 day plan",
            "price_cents": 1500,
            "enabled": false,
            "validity_secs": 604800,
            "usage_limit_secs": 0,
            "data_limit_bytes": 10737418240u64,
            "upload_rate_bps": 2000000,
            "download_rate_bps": "5000000",
            "device_limit": 2
        }))
        .expect("valid");

        assert_eq!(input.price_cents, 1500);
        assert!(!input.enabled);
        assert_eq!(input.validity_secs, Some(604800));
        assert_eq!(input.usage_limit_secs, None, "0 means unlimited");
        assert_eq!(input.data_limit_bytes, Some(10_737_418_240));
        assert_eq!(input.download_rate_bps, Some(5_000_000));
        assert_eq!(input.device_limit, Some(2));
    }

    #[test]
    fn rejects_missing_or_empty_name() {
        assert!(matches!(
            PackageInput::from_json(&json!({})),
            Err(PackageError::Validation { field: "name", .. })
        ));
        assert!(matches!(
            PackageInput::from_json(&json!({ "name": "   " })),
            Err(PackageError::Validation { field: "name", .. })
        ));
    }

    #[test]
    fn rejects_invalid_numbers() {
        assert!(matches!(
            PackageInput::from_json(&json!({ "name": "x", "price_cents": -5 })),
            Err(PackageError::Validation {
                field: "price_cents",
                ..
            })
        ));
        assert!(matches!(
            PackageInput::from_json(&json!({ "name": "x", "data_limit_bytes": "lots" })),
            Err(PackageError::Validation {
                field: "data_limit_bytes",
                ..
            })
        ));
    }

    #[test]
    fn rejects_non_object_body() {
        assert!(matches!(
            PackageInput::from_json(&json!([1, 2, 3])),
            Err(PackageError::Validation { field: "body", .. })
        ));
    }
}
