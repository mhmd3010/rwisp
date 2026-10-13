use std::collections::HashMap;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use serde_json::{Value, json};

use rwisp_server::vouchers::{self, BatchRequest, RevokeOutcome, Voucher, VoucherFilter};

use super::common::parse_id;
use crate::api_error::ApiError;
use crate::state::AppState;

pub async fn create(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let request = BatchRequest::from_json(&body)?;
    let batch = vouchers::create_batch(&state.db, &request).await?;

    Ok((
        StatusCode::CREATED,
        Json(json!({
            "batch_id": batch.batch_id,
            "package_id": request.package_id,
            "router_id": request.router_id,
            "quantity": batch.quantity,
            "codes": batch.codes,
        })),
    ))
}

pub async fn list(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, ApiError> {
    let filter = VoucherFilter::from_query(&params)?;
    let records = vouchers::list_vouchers(&state.db, &filter).await?;
    let vouchers: Vec<Value> = records.iter().map(voucher_json).collect();

    Ok(Json(json!({
        "vouchers": vouchers,
        "count": vouchers.len(),
        "limit": filter.limit,
        "offset": filter.offset,
    })))
}

pub async fn detail(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let id = parse_id(&id)?;
    let voucher = vouchers::get_voucher(&state.db, id)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("voucher {id} was not found")))?;

    Ok(Json(json!({ "voucher": voucher_json(&voucher) })))
}

pub async fn revoke(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let id = parse_id(&id)?;

    match vouchers::revoke_voucher(&state.db, id).await? {
        RevokeOutcome::Revoked(voucher) => Ok(Json(json!({ "voucher": voucher_json(&voucher) }))),
        RevokeOutcome::NotFound => Err(ApiError::not_found(format!("voucher {id} was not found"))),
        RevokeOutcome::NotRevocable { status } => Err(ApiError::conflict(format!(
            "voucher {id} cannot be revoked (status: {})",
            status.as_str()
        ))),
    }
}

pub async fn batch_codes(
    State(state): State<AppState>,
    Path(batch_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let batch_id = parse_id(&batch_id)?;

    if !vouchers::batch_exists(&state.db, batch_id).await? {
        return Err(ApiError::not_found(format!(
            "voucher batch {batch_id} was not found"
        )));
    }

    let codes = vouchers::list_batch_codes(&state.db, batch_id).await?;

    Ok(Json(json!({
        "batch_id": batch_id,
        "count": codes.len(),
        "codes": codes,
    })))
}

fn voucher_json(voucher: &Voucher) -> Value {
    json!({
        "id": voucher.id,
        "batch_id": voucher.batch_id,
        "package_id": voucher.package_id,
        "router_id": voucher.router_id,
        "status": voucher.status.as_str(),
        "created_at": voucher.created_at,
        "activated_at": voucher.activated_at,
        "expires_at": voucher.expires_at,
        "last_used_at": voucher.last_used_at,
        "usage_secs": voucher.usage_secs,
        "bytes_in": voucher.bytes_in,
        "bytes_out": voucher.bytes_out,
        "revoked_at": voucher.revoked_at,
    })
}
