use sqlx::mysql::MySqlRow;
use sqlx::{FromRow, MySqlPool, QueryBuilder, Row};

use crate::sessions::error::SessionError;
use crate::sessions::model::{SessionFilter, SessionRecord, SessionStatus};

const SESSION_COLUMNS: &str = "id, username, acct_session_id, acct_unique_id, nas_ip, \
    nas_identifier, calling_station_id, framed_ip, account_id, voucher_id, status, session_time, \
    input_octets, output_octets, terminate_cause, CAST(started_at AS CHAR) AS started_at, \
    CAST(updated_at AS CHAR) AS updated_at, CAST(stopped_at AS CHAR) AS stopped_at, \
    CAST(created_at AS CHAR) AS created_at, \
    (status <> 'stop' AND updated_at IS NOT NULL \
     AND updated_at < DATE_SUB(NOW(), INTERVAL 900 SECOND)) AS is_stale";

impl FromRow<'_, MySqlRow> for SessionRecord {
    fn from_row(row: &MySqlRow) -> Result<Self, sqlx::Error> {
        let status_raw: String = row.try_get("status")?;
        let status = SessionStatus::parse(&status_raw).ok_or_else(|| {
            sqlx::Error::Decode(Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("unknown session status: {status_raw}"),
            )))
        })?;
        let session_time: u64 = row.try_get("session_time")?;
        let stale_raw: i64 = row.try_get("is_stale")?;

        Ok(Self {
            id: row.try_get("id")?,
            username: row.try_get("username")?,
            acct_session_id: row.try_get("acct_session_id")?,
            acct_unique_id: row.try_get("acct_unique_id")?,
            nas_ip: row.try_get("nas_ip")?,
            nas_identifier: row.try_get("nas_identifier")?,
            calling_station_id: row.try_get("calling_station_id")?,
            framed_ip: row.try_get("framed_ip")?,
            account_id: row.try_get("account_id")?,
            voucher_id: row.try_get("voucher_id")?,
            status,
            session_time,
            input_octets: row.try_get("input_octets")?,
            output_octets: row.try_get("output_octets")?,
            terminate_cause: row.try_get("terminate_cause")?,
            started_at: row.try_get("started_at")?,
            updated_at: row.try_get("updated_at")?,
            stopped_at: row.try_get("stopped_at")?,
            created_at: row.try_get("created_at")?,
            online: status != SessionStatus::Stop,
            stale: stale_raw != 0,
        })
    }
}

pub async fn list_sessions(
    pool: &MySqlPool,
    filter: &SessionFilter,
) -> Result<Vec<SessionRecord>, SessionError> {
    let mut builder = QueryBuilder::new(format!(
        "SELECT {SESSION_COLUMNS} FROM tbl_radius_sessions WHERE 1 = 1"
    ));

    if let Some(username) = &filter.username {
        builder
            .push(" AND username LIKE ")
            .push_bind(format!("%{username}%"));
    }
    if let Some(account_id) = filter.account_id {
        builder.push(" AND account_id = ").push_bind(account_id);
    }
    if let Some(voucher_id) = filter.voucher_id {
        builder.push(" AND voucher_id = ").push_bind(voucher_id);
    }
    if let Some(nas_ip) = &filter.nas_ip {
        builder.push(" AND nas_ip = ").push_bind(nas_ip);
    }
    if let Some(framed_ip) = &filter.framed_ip {
        builder.push(" AND framed_ip = ").push_bind(framed_ip);
    }
    if let Some(mac) = &filter.calling_station_id {
        builder.push(" AND calling_station_id = ").push_bind(mac);
    }
    if let Some(status) = filter.status {
        builder.push(" AND status = ").push_bind(status.as_str());
    } else if filter.online_only {
        builder.push(" AND status <> 'stop'");
    }
    builder
        .push(" ORDER BY COALESCE(updated_at, created_at) DESC, id DESC LIMIT ")
        .push_bind(i64::from(filter.limit))
        .push(" OFFSET ")
        .push_bind(i64::from(filter.offset));

    let rows = builder
        .build_query_as::<SessionRecord>()
        .fetch_all(pool)
        .await?;
    Ok(rows)
}

