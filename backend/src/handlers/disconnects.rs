use std::collections::HashMap;

use axum::Json;
use axum::extract::{Path, Query, State};
use serde_json::{Value, json};

use rwisp_server::disconnects::{self, DisconnectFilter, DisconnectRecord};
use rwisp_server::sessions::{self, SessionFilter, SessionRecord};

use super::common::parse_id;
use crate::api_error::ApiError;
use crate::state::AppState;

pub async fn list(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, ApiError> {
    let filter = DisconnectFilter::from_query(&params)?;
    let records = disconnects::list(&state.db, &filter).await?;
    let items: Vec<Value> = records.iter().map(disconnect_json).collect();
    Ok(Json(json!({
        "disconnects": items,
        "count": items.len(),
        "limit": filter.limit,
        "offset": filter.offset,
    })))
}

pub async fn detail(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let id = parse_id(&id)?;
    let record = disconnects::get(&state.db, id)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("disconnect {id} was not found")))?;
    Ok(Json(json!({ "disconnect": disconnect_json(&record) })))
}

/// Manual disconnect by session, account, voucher or router.
pub async fn create(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let reason = body
        .get("reason")
        .and_then(|v| v.as_str())
        .unwrap_or("manual")
        .to_string();

    let mut enqueued = 0u64;

    if let Some(session_id) = body.get("session_id").and_then(|v| v.as_u64()) {
        let session = sessions::get_session(&state.db, session_id)
            .await?
            .ok_or_else(|| ApiError::not_found(format!("session {session_id} was not found")))?;
        super::sessions::enqueue_for_session(&state, &session, &reason).await?;
        enqueued += 1;
    }

    if let Some(account_id) = body.get("account_id").and_then(|v| v.as_u64()) {
        enqueued += enqueue_matching(&state, "account_id", account_id, &reason).await?;
    }
    if let Some(voucher_id) = body.get("voucher_id").and_then(|v| v.as_u64()) {
        enqueued += enqueue_matching(&state, "voucher_id", voucher_id, &reason).await?;
    }
    if let Some(router_id) = body.get("router_id").and_then(|v| v.as_u64()) {
        enqueued += enqueue_router(&state, router_id, &reason).await?;
    }

    if enqueued == 0 {
        return Err(ApiError::bad_request(
            "provide session_id, account_id, voucher_id or router_id",
        ));
    }

    Ok(Json(json!({ "enqueued": enqueued })))
}

pub async fn retry(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let id = parse_id(&id)?;
    if disconnects::requeue(&state.db, id).await? {
        Ok(Json(json!({ "disconnect_id": id, "status": "pending" })))
    } else {
        Err(ApiError::conflict(
            "only failed or unsupported disconnects can be retried",
        ))
    }
}

async fn enqueue_matching(
    state: &AppState,
    column: &str,
    value: u64,
    reason: &str,
) -> Result<u64, ApiError> {
    let filter = SessionFilter {
        online_only: true,
        account_id: (column == "account_id").then_some(value),
        voucher_id: (column == "voucher_id").then_some(value),
        limit: 500,
        ..SessionFilter::default()
    };
    let sessions = sessions::list_sessions(&state.db, &filter).await?;
    let mut count = 0;
    for session in sessions {
        super::sessions::enqueue_for_session(state, &session, reason).await?;
        count += 1;
    }
    Ok(count)
}

async fn enqueue_router(state: &AppState, router_id: u64, reason: &str) -> Result<u64, ApiError> {
    let row = sqlx::query("SELECT radius_nas_ip, host FROM tbl_routers WHERE id = ?")
        .bind(router_id)
        .fetch_optional(&state.db)
        .await?;
    let Some(row) = row else {
        return Err(ApiError::not_found(format!(
            "router {router_id} was not found"
        )));
    };
    use sqlx::Row;
    let nas_ip: Option<String> = row.try_get("radius_nas_ip")?;
    let host: String = row.try_get("host")?;

    let sessions = sessions::list_sessions(
        &state.db,
        &SessionFilter {
            online_only: true,
            limit: 500,
            ..SessionFilter::default()
        },
    )
    .await?;

    let matches: Vec<SessionRecord> = sessions
        .into_iter()
        .filter(|s| s.nas_ip == host || nas_ip.as_deref() == Some(s.nas_ip.as_str()))
        .collect();

    let mut count = 0;
    for session in matches {
        super::sessions::enqueue_for_session(state, &session, reason).await?;
        count += 1;
    }
    Ok(count)
}

fn disconnect_json(record: &DisconnectRecord) -> Value {
    json!({
        "id": record.id,
        "username": record.username,
        "account_id": record.account_id,
        "voucher_id": record.voucher_id,
        "session_row_id": record.session_row_id,
        "router_id": record.router_id,
        "nas_ip": record.nas_ip,
        "acct_session_id": record.acct_session_id,
        "reason": record.reason,
        "method": record.method.as_str(),
        "status": record.status.as_str(),
        "attempts": record.attempts,
        "max_attempts": record.max_attempts,
        "last_error": record.last_error,
        "next_attempt_at": record.next_attempt_at,
        "created_at": record.created_at,
        "updated_at": record.updated_at,
        "completed_at": record.completed_at,
    })
}
