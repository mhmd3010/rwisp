use std::future::Future;

use axum::Json;
use axum::extract::{Path, State};
use serde_json::{Map, Value, json};

use rwisp_server::routers;
use rwisp_server::rwispros::{
    HotspotActive, HotspotHost, HotspotIpBinding, HotspotProfile, HotspotServer, HotspotUser,
    RouterClient, RouterOsError,
};

use super::common::{encryption_key, load_router, parse_id, with_router_timeout};
use crate::api_error::ApiError;
use crate::state::AppState;

pub async fn servers(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let id = parse_id(&id)?;
    let value = with_router_timeout(
        &state,
        endpoint(&state, id, "servers", |client| async move {
            client.get_hotspot_servers().await
        }),
    )
    .await?;
    Ok(Json(value))
}

pub async fn profiles(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let id = parse_id(&id)?;
    let value = with_router_timeout(
        &state,
        endpoint(&state, id, "profiles", |client| async move {
            client.get_hotspot_profiles().await
        }),
    )
    .await?;
    Ok(Json(value))
}

pub async fn users(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let id = parse_id(&id)?;
    let value = with_router_timeout(
        &state,
        endpoint(&state, id, "users", |client| async move {
            client.get_hotspot_users().await
        }),
    )
    .await?;
    Ok(Json(value))
}

pub async fn active(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let id = parse_id(&id)?;
    let value = with_router_timeout(
        &state,
        endpoint(&state, id, "active", |client| async move {
            client.get_hotspot_active().await
        }),
    )
    .await?;
    Ok(Json(value))
}

pub async fn ip_bindings(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let id = parse_id(&id)?;
    let value = with_router_timeout(
        &state,
        endpoint(&state, id, "ip_bindings", |client| async move {
            client.get_hotspot_ip_bindings().await
        }),
    )
    .await?;
    Ok(Json(value))
}

pub async fn hosts(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let id = parse_id(&id)?;
    let value = with_router_timeout(
        &state,
        endpoint(&state, id, "hosts", |client| async move {
            client.get_hotspot_hosts().await
        }),
    )
    .await?;
    Ok(Json(value))
}

async fn endpoint<T, F, Fut>(
    state: &AppState,
    id: u64,
    collection: &str,
    fetch: F,
) -> Result<Value, ApiError>
where
    T: HotspotEntry,
    F: FnOnce(RouterClient) -> Fut,
    Fut: Future<Output = Result<Vec<T>, RouterOsError>>,
{
    let key = encryption_key(state)?;
    let record = load_router(state, id).await?;
    let client = routers::connect_router(&state.db, id, key).await?;
    let items = fetch(client).await?;
    let values: Vec<Value> = items.iter().map(HotspotEntry::to_json).collect();

    let mut object = Map::new();
    object.insert("id".to_string(), json!(record.id));
    object.insert("name".to_string(), json!(record.name));
    object.insert("host".to_string(), json!(record.host));
    object.insert("count".to_string(), json!(values.len()));
    object.insert(collection.to_string(), Value::Array(values));
    Ok(Value::Object(object))
}

trait HotspotEntry {
    fn to_json(&self) -> Value;
}

impl HotspotEntry for HotspotServer {
    fn to_json(&self) -> Value {
        json!({
            "name": self.name,
            "interface": self.interface,
            "address_pool": self.address_pool,
            "profile": self.profile,
            "idle_timeout": self.idle_timeout,
            "keepalive_timeout": self.keepalive_timeout,
            "addresses_per_mac": self.addresses_per_mac,
            "disabled": self.disabled,
            "comment": self.comment,
        })
    }
}

impl HotspotEntry for HotspotProfile {
    fn to_json(&self) -> Value {
        json!({
            "name": self.name,
            "hotspot_address": self.hotspot_address,
            "dns_name": self.dns_name,
            "html_directory": self.html_directory,
            "login_by": self.login_by,
            "use_radius": self.use_radius,
            "shared_users": self.shared_users,
            "rate_limit": self.rate_limit,
            "http_cookie_lifetime": self.http_cookie_lifetime,
        })
    }
}

impl HotspotEntry for HotspotUser {
    fn to_json(&self) -> Value {
        json!({
            "server": self.server,
            "name": self.name,
            "profile": self.profile,
            "uptime": self.uptime,
            "bytes_in": self.bytes_in,
            "bytes_out": self.bytes_out,
            "packets_in": self.packets_in,
            "packets_out": self.packets_out,
            "limit_uptime": self.limit_uptime,
            "limit_bytes_in": self.limit_bytes_in,
            "limit_bytes_out": self.limit_bytes_out,
            "mac_address": self.mac_address,
            "comment": self.comment,
            "disabled": self.disabled,
        })
    }
}

impl HotspotEntry for HotspotActive {
    fn to_json(&self) -> Value {
        json!({
            "server": self.server,
            "user": self.user,
            "address": self.address,
            "mac_address": self.mac_address,
            "login_by": self.login_by,
            "uptime": self.uptime,
            "idle_time": self.idle_time,
            "session_time_left": self.session_time_left,
            "bytes_in": self.bytes_in,
            "bytes_out": self.bytes_out,
            "packets_in": self.packets_in,
            "packets_out": self.packets_out,
        })
    }
}

impl HotspotEntry for HotspotIpBinding {
    fn to_json(&self) -> Value {
        json!({
            "server": self.server,
            "address": self.address,
            "mac_address": self.mac_address,
            "to_address": self.to_address,
            "type": self.binding_type,
            "comment": self.comment,
            "disabled": self.disabled,
        })
    }
}

impl HotspotEntry for HotspotHost {
    fn to_json(&self) -> Value {
        json!({
            "server": self.server,
            "address": self.address,
            "to_address": self.to_address,
            "mac_address": self.mac_address,
            "uptime": self.uptime,
            "idle_time": self.idle_time,
            "bytes_in": self.bytes_in,
            "bytes_out": self.bytes_out,
            "packets_in": self.packets_in,
            "packets_out": self.packets_out,
            "authorized": self.authorized,
            "bypassed": self.bypassed,
        })
    }
}
