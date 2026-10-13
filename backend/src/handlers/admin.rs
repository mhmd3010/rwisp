use axum::Extension;
use axum::Json;
use axum::extract::{Path, Query, State};
use serde_json::{Value, json};

use super::common::parse_id;
use crate::admin::{self, AdminIdentity, Role};
use crate::api_error::ApiError;
use crate::state::AppState;

pub async fn list_tokens(
    State(state): State<AppState>,
    Extension(_identity): Extension<AdminIdentity>,
) -> Result<Json<Value>, ApiError> {
    let rows = sqlx::query(
        "SELECT id, name, role, enabled, CAST(created_at AS CHAR) AS created_at, \
         CAST(last_used_at AS CHAR) AS last_used_at FROM tbl_admin_tokens ORDER BY id",
    )
    .fetch_all(&state.db)
    .await?;
    use sqlx::Row;
    let mut tokens = Vec::with_capacity(rows.len());
    for row in rows {
        tokens.push(json!({
            "id": row.try_get::<u64, _>("id")?,
            "name": row.try_get::<String, _>("name")?,
            "role": row.try_get::<String, _>("role")?,
            "enabled": row.try_get::<i8, _>("enabled")? != 0,
            "created_at": row.try_get::<Option<String>, _>("created_at")?,
            "last_used_at": row.try_get::<Option<String>, _>("last_used_at")?,
        }));
    }
    Ok(Json(json!({ "tokens": tokens })))
}

pub async fn create_token(
    State(state): State<AppState>,
    Extension(identity): Extension<AdminIdentity>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    identity.require(Role::Admin)?;

    let name = body
        .get("name")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .ok_or_else(|| ApiError::bad_request("name is required"))?;
    let role = body
        .get("role")
        .and_then(|v| v.as_str())
        .and_then(Role::parse)
        .unwrap_or(Role::Readonly);

    let token = admin::generate_secret();
    let id = admin::create_db_token(&state.db, name, &token, role)
        .await
        .map_err(|_| ApiError::internal())?;

    admin::audit(
        &state.db,
        &identity.actor,
        identity.role,
        "admin_token.create",
        Some("admin_token"),
        Some(&id.to_string()),
        Some(name),
    )
    .await;

    // The token is shown exactly once, at creation.
    Ok(Json(json!({
        "id": id,
        "name": name,
        "role": role.as_str(),
        "token": token,
    })))
}

pub async fn revoke_token(
    State(state): State<AppState>,
    Extension(identity): Extension<AdminIdentity>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    identity.require(Role::Admin)?;
    let id = parse_id(&id)?;

    if !admin::revoke_db_token(&state.db, id).await? {
        return Err(ApiError::not_found(format!(
            "admin token {id} was not found"
        )));
    }

    admin::audit(
        &state.db,
        &identity.actor,
        identity.role,
        "admin_token.revoke",
        Some("admin_token"),
        Some(&id.to_string()),
        None,
    )
    .await;

    Ok(Json(json!({ "id": id, "enabled": false })))
}

pub async fn audit_log(
    State(state): State<AppState>,
    Extension(_identity): Extension<AdminIdentity>,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> Result<Json<Value>, ApiError> {
    let limit = params
        .get("limit")
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(100)
        .clamp(1, 500);

    let rows = sqlx::query(
        "SELECT id, actor, role, action, target_type, target_id, detail, \
         CAST(created_at AS CHAR) AS created_at FROM tbl_admin_audit \
         ORDER BY id DESC LIMIT ?",
    )
    .bind(i64::from(limit))
    .fetch_all(&state.db)
    .await?;
    use sqlx::Row;
    let mut entries = Vec::with_capacity(rows.len());
    for row in rows {
        entries.push(json!({
            "id": row.try_get::<u64, _>("id")?,
            "actor": row.try_get::<String, _>("actor")?,
            "role": row.try_get::<String, _>("role")?,
            "action": row.try_get::<String, _>("action")?,
            "target_type": row.try_get::<Option<String>, _>("target_type")?,
            "target_id": row.try_get::<Option<String>, _>("target_id")?,
            "detail": row.try_get::<Option<String>, _>("detail")?,
            "created_at": row.try_get::<Option<String>, _>("created_at")?,
        }));
    }
    Ok(Json(json!({ "audit": entries, "count": entries.len() })))
}
