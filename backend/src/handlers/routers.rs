use axum::Json;
use axum::extract::{Path, State};
use serde_json::{Value, json};

use rwisp_server::routers::{self, RouterRecord};

use super::common::{encryption_key, load_router, parse_id, with_router_timeout};
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
        "enabled": record.enabled,
        "created_at": record.created_at,
        "updated_at": record.updated_at,
    })
}
