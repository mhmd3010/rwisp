use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde_json::{Value, json};

use rwisp_server::packages::{self, Package, PackageInput};

use super::common::parse_id;
use crate::api_error::ApiError;
use crate::state::AppState;

pub async fn list(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let records = packages::list_packages(&state.db, true).await?;
    let packages: Vec<Value> = records.iter().map(package_json).collect();
    Ok(Json(json!({ "packages": packages })))
}

pub async fn create(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let input = PackageInput::from_json(&body)?;
    let id = packages::create_package(&state.db, &input).await?;
    let package = load(&state, id).await?;

    Ok((
        StatusCode::CREATED,
        Json(json!({ "package": package_json(&package) })),
    ))
}

pub async fn detail(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let id = parse_id(&id)?;
    let package = packages::get_package(&state.db, id)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("package {id} was not found")))?;

    Ok(Json(json!({ "package": package_json(&package) })))
}

pub async fn update(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let id = parse_id(&id)?;
    let input = PackageInput::from_json(&body)?;

    if !packages::update_package(&state.db, id, &input).await? {
        return Err(ApiError::not_found(format!("package {id} was not found")));
    }

    let package = load(&state, id).await?;
    Ok(Json(json!({ "package": package_json(&package) })))
}

pub async fn delete(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let id = parse_id(&id)?;

    if !packages::delete_package(&state.db, id).await? {
        return Err(ApiError::not_found(format!("package {id} was not found")));
    }

    Ok(StatusCode::NO_CONTENT)
}

async fn load(state: &AppState, id: u64) -> Result<Package, ApiError> {
    packages::get_package(&state.db, id)
        .await?
        .ok_or_else(ApiError::internal)
}

fn package_json(package: &Package) -> Value {
    json!({
        "id": package.id,
        "name": package.name,
        "description": package.description,
        "price_cents": package.price_cents,
        "enabled": package.enabled,
        "validity_secs": package.validity_secs,
        "usage_limit_secs": package.usage_limit_secs,
        "data_limit_bytes": package.data_limit_bytes,
        "upload_rate_bps": package.upload_rate_bps,
        "download_rate_bps": package.download_rate_bps,
        "device_limit": package.device_limit,
        "created_at": package.created_at,
        "updated_at": package.updated_at,
    })
}
