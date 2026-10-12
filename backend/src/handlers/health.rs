use crate::state::AppState;
use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use serde_json::{Value, json};

pub async fn health() -> Json<Value> {
    Json(json!({
        "status": "ok",
        "service": "rwisp-server"
    }))
}

pub async fn health_db(
    State(state): State<AppState>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)> {
    match sqlx::query("SELECT 1").fetch_all(&state.db).await {
        Ok(_) => Ok((
            StatusCode::OK,
            Json(json!({
                "status": "ok",
                "database": "connected"
            })),
        )),
        Err(error) => {
            println!("Database health check failed: {}", error);
            Err((
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({
                    "error": "Database connection failed"
                })),
            ))
        }
    }
}
