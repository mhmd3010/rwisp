use md5::{Digest, Md5};
use serde_json::{Value, json};
use sqlx::{MySqlPool, Row};
use subtle::ConstantTimeEq;

use crate::accounts::{self, Account, AccountStatus, SubscriptionStatus};
use crate::packages;
use crate::radius::attributes::{RadiusRequest, RadiusResponse, rate_limit};
use crate::radius::error::RadiusError;
use crate::vouchers::{self, Voucher, VoucherStatus};

/// HTTP status + JSON body returned to `rlm_rest`.
///
/// `rlm_rest` maps these codes: 2xx -> ok, 401 -> reject, 403 -> userlock,
/// 404 -> notfound, 5xx -> fail (closed).
pub struct AuthorizeOutcome {
    pub status: u16,
    pub body: Value,
}

impl AuthorizeOutcome {
    fn accept(response: RadiusResponse) -> Self {
        Self {
            status: 200,
            body: response.into_json(),
        }
    }

    fn ok() -> Self {
        Self {
            status: 200,
            body: json!({}),
        }
    }

    fn reject() -> Self {
        Self {
            status: 401,
            body: json!({}),
        }
    }

    fn locked() -> Self {
        Self {
            status: 403,
            body: json!({}),
        }
    }

    fn not_found() -> Self {
        Self {
            status: 404,
            body: json!({}),
        }
    }
}

/// Authorization phase: policy only.
///
/// Resolves the account/voucher, enforces state, expiry, router restriction and
/// cumulative limits, and returns the reply attributes plus `Auth-Type := rest`.
/// It performs **no** password check and **no** activation, so authentication
/// cannot be bypassed and nothing is activated before the credential is proven.
pub async fn authorize(
    pool: &MySqlPool,
    request: &RadiusRequest,
) -> Result<AuthorizeOutcome, RadiusError> {
    let username = request
        .get("User-Name")
        .ok_or_else(|| RadiusError::BadRequest {
            message: "User-Name is required".to_string(),
        })?;

    if let Some(account) = accounts::get_account_by_username(pool, username).await? {
        return policy_account(pool, request, account).await;
    }
    if let Some(voucher) = vouchers::get_voucher_by_code(pool, username).await? {
        return policy_voucher(pool, request, voucher).await;
    }
    Ok(AuthorizeOutcome::not_found())
}

/// Authentication phase (`Auth-Type rest`): verifies the credential and only
/// then performs the atomic first-login activation.
pub async fn authenticate(
    pool: &MySqlPool,
    request: &RadiusRequest,
) -> Result<AuthorizeOutcome, RadiusError> {
    let username = request
        .get("User-Name")
        .ok_or_else(|| RadiusError::BadRequest {
            message: "User-Name is required".to_string(),
        })?;

    if let Some(account) = accounts::get_account_by_username(pool, username).await? {
        return authenticate_account(pool, request, account).await;
    }
    if let Some(voucher) = vouchers::get_voucher_by_code(pool, username).await? {
        return authenticate_voucher(pool, request, voucher).await;
    }
    Ok(AuthorizeOutcome::not_found())
}

async fn policy_account(
    pool: &MySqlPool,
    request: &RadiusRequest,
    account: Account,
) -> Result<AuthorizeOutcome, RadiusError> {
    if let Some(router_id) = account.router_id
        && !router_allows(pool, router_id, request).await?
    {
        return Ok(AuthorizeOutcome::locked());
    }

    match account.status {
        AccountStatus::Suspended | AccountStatus::Disabled => return Ok(AuthorizeOutcome::locked()),
        AccountStatus::Active => {
            if account.subscription_status == SubscriptionStatus::Expired {
                return Ok(AuthorizeOutcome::reject());
            }
        }
        AccountStatus::Pending => {}
    }

    if let Some(expires_at) = account.expires_at.as_deref()
        && remaining_seconds(pool, expires_at).await? <= 0
    {
        return Ok(AuthorizeOutcome::reject());
    }

    let start =
        active_subscription_start(pool, account.id, account.activated_at.as_deref()).await?;
    let usage = account_usage(pool, account.id, start.as_deref()).await?;
    if !within_limits(pool, account.package_id, &usage).await? {
        return Ok(AuthorizeOutcome::reject());
    }

    let response = package_response(
        pool,
        account.package_id,
        account.expires_at.as_deref(),
        start.as_deref(),
    )
    .await?;
    Ok(AuthorizeOutcome::accept(response))
}

