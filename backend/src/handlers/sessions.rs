use std::collections::HashMap;

use axum::Extension;
use axum::Json;
use axum::extract::{Path, Query, State};
use serde_json::{Value, json};
use sqlx::Row;

use rwisp_server::disconnects::{self, DisconnectMethod, NewDisconnect};
use rwisp_server::routers;
use rwisp_server::sessions::{self, ReconcileReport, SessionFilter, SessionRecord};

use super::common::parse_id;
use crate::admin::{self, AdminIdentity};
use crate::api_error::ApiError;
use crate::state::AppState;

pub async fn list(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, ApiError> {
    let filter = SessionFilter::from_query(&params)?;
    let records = sessions::list_sessions(&state.db, &filter).await?;
    let sessions: Vec<Value> = records.iter().map(session_json).collect();
    Ok(Json(json!({
        "sessions": sessions,
        "count": sessions.len(),
        "limit": filter.limit,
        "offset": filter.offset,
    })))
}

pub async fn detail(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let id = parse_id(&id)?;
    let session = sessions::get_session(&state.db, id)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("session {id} was not found")))?;
    Ok(Json(json!({ "session": session_json(&session) })))
}

pub async fn history(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, ApiError> {
    let username = params
        .get("username")
        .map(|v| v.trim())
        .filter(|v| !v.is_empty())
        .ok_or_else(|| ApiError::bad_request("username is required"))?;
    let limit = params
        .get("limit")
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(100);
    let records = sessions::session_history(&state.db, username, limit).await?;
    let sessions: Vec<Value> = records.iter().map(session_json).collect();
    Ok(Json(
        json!({ "username": username, "sessions": sessions, "count": sessions.len() }),
    ))
}

pub async fn stats(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let stats = sessions::stats(&state.db).await?;
    Ok(Json(json!({
        "online": stats.online,
        "stale": stats.stale,
        "total_sessions": stats.total_sessions,
        "total_input_octets": stats.total_input_octets,
        "total_output_octets": stats.total_output_octets,
        "total_octets": stats.total_input_octets.saturating_add(stats.total_output_octets),
        "total_session_time": stats.total_session_time,
    })))
}

pub async fn stale(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let records = sessions::stale_sessions(&state.db).await?;
    let sessions: Vec<Value> = records.iter().map(session_json).collect();
    Ok(Json(
        json!({ "stale_sessions": sessions, "count": sessions.len() }),
    ))
}

/// Compare RWISP online sessions with RouterOS active sessions for one router.
pub async fn reconcile(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, ApiError> {
    let router_id = params
        .get("router_id")
        .and_then(|v| v.trim().parse::<u64>().ok())
        .ok_or_else(|| ApiError::bad_request("router_id is required"))?;
    let key = state.encryption_key.as_deref().ok_or_else(|| {
        ApiError::service_unavailable("router credential encryption is not configured")
    })?;

    let client = routers::connect_router(&state.db, router_id, key).await?;
    let active = client
        .get_hotspot_active_raw()
        .await
        .map_err(ApiError::from)?;

    let routeros_users: Vec<String> = active
        .iter()
        .filter_map(|reply| reply.attributes.get("user").cloned().flatten())
        .collect();

    let filter = SessionFilter {
        online_only: true,
        limit: 1000,
        ..SessionFilter::default()
    };
    let online = sessions::list_sessions(&state.db, &filter).await?;
    let rwisp_users: Vec<String> = online.iter().map(|s| s.username.clone()).collect();

    let report = ReconcileReport {
        rwisp_online: rwisp_users.len(),
        routeros_active: routeros_users.len(),
        missing_stop: rwisp_users
            .iter()
            .filter(|u| !routeros_users.contains(u))
            .cloned()
            .collect(),
        unknown_on_router: routeros_users
            .iter()
            .filter(|u| !rwisp_users.contains(u))
            .cloned()
            .collect(),
    };

    Ok(Json(json!({
        "router_id": router_id,
        "rwisp_online": report.rwisp_online,
        "routeros_active": report.routeros_active,
        "missing_stop": report.missing_stop,
        "unknown_on_router": report.unknown_on_router,
    })))
}

pub async fn disconnect(
    State(state): State<AppState>,
    Extension(identity): Extension<AdminIdentity>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let id = parse_id(&id)?;
    let session = sessions::get_session(&state.db, id)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("session {id} was not found")))?;

    let router_id: Option<u64> = sqlx::query_scalar(
        "SELECT id FROM tbl_routers WHERE radius_nas_ip = ? OR host = ? LIMIT 1",
    )
    .bind(&session.nas_ip)
    .bind(&session.nas_ip)
    .fetch_optional(&state.db)
    .await?;

    // Synchronous RouterOS removal when the session's router is resolvable and
    // the credential key is available; otherwise fall back to the retry queue.
    if let (Some(router_id), Some(key)) = (router_id, state.encryption_key.as_deref()) {
        let outcome = disconnects::routeros_disconnect_now(
            &state.db,
            router_id,
            &session.username,
            session.calling_station_id.as_deref(),
            session.framed_ip.as_deref(),
            key,
        )
        .await;

        match outcome {
            disconnects::DisconnectOutcome::Success => {
                admin::audit(
                    &state.db,
                    &identity.actor,
                    identity.role,
                    "session.disconnect",
                    Some("session"),
                    Some(&session.id.to_string()),
                    Some("routeros: disconnected"),
                )
                .await;
                return Ok(Json(json!({
                    "session_id": session.id,
                    "username": session.username,
                    "disconnected": true,
                    "method": "routeros",
                    "router_id": router_id,
                })));
            }
            disconnects::DisconnectOutcome::Retry { error } => {
                let disconnect_id = enqueue_for_session(&state, &session, "manual").await?;
                admin::audit(
                    &state.db,
                    &identity.actor,
                    identity.role,
                    "session.disconnect",
                    Some("session"),
                    Some(&session.id.to_string()),
                    Some("routeros failed; queued for retry"),
                )
                .await;
                return Ok(Json(json!({
                    "session_id": session.id,
                    "username": session.username,
                    "disconnected": false,
                    "queued": true,
                    "disconnect_id": disconnect_id,
                    "method": "routeros",
                    "error": error,
                })));
            }
            disconnects::DisconnectOutcome::Unsupported { reason } => {
                let disconnect_id = enqueue_for_session(&state, &session, "manual").await?;
                admin::audit(
                    &state.db,
                    &identity.actor,
                    identity.role,
                    "session.disconnect",
                    Some("session"),
                    Some(&session.id.to_string()),
                    Some("routeros unsupported; queued"),
                )
                .await;
                return Ok(Json(json!({
                    "session_id": session.id,
                    "username": session.username,
                    "disconnected": false,
                    "queued": true,
                    "disconnect_id": disconnect_id,
                    "method": "routeros",
                    "error": reason,
                })));
            }
        }
    }

    // No resolvable router (or no credential key): queue for background retries.
    let disconnect_id = enqueue_for_session(&state, &session, "manual").await?;
    admin::audit(
        &state.db,
        &identity.actor,
        identity.role,
        "session.disconnect",
        Some("session"),
        Some(&session.id.to_string()),
        Some("no router/credential key; queued"),
    )
    .await;
    Ok(Json(json!({
        "session_id": session.id,
        "username": session.username,
        "disconnected": false,
        "queued": true,
        "disconnect_id": disconnect_id,
    })))
}

