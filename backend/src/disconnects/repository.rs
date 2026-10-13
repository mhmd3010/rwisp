use sqlx::mysql::MySqlRow;
use sqlx::{FromRow, MySqlPool, QueryBuilder, Row};

use crate::disconnects::error::DisconnectError;
use crate::disconnects::model::{
    DisconnectFilter, DisconnectMethod, DisconnectRecord, DisconnectStats, DisconnectStatus,
    NewDisconnect,
};

const COLS: &str = "id, username, account_id, voucher_id, session_row_id, router_id, nas_ip, \
    acct_session_id, reason, method, status, attempts, max_attempts, last_error, \
    CAST(next_attempt_at AS CHAR) AS next_attempt_at, CAST(created_at AS CHAR) AS created_at, \
    CAST(updated_at AS CHAR) AS updated_at, CAST(completed_at AS CHAR) AS completed_at";

impl FromRow<'_, MySqlRow> for DisconnectRecord {
    fn from_row(row: &MySqlRow) -> Result<Self, sqlx::Error> {
        let method_raw: String = row.try_get("method")?;
        let method =
            DisconnectMethod::parse(&method_raw).ok_or_else(|| decode("method", &method_raw))?;
        let status_raw: String = row.try_get("status")?;
        let status =
            DisconnectStatus::parse(&status_raw).ok_or_else(|| decode("status", &status_raw))?;

        Ok(Self {
            id: row.try_get("id")?,
            username: row.try_get("username")?,
            account_id: row.try_get("account_id")?,
            voucher_id: row.try_get("voucher_id")?,
            session_row_id: row.try_get("session_row_id")?,
            router_id: row.try_get("router_id")?,
            nas_ip: row.try_get("nas_ip")?,
            acct_session_id: row.try_get("acct_session_id")?,
            reason: row.try_get("reason")?,
            method,
            status,
            attempts: row.try_get("attempts")?,
            max_attempts: row.try_get("max_attempts")?,
            last_error: row.try_get("last_error")?,
            next_attempt_at: row.try_get("next_attempt_at")?,
            created_at: row.try_get("created_at")?,
            updated_at: row.try_get("updated_at")?,
            completed_at: row.try_get("completed_at")?,
        })
    }
}

fn decode(field: &str, value: &str) -> sqlx::Error {
    sqlx::Error::Decode(Box::new(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        format!("unknown {field} value: {value}"),
    )))
}

/// Enqueue a disconnect idempotently: if a pending disconnect already exists for
/// the same session, its id is returned instead of creating a duplicate.
pub async fn enqueue(pool: &MySqlPool, request: &NewDisconnect) -> Result<u64, DisconnectError> {
    let result = sqlx::query(
        "INSERT INTO tbl_disconnects \
         (username, account_id, voucher_id, session_row_id, router_id, nas_ip, acct_session_id, \
          reason, method, max_attempts, next_attempt_at) \
         SELECT ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, NOW() FROM DUAL \
         WHERE NOT EXISTS ( \
            SELECT 1 FROM tbl_disconnects \
            WHERE status = 'pending' AND ( \
                (acct_session_id IS NOT NULL AND acct_session_id = ?) \
                OR (session_row_id IS NOT NULL AND session_row_id = ?) \
                OR (acct_session_id IS NULL AND session_row_id IS NULL AND username = ?) \
            ) \
         )",
    )
    .bind(&request.username)
    .bind(request.account_id)
    .bind(request.voucher_id)
    .bind(request.session_row_id)
    .bind(request.router_id)
    .bind(&request.nas_ip)
    .bind(&request.acct_session_id)
    .bind(&request.reason)
    .bind(request.method.as_str())
    .bind(request.max_attempts)
    .bind(&request.acct_session_id)
    .bind(request.session_row_id)
    .bind(&request.username)
    .execute(pool)
    .await?;

    if result.last_insert_id() != 0 {
        return Ok(result.last_insert_id());
    }

    // Already pending: return the existing row id.
    let existing: Option<u64> = sqlx::query_scalar(
        "SELECT id FROM tbl_disconnects WHERE status = 'pending' \
         AND ((acct_session_id IS NOT NULL AND acct_session_id <=> ?) \
              OR (session_row_id IS NOT NULL AND session_row_id <=> ?) \
              OR (acct_session_id IS NULL AND session_row_id IS NULL AND username = ?)) \
         ORDER BY id LIMIT 1",
    )
    .bind(&request.acct_session_id)
    .bind(request.session_row_id)
    .bind(&request.username)
    .fetch_optional(pool)
    .await?;

    existing.ok_or(DisconnectError::Validation {
        field: "disconnect",
        message: "could not enqueue disconnect".to_string(),
    })
}

pub async fn get(pool: &MySqlPool, id: u64) -> Result<Option<DisconnectRecord>, DisconnectError> {
    let query = format!("SELECT {COLS} FROM tbl_disconnects WHERE id = ?");
    Ok(
        sqlx::query_as::<_, DisconnectRecord>(sqlx::AssertSqlSafe(query))
            .bind(id)
            .fetch_optional(pool)
            .await?,
    )
}

