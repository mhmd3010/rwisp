use std::collections::HashMap;

use serde_json::{Map, Value};

use crate::accounts::error::AccountError;
use crate::accounts::password::validate_password;

pub const DEFAULT_PAGE_SIZE: u32 = 50;
pub const MAX_PAGE_SIZE: u32 = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccountStatus {
    Pending,
    Active,
    Suspended,
    Disabled,
}

impl AccountStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Active => "active",
            Self::Suspended => "suspended",
            Self::Disabled => "disabled",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "pending" => Some(Self::Pending),
            "active" => Some(Self::Active),
            "suspended" => Some(Self::Suspended),
            "disabled" => Some(Self::Disabled),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubscriptionStatus {
    Active,
    Scheduled,
    Expired,
    None,
}

impl SubscriptionStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Scheduled => "scheduled",
            Self::Expired => "expired",
            Self::None => "none",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "active" => Some(Self::Active),
            "scheduled" => Some(Self::Scheduled),
            "expired" => Some(Self::Expired),
            "none" => Some(Self::None),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackageActivation {
    Immediate,
    NextExpiry,
}

impl PackageActivation {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Immediate => "immediate",
            Self::NextExpiry => "next_expiry",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "immediate" => Some(Self::Immediate),
            "next_expiry" | "next-expiry" | "next_expiration" => Some(Self::NextExpiry),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccountPackageStatus {
    Active,
    Scheduled,
    Expired,
    Completed,
    Cancelled,
}

impl AccountPackageStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Scheduled => "scheduled",
            Self::Expired => "expired",
            Self::Completed => "completed",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "active" => Some(Self::Active),
            "scheduled" => Some(Self::Scheduled),
            "expired" => Some(Self::Expired),
            "completed" => Some(Self::Completed),
            "cancelled" => Some(Self::Cancelled),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    pub id: u64,
    pub username: String,
    pub full_name: Option<String>,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub address: Option<String>,
    pub router_id: Option<u64>,
    pub package_id: Option<u64>,
    pub status: AccountStatus,
    pub subscription_status: SubscriptionStatus,
    pub activated_at: Option<String>,
    pub expires_at: Option<String>,
    pub last_login_at: Option<String>,
    pub last_login_ip: Option<String>,
    pub usage_secs: u64,
    pub bytes_in: u64,
    pub bytes_out: u64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountPackage {
    pub id: u64,
    pub account_id: u64,
    pub package_id: u64,
    pub activation: PackageActivation,
    pub status: AccountPackageStatus,
    pub starts_at: Option<String>,
    pub expires_at: Option<String>,
    pub activated_at: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewAccount {
    pub username: String,
    pub password: String,
    pub full_name: Option<String>,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub address: Option<String>,
    pub router_id: Option<u64>,
    pub package_id: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountUpdate {
    pub full_name: Option<String>,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub address: Option<String>,
    pub router_id: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PasswordReset {
    pub password: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenewalRequest {
    pub package_id: u64,
    pub activation: PackageActivation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenewalResult {
    pub account: Account,
    pub assignment: AccountPackage,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActivationOutcome {
    Activated(Box<Account>),
    NotFound,
    NotPending { status: AccountStatus },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StatusOutcome {
    Updated(Box<Account>),
    NotFound,
    InvalidState { status: AccountStatus },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountFilter {
    pub username: Option<String>,
    pub status: Option<AccountStatus>,
    pub package_id: Option<u64>,
    pub router_id: Option<u64>,
    pub limit: u32,
    pub offset: u32,
}

impl Default for AccountFilter {
    fn default() -> Self {
        Self {
            username: None,
            status: None,
            package_id: None,
            router_id: None,
            limit: DEFAULT_PAGE_SIZE,
            offset: 0,
        }
    }
}

impl AccountFilter {
    pub fn from_query(params: &HashMap<String, String>) -> Result<Self, AccountError> {
        let status = match params.get("status") {
            Some(raw) => {
                Some(
                    AccountStatus::parse(raw).ok_or_else(|| AccountError::Validation {
                        field: "status",
                        message: "must be one of pending, active, suspended, disabled".to_string(),
                    })?,
                )
            }
            None => None,
        };

        let username = match params.get("username") {
            Some(raw) if !raw.trim().is_empty() => {
                let value = raw.trim();
                if value.chars().count() > 100 {
                    return Err(AccountError::Validation {
                        field: "username",
                        message: "must be at most 100 characters".to_string(),
                    });
                }
                Some(value.to_string())
            }
            _ => None,
        };

        let limit = match params.get("limit") {
            Some(raw) => raw
                .trim()
                .parse::<u32>()
                .map_err(|_| AccountError::Validation {
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
                .map_err(|_| AccountError::Validation {
                    field: "offset",
                    message: "must be a non-negative integer".to_string(),
                })?,
            None => 0,
        };

        Ok(Self {
            username,
            status,
            package_id: optional_query_u64(params, "package_id")?,
            router_id: optional_query_u64(params, "router_id")?,
            limit,
            offset,
        })
    }
}

impl NewAccount {
    pub fn from_json(value: &Value) -> Result<Self, AccountError> {
        let object = object(value)?;

        let username = required_username(object)?;
        let password = required_password(object)?;

        Ok(Self {
            username,
            password,
            full_name: optional_string(object, "full_name", 150)?,
            email: optional_string(object, "email", 190)?,
            phone: optional_string(object, "phone", 40)?,
            address: optional_string(object, "address", 255)?,
            router_id: optional_u64(object, "router_id")?,
            package_id: optional_u64(object, "package_id")?,
        })
    }
}

impl AccountUpdate {
    pub fn from_json(value: &Value) -> Result<Self, AccountError> {
        let object = object(value)?;

        Ok(Self {
            full_name: optional_string(object, "full_name", 150)?,
            email: optional_string(object, "email", 190)?,
            phone: optional_string(object, "phone", 40)?,
            address: optional_string(object, "address", 255)?,
            router_id: optional_u64(object, "router_id")?,
        })
    }
}

impl PasswordReset {
    pub fn from_json(value: &Value) -> Result<Self, AccountError> {
        let object = object(value)?;
        Ok(Self {
            password: required_password(object)?,
        })
    }
}

impl RenewalRequest {
    pub fn from_json(value: &Value) -> Result<Self, AccountError> {
        let object = object(value)?;

        let package_id =
            optional_u64(object, "package_id")?.ok_or_else(|| AccountError::Validation {
                field: "package_id",
                message: "is required".to_string(),
            })?;

        let activation = match object.get("activation") {
            None | Some(Value::Null) => PackageActivation::Immediate,
            Some(Value::String(raw)) => {
                PackageActivation::parse(raw).ok_or_else(|| AccountError::Validation {
                    field: "activation",
                    message: "must be immediate or next_expiry".to_string(),
                })?
            }
            Some(_) => {
                return Err(AccountError::Validation {
                    field: "activation",
                    message: "must be a string".to_string(),
                });
            }
        };

        Ok(Self {
            package_id,
            activation,
        })
    }
}

fn object(value: &Value) -> Result<&Map<String, Value>, AccountError> {
    value.as_object().ok_or_else(|| AccountError::Validation {
        field: "body",
        message: "expected a JSON object".to_string(),
    })
}

fn required_username(object: &Map<String, Value>) -> Result<String, AccountError> {
    let value = object
        .get("username")
        .ok_or_else(|| AccountError::Validation {
            field: "username",
            message: "is required".to_string(),
        })?;
    let username = value
        .as_str()
        .ok_or_else(|| AccountError::Validation {
            field: "username",
            message: "must be a string".to_string(),
        })?
        .trim()
        .to_string();

    if username.chars().count() < 3 {
        return Err(AccountError::Validation {
            field: "username",
            message: "must be at least 3 characters".to_string(),
        });
    }
    if username.chars().count() > 100 {
        return Err(AccountError::Validation {
            field: "username",
            message: "must be at most 100 characters".to_string(),
        });
    }
    if !username
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '@' | '-'))
    {
        return Err(AccountError::Validation {
            field: "username",
            message: "may contain only letters, digits, and . _ @ -".to_string(),
        });
    }

    Ok(username)
}

fn required_password(object: &Map<String, Value>) -> Result<String, AccountError> {
    let value = object
        .get("password")
        .ok_or_else(|| AccountError::Validation {
            field: "password",
            message: "is required".to_string(),
        })?;
    let password = value.as_str().ok_or_else(|| AccountError::Validation {
        field: "password",
        message: "must be a string".to_string(),
    })?;

    validate_password(password)?;
    Ok(password.to_string())
}

fn optional_u64(
    object: &Map<String, Value>,
    field: &'static str,
) -> Result<Option<u64>, AccountError> {
    match object.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value_to_u64(value, field).map(Some),
    }
}

fn value_to_u64(value: &Value, field: &'static str) -> Result<u64, AccountError> {
    if let Some(number) = value.as_u64() {
        return Ok(number);
    }
    if let Some(text) = value.as_str()
        && let Ok(number) = text.trim().parse::<u64>()
    {
        return Ok(number);
    }
    Err(AccountError::Validation {
        field,
        message: "must be a non-negative integer".to_string(),
    })
}

fn optional_string(
    object: &Map<String, Value>,
    field: &'static str,
    max_len: usize,
) -> Result<Option<String>, AccountError> {
    match object.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => {
            let text = value.as_str().ok_or_else(|| AccountError::Validation {
                field,
                message: "must be a string".to_string(),
            })?;
            let text = text.trim();
            if text.is_empty() {
                return Ok(None);
            }
            if text.chars().count() > max_len {
                return Err(AccountError::Validation {
                    field,
                    message: format!("must be at most {max_len} characters"),
                });
            }
            Ok(Some(text.to_string()))
        }
    }
}

fn optional_query_u64(
    params: &HashMap<String, String>,
    field: &'static str,
) -> Result<Option<u64>, AccountError> {
    match params.get(field) {
        None => Ok(None),
        Some(raw) => raw
            .trim()
            .parse::<u64>()
            .map(Some)
            .map_err(|_| AccountError::Validation {
                field,
                message: "must be a positive integer".to_string(),
            }),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use serde_json::json;

    use super::{AccountFilter, DEFAULT_PAGE_SIZE, NewAccount, PackageActivation, RenewalRequest};
    use crate::accounts::error::AccountError;

    #[test]
    fn parses_valid_new_account() {
        let account = NewAccount::from_json(&json!({
            "username": "  customer.01 ",
            "password": "super-secret-1",
            "full_name": "Customer One",
            "email": "c1@example.com",
            "router_id": "3",
            "package_id": 5
        }))
        .expect("valid");

        assert_eq!(account.username, "customer.01");
        assert_eq!(account.password, "super-secret-1");
        assert_eq!(account.router_id, Some(3));
        assert_eq!(account.package_id, Some(5));
    }

    #[test]
    fn rejects_bad_username_and_password() {
        assert!(matches!(
            NewAccount::from_json(&json!({ "username": "ab", "password": "longenough1" })),
            Err(AccountError::Validation {
                field: "username",
                ..
            })
        ));
        assert!(matches!(
            NewAccount::from_json(&json!({ "username": "bad name", "password": "longenough1" })),
            Err(AccountError::Validation {
                field: "username",
                ..
            })
        ));
        assert!(matches!(
            NewAccount::from_json(&json!({ "username": "validname", "password": "short" })),
            Err(AccountError::Validation {
                field: "password",
                ..
            })
        ));
    }

    #[test]
    fn parses_renewal_with_defaults() {
        let renewal = RenewalRequest::from_json(&json!({ "package_id": 4 })).expect("valid");
        assert_eq!(renewal.package_id, 4);
        assert_eq!(renewal.activation, PackageActivation::Immediate);

        let scheduled =
            RenewalRequest::from_json(&json!({ "package_id": 4, "activation": "next_expiry" }))
                .expect("valid");
        assert_eq!(scheduled.activation, PackageActivation::NextExpiry);

        assert!(
            RenewalRequest::from_json(&json!({ "package_id": 4, "activation": "whenever" }))
                .is_err()
        );
    }

    #[test]
    fn parses_filter_defaults() {
        let filter = AccountFilter::from_query(&HashMap::new()).expect("defaults");
        assert_eq!(filter.limit, DEFAULT_PAGE_SIZE);
        assert_eq!(filter.status, None);

        let mut params = HashMap::new();
        params.insert("status".to_string(), "Suspended".to_string());
        params.insert("username".to_string(), "cust".to_string());
        let filter = AccountFilter::from_query(&params).expect("filter");
        assert_eq!(filter.username.as_deref(), Some("cust"));
        assert_eq!(filter.status, Some(super::AccountStatus::Suspended));
    }
}
