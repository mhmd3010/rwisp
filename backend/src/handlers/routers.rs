use std::time::Duration;

use axum::Extension;
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use serde_json::Value as JsonValue;
use serde_json::{Value, json};

use rwisp_server::routers::{self, NewRouter, RouterRecord, RouterUpdate};
use rwisp_server::rwispros::Transport;

use super::common::{encryption_key, load_router, parse_id, with_router_timeout};
use crate::admin::{self, AdminIdentity, Role};
use crate::api_error::ApiError;
use crate::state::AppState;

pub async fn list(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let records = routers::list_routers(&state.db, true).await?;
    let routers: Vec<Value> = records.iter().map(router_json).collect();
    Ok(Json(json!({ "routers": routers })))
}

pub async fn detail(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let id = parse_id(&id)?;
    let record = routers::get_router(&state.db, id)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("router {id} was not found")))?;

    Ok(Json(json!({ "router": router_json(&record) })))
}

pub async fn status(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let id = parse_id(&id)?;
    let value = with_router_timeout(&state, monitor_status(&state, id)).await?;
    Ok(Json(value))
}

pub async fn resources(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let id = parse_id(&id)?;
    let value = with_router_timeout(&state, monitor_resources(&state, id)).await?;
    Ok(Json(value))
}

async fn monitor_status(state: &AppState, id: u64) -> Result<Value, ApiError> {
    let key = encryption_key(state)?;
    let record = load_router(state, id).await?;
    let client = routers::connect_router(&state.db, id, key).await?;
    let identity = client.get_identity().await?;

    Ok(json!({
        "id": record.id,
        "name": record.name,
        "host": record.host,
        "online": true,
        "identity": {
            "name": identity.name,
            "revision": identity.revision,
        }
    }))
}

async fn monitor_resources(state: &AppState, id: u64) -> Result<Value, ApiError> {
    let key = encryption_key(state)?;
    let record = load_router(state, id).await?;
    let client = routers::connect_router(&state.db, id, key).await?;
    let resources = client.get_resources().await?;

    Ok(json!({
        "id": record.id,
        "name": record.name,
        "resources": {
            "uptime": resources.uptime,
            "version": resources.version,
            "board_name": resources.board_name,
            "architecture_name": resources.architecture_name,
            "cpu": resources.cpu,
            "cpu_count": resources.cpu_count,
            "cpu_frequency": resources.cpu_frequency,
            "cpu_load": resources.cpu_load,
            "free_memory": resources.free_memory,
            "total_memory": resources.total_memory,
            "free_hdd_space": resources.free_hdd_space,
            "total_hdd_space": resources.total_hdd_space,
        }
    }))
}

fn router_json(record: &RouterRecord) -> Value {
    json!({
        "id": record.id,
        "name": record.name,
        "host": record.host,
        "api_port": record.api_port,
        "transport": record.transport.as_str(),
        "timeout_secs": record.timeout.as_secs(),
        "radius_nas_ip": record.radius_nas_ip,
        "nas_identifier": record.nas_identifier,
        "coa_address": record.coa_address,
        "coa_port": record.coa_port,
        "auth_reachable": record.auth_reachable,
        "disconnect_reachable": record.disconnect_reachable,
        "has_coa_secret": record.has_coa_secret,
        "has_nas_secret": record.has_nas_secret,
        "enabled": record.enabled,
        "created_at": record.created_at,
        "updated_at": record.updated_at,
    })
}

pub async fn create(
    State(state): State<AppState>,
    Extension(identity): Extension<AdminIdentity>,
    Json(body): Json<JsonValue>,
) -> Result<(StatusCode, Json<JsonValue>), ApiError> {
    identity.require(Role::Admin)?;
    let key = encryption_key_or_error(&state)?;
    let input = RouterInput::from_json(&body)?;

    let password = input
        .password
        .clone()
        .ok_or_else(|| ApiError::bad_request("password is required"))?;
    let mut new_router = NewRouter::new(&input.name, &input.host, &input.username, password)
        .with_port(input.api_port)
        .with_transport(input.transport)
        .with_timeout(Duration::from_secs(input.timeout_secs))
        .enabled(input.enabled);
    if let Some(nas_ip) = &input.radius_nas_ip {
        new_router = new_router.with_radius_nas_ip(nas_ip.clone());
    }
    if let Some(nas_id) = &input.nas_identifier {
        new_router = new_router.with_nas_identifier(nas_id.clone());
    }
    if let Some(coa) = &input.coa_address {
        new_router = new_router.with_coa(coa.clone(), input.coa_port.unwrap_or(3799));
    }
    new_router = new_router.with_disconnect_reachable(input.disconnect_reachable);

    let id = routers::create_router(&state.db, &new_router, key).await?;
    if let Some(secret) = &input.nas_secret {
        let _ = routers::set_nas_secret(&state.db, id, key, Some(secret)).await;
    }
    if let Some(secret) = &input.coa_secret {
        let _ = routers::set_coa_secret(&state.db, id, key, Some(secret)).await;
    }

    admin::audit(
        &state.db,
        &identity.actor,
        identity.role,
        "router.create",
        Some("router"),
        Some(&id.to_string()),
        Some(&input.name),
    )
    .await;
    let record = routers::get_router(&state.db, id)
        .await?
        .ok_or_else(ApiError::internal)?;
    Ok((
        StatusCode::CREATED,
        Json(json!({ "router": router_json(&record) })),
    ))
}

