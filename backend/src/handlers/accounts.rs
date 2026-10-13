use std::collections::HashMap;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use serde_json::{Value, json};

use rwisp_server::accounts::{
    self, Account, AccountFilter, AccountPackage, AccountUpdate, NewAccount, PasswordReset,
    RenewalRequest, StatusOutcome,
};

use super::common::parse_id;
use crate::api_error::ApiError;
use crate::state::AppState;

pub async fn create(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let new_account = NewAccount::from_json(&body)?;
    let id = accounts::create_account(&state.db, &new_account).await?;
    let account = load(&state, id).await?;

    Ok((
        StatusCode::CREATED,
        Json(json!({ "account": account_json(&account) })),
    ))
}

pub async fn list(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, ApiError> {
    let filter = AccountFilter::from_query(&params)?;
    let records = accounts::list_accounts(&state.db, &filter).await?;
    let accounts: Vec<Value> = records.iter().map(account_json).collect();

    Ok(Json(json!({
        "accounts": accounts,
        "count": accounts.len(),
        "limit": filter.limit,
        "offset": filter.offset,
    })))
}

pub async fn detail(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let id = parse_id(&id)?;
    let account = load(&state, id).await?;
    Ok(Json(json!({ "account": account_json(&account) })))
}

pub async fn update(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let id = parse_id(&id)?;
    let update = AccountUpdate::from_json(&body)?;

    if !accounts::update_account(&state.db, id, &update).await? {
        return Err(ApiError::not_found(format!("account {id} was not found")));
    }

    let account = load(&state, id).await?;
    Ok(Json(json!({ "account": account_json(&account) })))
}

pub async fn reset_password(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Result<StatusCode, ApiError> {
    let id = parse_id(&id)?;
    let reset = PasswordReset::from_json(&body)?;

    if !accounts::reset_password(&state.db, id, &reset.password).await? {
        return Err(ApiError::not_found(format!("account {id} was not found")));
    }

    Ok(StatusCode::NO_CONTENT)
}

pub async fn suspend(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let id = parse_id(&id)?;
    match accounts::suspend_account(&state.db, id).await? {
        StatusOutcome::Updated(account) => Ok(Json(json!({ "account": account_json(&account) }))),
        StatusOutcome::NotFound => Err(ApiError::not_found(format!("account {id} was not found"))),
        StatusOutcome::InvalidState { status } => Err(ApiError::conflict(format!(
            "account {id} cannot be suspended (status: {})",
            status.as_str()
        ))),
    }
}

pub async fn reactivate(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let id = parse_id(&id)?;
    match accounts::reactivate_account(&state.db, id).await? {
        StatusOutcome::Updated(account) => Ok(Json(json!({ "account": account_json(&account) }))),
        StatusOutcome::NotFound => Err(ApiError::not_found(format!("account {id} was not found"))),
        StatusOutcome::InvalidState { status } => Err(ApiError::conflict(format!(
            "account {id} cannot be reactivated (status: {})",
            status.as_str()
        ))),
    }
}

pub async fn delete(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let id = parse_id(&id)?;

    if !accounts::delete_account(&state.db, id).await? {
        return Err(ApiError::not_found(format!("account {id} was not found")));
    }

    Ok(StatusCode::NO_CONTENT)
}

pub async fn renew(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let id = parse_id(&id)?;
    let request = RenewalRequest::from_json(&body)?;
    let result = accounts::renew_account(&state.db, id, &request).await?;

    Ok(Json(json!({
        "account": account_json(&result.account),
        "assignment": assignment_json(&result.assignment),
        "activation": result.assignment.activation.as_str(),
    })))
}

pub async fn packages(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let id = parse_id(&id)?;
    if accounts::get_account(&state.db, id).await?.is_none() {
        return Err(ApiError::not_found(format!("account {id} was not found")));
    }

    let assignments = accounts::list_account_packages(&state.db, id).await?;
    let packages: Vec<Value> = assignments.iter().map(assignment_json).collect();

    Ok(Json(json!({
        "account_id": id,
        "count": packages.len(),
        "packages": packages,
    })))
}

async fn load(state: &AppState, id: u64) -> Result<Account, ApiError> {
    accounts::get_account(&state.db, id)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("account {id} was not found")))
}

fn account_json(account: &Account) -> Value {
    json!({
        "id": account.id,
        "username": account.username,
        "full_name": account.full_name,
        "email": account.email,
        "phone": account.phone,
        "address": account.address,
        "router_id": account.router_id,
        "package_id": account.package_id,
        "status": account.status.as_str(),
        "subscription_status": account.subscription_status.as_str(),
        "activated_at": account.activated_at,
        "expires_at": account.expires_at,
        "last_login_at": account.last_login_at,
        "last_login_ip": account.last_login_ip,
        "usage_secs": account.usage_secs,
        "bytes_in": account.bytes_in,
        "bytes_out": account.bytes_out,
        "created_at": account.created_at,
        "updated_at": account.updated_at,
    })
}

fn assignment_json(assignment: &AccountPackage) -> Value {
    json!({
        "id": assignment.id,
        "account_id": assignment.account_id,
        "package_id": assignment.package_id,
        "activation": assignment.activation.as_str(),
        "status": assignment.status.as_str(),
        "starts_at": assignment.starts_at,
        "expires_at": assignment.expires_at,
        "activated_at": assignment.activated_at,
        "created_at": assignment.created_at,
    })
}