async fn policy_voucher(
    pool: &MySqlPool,
    request: &RadiusRequest,
    voucher: Voucher,
) -> Result<AuthorizeOutcome, RadiusError> {
    if let Some(router_id) = voucher.router_id
        && !router_allows(pool, router_id, request).await?
    {
        return Ok(AuthorizeOutcome::locked());
    }

    match voucher.status {
        VoucherStatus::Revoked => return Ok(AuthorizeOutcome::locked()),
        VoucherStatus::Expired => return Ok(AuthorizeOutcome::reject()),
        VoucherStatus::Unused | VoucherStatus::Active => {}
    }

    if let Some(expires_at) = voucher.expires_at.as_deref()
        && remaining_seconds(pool, expires_at).await? <= 0
    {
        return Ok(AuthorizeOutcome::reject());
    }

    let usage = voucher_usage(pool, voucher.id, voucher.activated_at.as_deref()).await?;
    if !within_limits(pool, Some(voucher.package_id), &usage).await? {
        return Ok(AuthorizeOutcome::reject());
    }

    let response = package_response(
        pool,
        Some(voucher.package_id),
        voucher.expires_at.as_deref(),
        voucher.activated_at.as_deref(),
    )
    .await?;
    Ok(AuthorizeOutcome::accept(response))
}

async fn authenticate_account(
    pool: &MySqlPool,
    request: &RadiusRequest,
    account: Account,
) -> Result<AuthorizeOutcome, RadiusError> {
    if matches!(
        account.status,
        AccountStatus::Suspended | AccountStatus::Disabled
    ) {
        return Ok(AuthorizeOutcome::locked());
    }

    // Accounts are Argon2id: PAP only. CHAP would require the plaintext secret.
    let Some(password) = request.get("User-Password") else {
        return Ok(AuthorizeOutcome::reject());
    };
    if accounts::verify_account_password(pool, &account.username, password)
        .await?
        .is_none()
    {
        return Ok(AuthorizeOutcome::reject());
    }

    if account.status == AccountStatus::Pending {
        match accounts::activate_account(pool, account.id).await? {
            accounts::ActivationOutcome::Activated(_) => {}
            accounts::ActivationOutcome::NotPending { .. } => {}
            accounts::ActivationOutcome::NotFound => return Ok(AuthorizeOutcome::not_found()),
        }
    }

    let account = accounts::get_account(pool, account.id)
        .await?
        .ok_or_else(|| RadiusError::Internal {
            message: "account vanished during authentication".to_string(),
        })?;
    if account.subscription_status == SubscriptionStatus::Expired {
        return Ok(AuthorizeOutcome::reject());
    }
    if let Some(expires_at) = account.expires_at.as_deref()
        && remaining_seconds(pool, expires_at).await? <= 0
    {
        return Ok(AuthorizeOutcome::reject());
    }

    Ok(AuthorizeOutcome::ok())
}

async fn authenticate_voucher(
    pool: &MySqlPool,
    request: &RadiusRequest,
    voucher: Voucher,
) -> Result<AuthorizeOutcome, RadiusError> {
    match voucher.status {
        VoucherStatus::Revoked => return Ok(AuthorizeOutcome::locked()),
        VoucherStatus::Expired => return Ok(AuthorizeOutcome::reject()),
        VoucherStatus::Unused | VoucherStatus::Active => {}
    }

    if !verify_voucher_credential(request, &voucher.code) {
        return Ok(AuthorizeOutcome::reject());
    }

    if voucher.status == VoucherStatus::Unused {
        match vouchers::activate_voucher(pool, &voucher.code).await? {
            vouchers::ActivationOutcome::Activated(_) => {}
            vouchers::ActivationOutcome::NotUnused { .. } => {}
            vouchers::ActivationOutcome::NotFound => return Ok(AuthorizeOutcome::not_found()),
        }
    }

    let voucher = vouchers::get_voucher_by_code(pool, &voucher.code)
        .await?
        .ok_or_else(|| RadiusError::Internal {
            message: "voucher vanished during authentication".to_string(),
        })?;
    if let Some(expires_at) = voucher.expires_at.as_deref()
        && remaining_seconds(pool, expires_at).await? <= 0
    {
        return Ok(AuthorizeOutcome::reject());
    }

    Ok(AuthorizeOutcome::ok())
}

