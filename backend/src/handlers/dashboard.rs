use std::collections::HashMap;

use axum::Json;
use axum::extract::{Query, State};
use serde_json::{Value, json};
use sqlx::Row;

use crate::api_error::ApiError;
use crate::state::AppState;

pub async fn dashboard(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let routers = scalar(&state, "SELECT COUNT(*) FROM tbl_routers").await?;
    let routers_enabled =
        scalar(&state, "SELECT COUNT(*) FROM tbl_routers WHERE enabled = 1").await?;

    let accounts_total = scalar(&state, "SELECT COUNT(*) FROM tbl_accounts").await?;

    let account_status_rows = sqlx::query(
        "SELECT status, CAST(COUNT(*) AS UNSIGNED) AS c FROM tbl_accounts GROUP BY status",
    )
    .fetch_all(&state.db)
    .await?;
    let mut accounts_by_status = serde_json::Map::new();
    for row in account_status_rows {
        accounts_by_status.insert(
            row.try_get::<String, _>("status")?,
            json!(row.try_get::<u64, _>("c")?),
        );
    }

    let vouchers_total = scalar(&state, "SELECT COUNT(*) FROM tbl_vouchers").await?;
    let voucher_status_rows = sqlx::query(
        "SELECT status, CAST(COUNT(*) AS UNSIGNED) AS c FROM tbl_vouchers GROUP BY status",
    )
    .fetch_all(&state.db)
    .await?;
    let mut vouchers_by_status = serde_json::Map::new();
    for row in voucher_status_rows {
        vouchers_by_status.insert(
            row.try_get::<String, _>("status")?,
            json!(row.try_get::<u64, _>("c")?),
        );
    }

    let session_stats = rwisp_server::sessions::stats(&state.db).await?;
    let disconnect_stats = rwisp_server::disconnects::stats(&state.db).await?;

    let expiring: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM tbl_accounts WHERE status = 'active' \
         AND expires_at IS NOT NULL AND expires_at BETWEEN NOW() AND DATE_ADD(NOW(), INTERVAL 7 DAY)",
    )
    .fetch_one(&state.db)
    .await?;

    let recent_rows = sqlx::query(
        "SELECT actor, action, target_type, target_id, CAST(created_at AS CHAR) AS created_at \
         FROM tbl_admin_audit ORDER BY id DESC LIMIT 5",
    )
    .fetch_all(&state.db)
    .await?;
    let mut recent = Vec::with_capacity(recent_rows.len());
    for row in recent_rows {
        recent.push(json!({
            "actor": row.try_get::<String, _>("actor")?,
            "action": row.try_get::<String, _>("action")?,
            "target_type": row.try_get::<Option<String>, _>("target_type")?,
            "target_id": row.try_get::<Option<String>, _>("target_id")?,
            "created_at": row.try_get::<Option<String>, _>("created_at")?,
        }));
    }

    Ok(Json(json!({
        "routers": {
            "total": routers,
            "enabled": routers_enabled,
        },
        "accounts": {
            "total": accounts_total,
            "by_status": accounts_by_status,
        },
        "vouchers": {
            "total": vouchers_total,
            "by_status": vouchers_by_status,
        },
        "sessions": {
            "online": session_stats.online,
            "stale": session_stats.stale,
            "total": session_stats.total_sessions,
            "total_octets": session_stats.total_input_octets.saturating_add(session_stats.total_output_octets),
            "total_session_time": session_stats.total_session_time,
        },
        "subscriptions_expiring_7d": expiring,
        "disconnects": {
            "pending": disconnect_stats.pending,
            "success": disconnect_stats.success,
            "failed": disconnect_stats.failed,
            "unsupported": disconnect_stats.unsupported,
        },
        "recent_activity": recent,
    })))
}