pub async fn list(
    pool: &MySqlPool,
    filter: &DisconnectFilter,
) -> Result<Vec<DisconnectRecord>, DisconnectError> {
    let mut builder = QueryBuilder::new(format!("SELECT {COLS} FROM tbl_disconnects WHERE 1 = 1"));
    if let Some(status) = filter.status {
        builder.push(" AND status = ").push_bind(status.as_str());
    }
    if let Some(username) = &filter.username {
        builder
            .push(" AND username LIKE ")
            .push_bind(format!("%{username}%"));
    }
    if let Some(router_id) = filter.router_id {
        builder.push(" AND router_id = ").push_bind(router_id);
    }
    if let Some(account_id) = filter.account_id {
        builder.push(" AND account_id = ").push_bind(account_id);
    }
    if let Some(voucher_id) = filter.voucher_id {
        builder.push(" AND voucher_id = ").push_bind(voucher_id);
    }
    builder
        .push(" ORDER BY id DESC LIMIT ")
        .push_bind(i64::from(filter.limit))
        .push(" OFFSET ")
        .push_bind(i64::from(filter.offset));
    Ok(builder
        .build_query_as::<DisconnectRecord>()
        .fetch_all(pool)
        .await?)
}

pub async fn list_due(
    pool: &MySqlPool,
    limit: u32,
) -> Result<Vec<DisconnectRecord>, DisconnectError> {
    let mut builder = QueryBuilder::new(format!(
        "SELECT {COLS} FROM tbl_disconnects \
         WHERE status = 'pending' AND (next_attempt_at IS NULL OR next_attempt_at <= NOW()) \
         ORDER BY next_attempt_at ASC, id ASC LIMIT "
    ));
    builder.push_bind(i64::from(limit.clamp(1, 500)));
    Ok(builder
        .build_query_as::<DisconnectRecord>()
        .fetch_all(pool)
        .await?)
}

pub async fn mark_success(pool: &MySqlPool, id: u64) -> Result<(), DisconnectError> {
    sqlx::query(
        "UPDATE tbl_disconnects SET status = 'success', attempts = attempts + 1, \
         last_error = NULL, completed_at = NOW() WHERE id = ?",
    )
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Record a failed attempt with bounded exponential backoff. After
/// `max_attempts` the operation becomes `failed` (preserved, never deleted).
pub async fn mark_retry(
    pool: &MySqlPool,
    id: u64,
    error: &str,
    backoff_secs: u64,
) -> Result<(), DisconnectError> {
    sqlx::query(
        "UPDATE tbl_disconnects SET \
            attempts = attempts + 1, \
            status = IF(attempts + 1 >= max_attempts, 'failed', 'pending'), \
            last_error = ?, \
            next_attempt_at = IF(attempts + 1 >= max_attempts, next_attempt_at, \
                                 DATE_ADD(NOW(), INTERVAL ? SECOND)), \
            completed_at = IF(attempts + 1 >= max_attempts, NOW(), completed_at) \
         WHERE id = ?",
    )
    .bind(error)
    .bind(backoff_secs)
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn mark_unsupported(
    pool: &MySqlPool,
    id: u64,
    reason: &str,
) -> Result<(), DisconnectError> {
    sqlx::query(
        "UPDATE tbl_disconnects SET status = 'unsupported', attempts = attempts + 1, \
         last_error = ?, completed_at = NOW() WHERE id = ?",
    )
    .bind(reason)
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn stats(pool: &MySqlPool) -> Result<DisconnectStats, DisconnectError> {
    let row = sqlx::query(
        "SELECT \
           CAST(COALESCE(SUM(status = 'pending'), 0) AS UNSIGNED) AS pending, \
           CAST(COALESCE(SUM(status = 'success'), 0) AS UNSIGNED) AS success, \
           CAST(COALESCE(SUM(status = 'failed'), 0) AS UNSIGNED) AS failed, \
           CAST(COALESCE(SUM(status = 'unsupported'), 0) AS UNSIGNED) AS unsupported \
         FROM tbl_disconnects",
    )
    .fetch_one(pool)
    .await?;
    Ok(DisconnectStats {
        pending: row.try_get("pending")?,
        success: row.try_get("success")?,
        failed: row.try_get("failed")?,
        unsupported: row.try_get("unsupported")?,
    })
}

/// Reset a failed/unsupported disconnect back to pending for a manual retry.
pub async fn requeue(pool: &MySqlPool, id: u64) -> Result<bool, DisconnectError> {
    let result = sqlx::query(
        "UPDATE tbl_disconnects SET status = 'pending', attempts = 0, last_error = NULL, \
         next_attempt_at = NOW(), completed_at = NULL \
         WHERE id = ? AND status IN ('failed', 'unsupported')",
    )
    .bind(id)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}