fn verify_voucher_credential(request: &RadiusRequest, code: &str) -> bool {
    if let Some(password) = request.get("User-Password") {
        return password.as_bytes().ct_eq(code.as_bytes()).into();
    }
    let (Some(chap_password), Some(chap_challenge)) =
        (request.get("CHAP-Password"), request.get("CHAP-Challenge"))
    else {
        return false;
    };
    verify_chap(chap_password, chap_challenge, code)
}

#[allow(deprecated)]
fn verify_chap(chap_password_hex: &str, chap_challenge_hex: &str, secret: &str) -> bool {
    let Some(chap_password) = decode_hex(chap_password_hex) else {
        return false;
    };
    let Some(challenge) = decode_hex(chap_challenge_hex) else {
        return false;
    };
    if chap_password.len() != 17 {
        return false;
    }

    let mut hasher = Md5::new();
    hasher.update([chap_password[0]]);
    hasher.update(secret.as_bytes());
    hasher.update(&challenge);
    let digest = hasher.finalize();
    digest.as_slice().ct_eq(&chap_password[1..]).into()
}

#[derive(Debug, Default)]
struct Usage {
    bytes: u64,
    secs: u64,
}

async fn within_limits(
    pool: &MySqlPool,
    package_id: Option<u64>,
    usage: &Usage,
) -> Result<bool, RadiusError> {
    let Some(package_id) = package_id else {
        return Ok(true);
    };
    let Some(package) = packages::get_package(pool, package_id).await? else {
        return Ok(true);
    };
    if let Some(limit) = package.data_limit_bytes
        && usage.bytes >= limit
    {
        return Ok(false);
    }
    if let Some(limit) = package.usage_limit_secs
        && usage.secs >= limit
    {
        return Ok(false);
    }
    Ok(true)
}

async fn package_response(
    pool: &MySqlPool,
    package_id: Option<u64>,
    expires_at: Option<&str>,
    subscription_start: Option<&str>,
) -> Result<RadiusResponse, RadiusError> {
    let mut response = RadiusResponse::new();
    // Force the authenticate section to run; never `Accept`.
    response.set("control:Auth-Type", "rest");

    if let Some(package_id) = package_id
        && let Some(package) = packages::get_package(pool, package_id).await?
    {
        if let Some(limit) = rate_limit(package.upload_rate_bps, package.download_rate_bps) {
            response.set("reply:Mikrotik-Rate-Limit", limit);
        }
        if let Some(devices) = package.device_limit {
            response.set("control:Simultaneous-Use", devices.to_string());
        }
        if let Some(bytes) = package.data_limit_bytes {
            response.set("reply:Max-Octets", bytes.to_string());
        }
        if let Some(secs) = package.usage_limit_secs {
            response.set("reply:Max-All-Session", secs.to_string());
        }
    }

    if let Some(start) = subscription_start {
        response.set("reply:RWISP-Subscription-Start", start);
    }

    if let Some(expires_at) = expires_at {
        let remaining = remaining_seconds(pool, expires_at).await?;
        if remaining > 0 {
            response.set("reply:Session-Timeout", remaining.to_string());
        }
    }

    Ok(response)
}

async fn active_subscription_start(
    pool: &MySqlPool,
    account_id: u64,
    activated_at: Option<&str>,
) -> Result<Option<String>, RadiusError> {
    let start: Option<String> = sqlx::query_scalar(
        "SELECT CAST(COALESCE((SELECT starts_at FROM tbl_account_packages \
                 WHERE account_id = a.id AND status = 'active' ORDER BY id DESC LIMIT 1), \
                 a.activated_at) AS CHAR) FROM tbl_accounts a WHERE a.id = ?",
    )
    .bind(account_id)
    .fetch_optional(pool)
    .await?
    .flatten();

    Ok(start.or_else(|| activated_at.map(str::to_string)))
}

async fn account_usage(
    pool: &MySqlPool,
    account_id: u64,
    start: Option<&str>,
) -> Result<Usage, RadiusError> {
    let row = sqlx::query(
        "SELECT CAST(COALESCE(SUM(input_octets + output_octets), 0) AS UNSIGNED) AS bytes, \
                CAST(COALESCE(SUM(session_time), 0) AS UNSIGNED) AS secs \
         FROM tbl_radius_sessions \
         WHERE account_id = ? AND (? IS NULL OR started_at IS NULL OR started_at >= CAST(? AS DATETIME))",
    )
    .bind(account_id)
    .bind(start)
    .bind(start)
    .fetch_one(pool)
    .await?;
    Ok(Usage {
        bytes: row.try_get("bytes")?,
        secs: row.try_get("secs")?,
    })
}