pub async fn traffic(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, ApiError> {
    let group_by = params
        .get("group_by")
        .map(String::as_str)
        .unwrap_or("router");
    let from = params.get("from").map(String::as_str);
    let to = params.get("to").map(String::as_str);

    let query = match group_by {
        "router" => {
            "SELECT COALESCE(r.name, s.nas_ip) AS label, \
                    CAST(COALESCE(SUM(s.input_octets), 0) AS UNSIGNED) AS input_octets, \
                    CAST(COALESCE(SUM(s.output_octets), 0) AS UNSIGNED) AS output_octets, \
                    CAST(COUNT(*) AS UNSIGNED) AS sessions \
             FROM tbl_radius_sessions s \
             LEFT JOIN tbl_routers r ON r.radius_nas_ip = s.nas_ip OR r.host = s.nas_ip \
             WHERE (? IS NULL OR s.started_at >= CAST(? AS DATETIME)) \
               AND (? IS NULL OR s.started_at <= CAST(? AS DATETIME)) \
             GROUP BY label ORDER BY input_octets + output_octets DESC LIMIT 500"
        }
        "account" => {
            "SELECT COALESCE(a.username, s.username) AS label, \
                    CAST(COALESCE(SUM(s.input_octets), 0) AS UNSIGNED) AS input_octets, \
                    CAST(COALESCE(SUM(s.output_octets), 0) AS UNSIGNED) AS output_octets, \
                    CAST(COUNT(*) AS UNSIGNED) AS sessions \
             FROM tbl_radius_sessions s \
             LEFT JOIN tbl_accounts a ON a.id = s.account_id \
             WHERE s.account_id IS NOT NULL \
               AND (? IS NULL OR s.started_at >= CAST(? AS DATETIME)) \
               AND (? IS NULL OR s.started_at <= CAST(? AS DATETIME)) \
             GROUP BY label ORDER BY input_octets + output_octets DESC LIMIT 500"
        }
        "voucher" => {
            "SELECT COALESCE(v.code, s.username) AS label, \
                    CAST(COALESCE(SUM(s.input_octets), 0) AS UNSIGNED) AS input_octets, \
                    CAST(COALESCE(SUM(s.output_octets), 0) AS UNSIGNED) AS output_octets, \
                    CAST(COUNT(*) AS UNSIGNED) AS sessions \
             FROM tbl_radius_sessions s \
             LEFT JOIN tbl_vouchers v ON v.id = s.voucher_id \
             WHERE s.voucher_id IS NOT NULL \
               AND (? IS NULL OR s.started_at >= CAST(? AS DATETIME)) \
               AND (? IS NULL OR s.started_at <= CAST(? AS DATETIME)) \
             GROUP BY label ORDER BY input_octets + output_octets DESC LIMIT 500"
        }
        other => {
            return Err(ApiError::bad_request(format!(
                "unsupported group_by `{other}` (use router, account or voucher)"
            )));
        }
    };

    let rows = sqlx::query(query)
        .bind(from)
        .bind(from)
        .bind(to)
        .bind(to)
        .fetch_all(&state.db)
        .await?;

    let mut items = Vec::with_capacity(rows.len());
    for row in rows {
        let input: u64 = row.try_get("input_octets")?;
        let output: u64 = row.try_get("output_octets")?;
        items.push(json!({
            "label": row.try_get::<String, _>("label")?,
            "input_octets": input,
            "output_octets": output,
            "total_octets": input.saturating_add(output),
            "sessions": row.try_get::<u64, _>("sessions")?,
        }));
    }

    Ok(Json(json!({ "group_by": group_by, "traffic": items })))
}

pub async fn sessions_report(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, ApiError> {
    let from = params.get("from").map(String::as_str);
    let to = params.get("to").map(String::as_str);

    let row = sqlx::query(
        "SELECT CAST(COUNT(*) AS UNSIGNED) AS sessions, \
                CAST(COALESCE(SUM(input_octets), 0) AS UNSIGNED) AS input_octets, \
                CAST(COALESCE(SUM(output_octets), 0) AS UNSIGNED) AS output_octets, \
                CAST(COALESCE(SUM(session_time), 0) AS UNSIGNED) AS session_time \
         FROM tbl_radius_sessions \
         WHERE (? IS NULL OR started_at >= CAST(? AS DATETIME)) \
           AND (? IS NULL OR started_at <= CAST(? AS DATETIME))",
    )
    .bind(from)
    .bind(from)
    .bind(to)
    .bind(to)
    .fetch_one(&state.db)
    .await?;

    Ok(Json(json!({
        "from": from,
        "to": to,
        "sessions": row.try_get::<u64, _>("sessions")?,
        "input_octets": row.try_get::<u64, _>("input_octets")?,
        "output_octets": row.try_get::<u64, _>("output_octets")?,
        "session_time": row.try_get::<u64, _>("session_time")?,
    })))
}

async fn scalar(state: &AppState, query: &'static str) -> Result<i64, ApiError> {
    let value: i64 = sqlx::query_scalar(query).fetch_one(&state.db).await?;
    Ok(value)
}