pub async fn update(
    State(state): State<AppState>,
    Extension(identity): Extension<AdminIdentity>,
    Path(id): Path<String>,
    Json(body): Json<JsonValue>,
) -> Result<Json<JsonValue>, ApiError> {
    identity.require(Role::Admin)?;
    let id = parse_id(&id)?;
    let key = encryption_key_or_error(&state)?;
    let existing = routers::get_router(&state.db, id)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("router {id} was not found")))?;

    let input = RouterInput::from_json(&body)?;
    let mut update = RouterUpdate::from_record(&existing);
    update.name = input.name;
    update.host = input.host;
    update.api_port = input.api_port;
    update.username = input.username;
    update.transport = input.transport;
    update.timeout = Duration::from_secs(input.timeout_secs);
    update.radius_nas_ip = input.radius_nas_ip;
    update.nas_identifier = input.nas_identifier;
    update.coa_address = input.coa_address;
    update.coa_port = input.coa_port;
    update.auth_reachable = input.auth_reachable;
    update.disconnect_reachable = input.disconnect_reachable;
    update.enabled = input.enabled;
    if let Some(password) = input.password {
        update = update.with_password(password);
    }

    if !routers::update_router(&state.db, id, &update, key).await? {
        return Err(ApiError::not_found(format!("router {id} was not found")));
    }
    if let Some(secret) = &input.nas_secret {
        routers::set_nas_secret(&state.db, id, key, Some(secret)).await?;
    }
    if let Some(secret) = &input.coa_secret {
        routers::set_coa_secret(&state.db, id, key, Some(secret)).await?;
    }

    admin::audit(
        &state.db,
        &identity.actor,
        identity.role,
        "router.update",
        Some("router"),
        Some(&id.to_string()),
        None,
    )
    .await;
    let record = routers::get_router(&state.db, id)
        .await?
        .ok_or_else(ApiError::internal)?;
    Ok(Json(json!({ "router": router_json(&record) })))
}

