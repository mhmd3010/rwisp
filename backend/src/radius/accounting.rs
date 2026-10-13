use sqlx::{MySqlPool, Row};

use crate::accounts;
use crate::radius::attributes::RadiusRequest;
use crate::radius::error::RadiusError;
use crate::vouchers;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AcctStatus {
    Start,
    Interim,
    Stop,
}

fn combine_gigawords(octets: u64, gigawords: u64) -> u64 {
    octets.saturating_add(gigawords.saturating_mul(1 << 32))
}

impl AcctStatus {
    fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "start" => Some(Self::Start),
            "interim-update" | "interim" => Some(Self::Interim),
            "stop" => Some(Self::Stop),
            _ => None,
        }
    }

    fn as_str(&self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Interim => "interim",
            Self::Stop => "stop",
        }
    }
}

/// Process an accounting Start/Interim/Stop idempotently.
///
/// Sessions are keyed on (nas_ip, acct_session_id); counters only ever move
/// forward, and account/voucher totals are recomputed from the session rows so
/// replays never double-count. Radacct is still written by `rlm_sql`.
pub async fn process(pool: &MySqlPool, request: &RadiusRequest) -> Result<(), RadiusError> {
    let status_raw = request
        .get("Acct-Status-Type")
        .ok_or_else(|| RadiusError::BadRequest {
            message: "Acct-Status-Type is required".to_string(),
        })?;
    let Some(status) = AcctStatus::parse(status_raw) else {
        // Accounting-On/Off and other types are not session events.
        return Ok(());
    };

    let username = request
        .get("User-Name")
        .ok_or_else(|| RadiusError::BadRequest {
            message: "User-Name is required".to_string(),
        })?;
    let acct_session_id =
        request
            .get("Acct-Session-Id")
            .ok_or_else(|| RadiusError::BadRequest {
                message: "Acct-Session-Id is required".to_string(),
            })?;

    let nas_ip = request.get("NAS-IP-Address").unwrap_or_default();
    let session_time = request.get_u64("Acct-Session-Time").unwrap_or(0);
    // 64-bit counters: Acct-*-Octets is 32-bit; the RFC 2869 gigawords carry
    // the high 32 bits. Combine them so sessions beyond 4 GiB stay accurate.
    let input_octets = combine_gigawords(
        request.get_u64("Acct-Input-Octets").unwrap_or(0),
        request.get_u64("Acct-Input-Gigawords").unwrap_or(0),
    );
    let output_octets = combine_gigawords(
        request.get_u64("Acct-Output-Octets").unwrap_or(0),
        request.get_u64("Acct-Output-Gigawords").unwrap_or(0),
    );

    let account_id = accounts::get_account_by_username(pool, username)
        .await?
        .map(|account| account.id);
    let voucher_id = if account_id.is_none() {
        vouchers::get_voucher_by_code(pool, username)
            .await?
            .map(|voucher| voucher.id)
    } else {
        None
    };

    let mut tx = pool.begin().await?;

    sqlx::query(
        "INSERT INTO tbl_radius_sessions \
         (username, acct_session_id, acct_unique_id, nas_ip, nas_identifier, calling_station_id, \
          framed_ip, account_id, voucher_id, status, session_time, input_octets, output_octets, \
          terminate_cause, started_at, updated_at, stopped_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, \
                 IF(? = 'start', NOW(), NULL), NOW(), IF(? = 'stop', NOW(), NULL)) \
         ON DUPLICATE KEY UPDATE \
          account_id = COALESCE(VALUES(account_id), account_id), \
          voucher_id = COALESCE(VALUES(voucher_id), voucher_id), \
          nas_identifier = COALESCE(VALUES(nas_identifier), nas_identifier), \
          calling_station_id = COALESCE(VALUES(calling_station_id), calling_station_id), \
          framed_ip = COALESCE(VALUES(framed_ip), framed_ip), \
          session_time = GREATEST(session_time, VALUES(session_time)), \
          input_octets = GREATEST(input_octets, VALUES(input_octets)), \
          output_octets = GREATEST(output_octets, VALUES(output_octets)), \
          status = IF(VALUES(status) = 'stop' OR status = 'stop', 'stop', VALUES(status)), \
          terminate_cause = COALESCE(VALUES(terminate_cause), terminate_cause), \
          started_at = COALESCE(started_at, NOW()), \
          stopped_at = IF(VALUES(status) = 'stop', NOW(), stopped_at), \
          updated_at = NOW()",
    )
    .bind(username)
    .bind(acct_session_id)
    .bind(request.get("Acct-Unique-Session-Id"))
    .bind(nas_ip)
    .bind(request.get("NAS-Identifier"))
    .bind(request.get("Calling-Station-Id"))
    .bind(request.get("Framed-IP-Address"))
    .bind(account_id)
    .bind(voucher_id)
    .bind(status.as_str())
    .bind(session_time)
    .bind(input_octets)
    .bind(output_octets)
    .bind(request.get("Acct-Terminate-Cause"))
    .bind(status.as_str())
    .bind(status.as_str())
    .execute(&mut *tx)
    .await?;

    if let Some(account_id) = account_id {
        sqlx::query(
            "UPDATE tbl_accounts a SET \
                a.bytes_in = (SELECT COALESCE(SUM(input_octets), 0) \
                              FROM tbl_radius_sessions WHERE account_id = a.id), \
                a.bytes_out = (SELECT COALESCE(SUM(output_octets), 0) \
                               FROM tbl_radius_sessions WHERE account_id = a.id), \
                a.usage_secs = LEAST(4294967295, (SELECT COALESCE(SUM(session_time), 0) \
                                   FROM tbl_radius_sessions WHERE account_id = a.id)), \
                a.last_login_at = COALESCE(a.last_login_at, NOW()) \
             WHERE a.id = ?",
        )
        .bind(account_id)
        .execute(&mut *tx)
        .await?;
    }

    if let Some(voucher_id) = voucher_id {
        sqlx::query(
            "UPDATE tbl_vouchers v SET \
                v.bytes_in = (SELECT COALESCE(SUM(input_octets), 0) \
                              FROM tbl_radius_sessions WHERE voucher_id = v.id), \
                v.bytes_out = (SELECT COALESCE(SUM(output_octets), 0) \
                               FROM tbl_radius_sessions WHERE voucher_id = v.id), \
                v.usage_secs = LEAST(4294967295, (SELECT COALESCE(SUM(session_time), 0) \
                                   FROM tbl_radius_sessions WHERE voucher_id = v.id)), \
                v.last_used_at = COALESCE(v.last_used_at, NOW()) \
             WHERE v.id = ?",
        )
        .bind(voucher_id)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;
    Ok(())
}

/// Open sessions whose owning account/voucher no longer permits access.
pub async fn open_disconnect_candidates(
    pool: &MySqlPool,
) -> Result<Vec<crate::radius::coa::DisconnectTarget>, RadiusError> {
    let rows = sqlx::query(
        "SELECT s.id, s.username, s.nas_ip, s.acct_session_id \
         FROM tbl_radius_sessions s \
         LEFT JOIN tbl_accounts a ON a.id = s.account_id \
         LEFT JOIN tbl_vouchers v ON v.id = s.voucher_id \
         WHERE s.status <> 'stop' \
           AND (a.status IN ('suspended', 'disabled') \
                OR v.status IN ('revoked', 'expired') \
                OR (a.id IS NOT NULL AND a.expires_at IS NOT NULL AND a.expires_at < NOW()) \
                OR (v.id IS NOT NULL AND v.expires_at IS NOT NULL AND v.expires_at < NOW()))",
    )
    .fetch_all(pool)
    .await?;

    let mut targets = Vec::with_capacity(rows.len());
    for row in rows {
        targets.push(crate::radius::coa::DisconnectTarget {
            session_id: row.try_get("id")?,
            username: row.try_get("username")?,
            nas_ip: row.try_get("nas_ip")?,
            acct_session_id: row.try_get("acct_session_id")?,
        });
    }
    Ok(targets)
}