async fn voucher_usage(
    pool: &MySqlPool,
    voucher_id: u64,
    start: Option<&str>,
) -> Result<Usage, RadiusError> {
    let row = sqlx::query(
        "SELECT CAST(COALESCE(SUM(input_octets + output_octets), 0) AS UNSIGNED) AS bytes, \
                CAST(COALESCE(SUM(session_time), 0) AS UNSIGNED) AS secs \
         FROM tbl_radius_sessions \
         WHERE voucher_id = ? AND (? IS NULL OR started_at IS NULL OR started_at >= CAST(? AS DATETIME))",
    )
    .bind(voucher_id)
    .bind(start)
    .bind(start)
    .fetch_one(pool)
    .await?;
    Ok(Usage {
        bytes: row.try_get("bytes")?,
        secs: row.try_get("secs")?,
    })
}

async fn remaining_seconds(pool: &MySqlPool, expires_at: &str) -> Result<i64, RadiusError> {
    let seconds: i64 =
        sqlx::query_scalar("SELECT TIMESTAMPDIFF(SECOND, NOW(), CAST(? AS DATETIME))")
            .bind(expires_at)
            .fetch_one(pool)
            .await?;
    Ok(seconds)
}

async fn router_allows(
    pool: &MySqlPool,
    router_id: u64,
    request: &RadiusRequest,
) -> Result<bool, RadiusError> {
    let row =
        sqlx::query("SELECT host, radius_nas_ip, nas_identifier FROM tbl_routers WHERE id = ?")
            .bind(router_id)
            .fetch_optional(pool)
            .await?;
    let Some(row) = row else {
        return Ok(false);
    };

    let host: String = row.try_get("host")?;
    let radius_nas_ip: Option<String> = row.try_get("radius_nas_ip")?;
    let nas_identifier: Option<String> = row.try_get("nas_identifier")?;

    let request_nas_ip = request.get("NAS-IP-Address");
    let request_nas_id = request.get("NAS-Identifier");

    if let (Some(configured), Some(actual)) = (nas_identifier.as_deref(), request_nas_id)
        && configured == actual
    {
        return Ok(true);
    }
    if let (Some(configured), Some(actual)) = (radius_nas_ip.as_deref(), request_nas_ip)
        && configured == actual
    {
        return Ok(true);
    }
    if let Some(actual) = request_nas_ip
        && host == actual
    {
        return Ok(true);
    }

    Ok(false)
}

fn decode_hex(input: &str) -> Option<Vec<u8>> {
    let input = input.trim();
    if !input.len().is_multiple_of(2) {
        return None;
    }
    let mut output = Vec::with_capacity(input.len() / 2);
    for pair in input.as_bytes().as_chunks::<2>().0 {
        let high = (pair[0] as char).to_digit(16)?;
        let low = (pair[1] as char).to_digit(16)?;
        output.push(((high << 4) | low) as u8);
    }
    Some(output)
}

#[cfg(test)]
mod tests {
    use super::{decode_hex, verify_chap};

    #[test]
    fn decodes_hex() {
        assert_eq!(
            decode_hex("00ff10").as_deref(),
            Some(&[0x00, 0xff, 0x10][..])
        );
        assert_eq!(decode_hex("abc"), None);
        assert_eq!(decode_hex("zz"), None);
    }

    #[test]
    fn verifies_chap_md5() {
        use md5::{Digest, Md5};

        let secret = "VOUCHER-CODE";
        let chap_id = 7u8;
        let challenge = [1u8, 2, 3, 4, 5, 6, 7, 8];
        let mut hasher = Md5::new();
        hasher.update([chap_id]);
        hasher.update(secret.as_bytes());
        hasher.update(challenge);
        let digest = hasher.finalize();

        let mut packet = vec![chap_id];
        packet.extend_from_slice(&digest);

        let hex = |bytes: &[u8]| bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
        assert!(verify_chap(&hex(&packet), &hex(&challenge), secret));
        assert!(!verify_chap(&hex(&packet), &hex(&challenge), "WRONG"));
    }
}