pub async fn get_session(pool: &MySqlPool, id: u64) -> Result<Option<SessionRecord>, SessionError> {
    let query = format!("SELECT {SESSION_COLUMNS} FROM tbl_radius_sessions WHERE id = ?");
    let session = sqlx::query_as::<_, SessionRecord>(sqlx::AssertSqlSafe(query))
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(session)
}

pub async fn session_history(
    pool: &MySqlPool,
    username: &str,
    limit: u32,
) -> Result<Vec<SessionRecord>, SessionError> {
    let mut builder = QueryBuilder::new(format!(
        "SELECT {SESSION_COLUMNS} FROM tbl_radius_sessions WHERE username = "
    ));
    builder
        .push_bind(username.to_string())
        .push(" ORDER BY id DESC LIMIT ")
        .push_bind(i64::from(limit.clamp(1, 1000)));
    let rows = builder
        .build_query_as::<SessionRecord>()
        .fetch_all(pool)
        .await?;
    Ok(rows)
}

/// Online sessions with no accounting update inside the stale window (likely
/// missed Stop records). Returned as online so they are never silently dropped.
pub async fn stale_sessions(pool: &MySqlPool) -> Result<Vec<SessionRecord>, SessionError> {
    let query = format!(
        "SELECT {SESSION_COLUMNS} FROM tbl_radius_sessions \
         WHERE status <> 'stop' AND updated_at IS NOT NULL \
           AND updated_at < DATE_SUB(NOW(), INTERVAL 900 SECOND) \
         ORDER BY updated_at ASC"
    );
    let rows = sqlx::query_as::<_, SessionRecord>(sqlx::AssertSqlSafe(query))
        .fetch_all(pool)
        .await?;
    Ok(rows)
}

/// Count concurrent open sessions for a username (used for race-safe device
/// limit admission control).
pub async fn open_count_for_username(
    pool: &MySqlPool,
    username: &str,
) -> Result<u64, SessionError> {
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM tbl_radius_sessions WHERE username = ? AND status <> 'stop'",
    )
    .bind(username)
    .fetch_one(pool)
    .await?;
    Ok(count.max(0) as u64)
}

#[derive(Debug, Clone, Default)]
pub struct SessionStats {
    pub online: u64,
    pub stale: u64,
    pub total_sessions: u64,
    pub total_input_octets: u64,
    pub total_output_octets: u64,
    pub total_session_time: u64,
}

pub async fn stats(pool: &MySqlPool) -> Result<SessionStats, SessionError> {
    let row = sqlx::query(
        "SELECT \
           CAST(COUNT(*) AS UNSIGNED) AS total_sessions, \
           CAST(COALESCE(SUM(status <> 'stop'), 0) AS UNSIGNED) AS online, \
           CAST(COALESCE(SUM(status <> 'stop' AND updated_at IS NOT NULL \
                     AND updated_at < DATE_SUB(NOW(), INTERVAL 900 SECOND)), 0) AS UNSIGNED) AS stale, \
           CAST(COALESCE(SUM(input_octets), 0) AS UNSIGNED) AS total_input_octets, \
           CAST(COALESCE(SUM(output_octets), 0) AS UNSIGNED) AS total_output_octets, \
           CAST(COALESCE(SUM(session_time), 0) AS UNSIGNED) AS total_session_time \
         FROM tbl_radius_sessions",
    )
    .fetch_one(pool)
    .await?;

    Ok(SessionStats {
        online: row.try_get("online")?,
        stale: row.try_get("stale")?,
        total_sessions: row.try_get("total_sessions")?,
        total_input_octets: row.try_get("total_input_octets")?,
        total_output_octets: row.try_get("total_output_octets")?,
        total_session_time: row.try_get("total_session_time")?,
    })
}