pub async fn delete(
    State(state): State<AppState>,
    Extension(identity): Extension<AdminIdentity>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    identity.require(Role::Admin)?;
    let id = parse_id(&id)?;
    if !routers::delete_router(&state.db, id).await? {
        return Err(ApiError::not_found(format!("router {id} was not found")));
    }
    admin::audit(
        &state.db,
        &identity.actor,
        identity.role,
        "router.delete",
        Some("router"),
        Some(&id.to_string()),
        None,
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn set_secrets(
    State(state): State<AppState>,
    Extension(identity): Extension<AdminIdentity>,
    Path(id): Path<String>,
    Json(body): Json<JsonValue>,
) -> Result<Json<JsonValue>, ApiError> {
    identity.require(Role::Admin)?;
    let id = parse_id(&id)?;
    let key = encryption_key_or_error(&state)?;

    if routers::get_router(&state.db, id).await?.is_none() {
        return Err(ApiError::not_found(format!("router {id} was not found")));
    }

    if let Some(value) = body.get("nas_secret") {
        let secret = value.as_str().filter(|s| !s.is_empty());
        routers::set_nas_secret(&state.db, id, key, secret).await?;
    }
    if let Some(value) = body.get("coa_secret") {
        let secret = value.as_str().filter(|s| !s.is_empty());
        routers::set_coa_secret(&state.db, id, key, secret).await?;
    }

    admin::audit(
        &state.db,
        &identity.actor,
        identity.role,
        "router.set_secrets",
        Some("router"),
        Some(&id.to_string()),
        None,
    )
    .await;
    let record = routers::get_router(&state.db, id)
        .await?
        .ok_or_else(ApiError::internal)?;
    Ok(Json(json!({ "router": router_json(&record) })))
}

pub async fn status_history(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> Result<Json<JsonValue>, ApiError> {
    let id = parse_id(&id)?;
    if routers::get_router(&state.db, id).await?.is_none() {
        return Err(ApiError::not_found(format!("router {id} was not found")));
    }
    let limit = params
        .get("limit")
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(50);
    let history = routers::status_history(&state.db, id, limit).await?;
    let latest = routers::latest_status(&state.db, id).await?;
    Ok(Json(json!({
        "router_id": id,
        "latest": latest.map(|s| status_json(&s)),
        "history": history.iter().map(status_json).collect::<Vec<_>>(),
    })))
}

pub async fn provisioning_plan(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> Result<Json<JsonValue>, ApiError> {
    let id = parse_id(&id)?;
    let key = encryption_key_or_error(&state)?;
    let radius_address = params
        .get("radius_address")
        .map(String::as_str)
        .filter(|v| !v.trim().is_empty())
        .ok_or_else(|| ApiError::bad_request("radius_address is required"))?;

    let plan = routers::plan_provisioning(&state.db, id, key, radius_address).await?;
    Ok(Json(json!({
        "router_id": plan.router_id,
        "radius_address": plan.radius_address,
        "steps": plan.steps.iter().map(|s| json!({
            "action": s.action,
            "description": s.description,
            "command": s.command,
            "destructive": s.destructive,
        })).collect::<Vec<_>>(),
        "warnings": plan.warnings,
    })))
}

pub async fn provisioning_apply(
    State(state): State<AppState>,
    Extension(identity): Extension<AdminIdentity>,
    Path(id): Path<String>,
    Json(body): Json<JsonValue>,
) -> Result<Json<JsonValue>, ApiError> {
    identity.require(Role::Admin)?;
    let id = parse_id(&id)?;
    let key = encryption_key_or_error(&state)?;

    let radius_address = body
        .get("radius_address")
        .and_then(|v| v.as_str())
        .filter(|v| !v.trim().is_empty())
        .ok_or_else(|| ApiError::bad_request("radius_address is required"))?;
    let radius_secret = body
        .get("radius_secret")
        .and_then(|v| v.as_str())
        .filter(|v| !v.is_empty())
        .ok_or_else(|| ApiError::bad_request("radius_secret is required"))?;
    let approve = body
        .get("approve")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let plan = routers::plan_provisioning(&state.db, id, key, radius_address).await?;
    let report = routers::apply_provisioning(
        &state.db,
        id,
        key,
        &plan,
        approve,
        radius_address,
        radius_secret,
    )
    .await?;

    admin::audit(
        &state.db,
        &identity.actor,
        identity.role,
        "router.provision",
        Some("router"),
        Some(&id.to_string()),
        Some("applied"),
    )
    .await;
    Ok(Json(
        json!({ "applied": report.applied, "failed": report.failed }),
    ))
}

fn encryption_key_or_error(
    state: &AppState,
) -> Result<&rwisp_server::crypto::EncryptionKey, ApiError> {
    state.encryption_key.as_deref().ok_or_else(|| {
        ApiError::service_unavailable("router credential encryption is not configured")
    })
}

fn status_json(status: &routers::RouterStatus) -> JsonValue {
    json!({
        "router_id": status.router_id,
        "checked_at": status.checked_at,
        "reachable": status.reachable,
        "latency_ms": status.latency_ms,
        "version": status.version,
        "identity": status.identity,
        "error": status.error,
    })
}

struct RouterInput {
    name: String,
    host: String,
    api_port: u16,
    username: String,
    password: Option<String>,
    transport: Transport,
    timeout_secs: u64,
    radius_nas_ip: Option<String>,
    nas_identifier: Option<String>,
    coa_address: Option<String>,
    coa_port: Option<u16>,
    auth_reachable: bool,
    disconnect_reachable: bool,
    enabled: bool,
    nas_secret: Option<String>,
    coa_secret: Option<String>,
}

impl RouterInput {
    fn from_json(body: &JsonValue) -> Result<Self, ApiError> {
        let obj = body
            .as_object()
            .ok_or_else(|| ApiError::bad_request("expected a JSON object"))?;
        let get_str = |key: &str| obj.get(key).and_then(|v| v.as_str()).map(str::to_string);
        let get_u64 = |key: &str| obj.get(key).and_then(|v| v.as_u64());

        let name = get_str("name")
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
            .ok_or_else(|| ApiError::bad_request("name is required"))?;
        let host = get_str("host")
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
            .ok_or_else(|| ApiError::bad_request("host is required"))?;
        let username = get_str("username")
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
            .ok_or_else(|| ApiError::bad_request("username is required"))?;
        let password = get_str("password").filter(|v| !v.is_empty());

        let transport = get_str("transport")
            .as_deref()
            .and_then(Transport::parse)
            .unwrap_or(Transport::Tcp);
        let api_port = get_u64("api_port")
            .map(|v| v as u16)
            .unwrap_or_else(|| transport.default_port());

        Ok(Self {
            name,
            host,
            api_port,
            username,
            password,
            transport,
            timeout_secs: get_u64("timeout_secs").unwrap_or(10),
            radius_nas_ip: get_str("radius_nas_ip"),
            nas_identifier: get_str("nas_identifier"),
            coa_address: get_str("coa_address"),
            coa_port: get_u64("coa_port").map(|v| v as u16),
            auth_reachable: obj
                .get("auth_reachable")
                .and_then(|v| v.as_bool())
                .unwrap_or(true),
            disconnect_reachable: obj
                .get("disconnect_reachable")
                .and_then(|v| v.as_bool())
                .unwrap_or(true),
            enabled: obj.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true),
            nas_secret: get_str("nas_secret"),
            coa_secret: get_str("coa_secret"),
        })
    }
}