pub async fn enqueue_for_session(
    state: &AppState,
    session: &SessionRecord,
    reason: &str,
) -> Result<u64, ApiError> {
    let router_id: Option<u64> = sqlx::query_scalar(
        "SELECT id FROM tbl_routers WHERE radius_nas_ip = ? OR host = ? LIMIT 1",
    )
    .bind(&session.nas_ip)
    .bind(&session.nas_ip)
    .fetch_optional(&state.db)
    .await?;

    let method = match router_id {
        Some(router_id) => {
            let row = sqlx::query(
                "SELECT coa_address, disconnect_reachable FROM tbl_routers WHERE id = ?",
            )
            .bind(router_id)
            .fetch_one(&state.db)
            .await?;
            let coa_address: Option<String> = row.try_get("coa_address")?;
            let reachable: i8 = row.try_get("disconnect_reachable")?;
            if coa_address.map(|a| !a.trim().is_empty()).unwrap_or(false) && reachable != 0 {
                DisconnectMethod::Coa
            } else {
                DisconnectMethod::RouterOs
            }
        }
        None => DisconnectMethod::None,
    };

    let request = NewDisconnect {
        username: session.username.clone(),
        account_id: session.account_id,
        voucher_id: session.voucher_id,
        session_row_id: Some(session.id),
        router_id,
        nas_ip: Some(session.nas_ip.clone()),
        acct_session_id: Some(session.acct_session_id.clone()),
        reason: reason.to_string(),
        method,
        max_attempts: 5,
    };

    Ok(disconnects::enqueue(&state.db, &request).await?)
}

pub fn session_json(session: &SessionRecord) -> Value {
    json!({
        "id": session.id,
        "username": session.username,
        "acct_session_id": session.acct_session_id,
        "nas_ip": session.nas_ip,
        "nas_identifier": session.nas_identifier,
        "calling_station_id": session.calling_station_id,
        "framed_ip": session.framed_ip,
        "account_id": session.account_id,
        "voucher_id": session.voucher_id,
        "status": session.status.as_str(),
        "online": session.online,
        "stale": session.stale,
        "session_time": session.session_time,
        "input_octets": session.input_octets,
        "output_octets": session.output_octets,
        "total_octets": session.total_octets(),
        "terminate_cause": session.terminate_cause,
        "started_at": session.started_at,
        "updated_at": session.updated_at,
        "stopped_at": session.stopped_at,
        "created_at": session.created_at,
    })
}
