use axum::Json;
use axum::extract::State;
use serde_json::{Value, json};

use crate::api_error::ApiError;
use crate::state::AppState;

/// Current scheduler job state (lease owner, last run/status/detail).
pub async fn jobs(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let jobs = rwisp_server::scheduler::status(&state.db).await?;
    let jobs: Vec<Value> = jobs
        .iter()
        .map(|job| {
            json!({
                "job": job.job_name,
                "lease_owner": job.lease_owner,
                "lease_until": job.lease_until,
                "last_started_at": job.last_started_at,
                "last_finished_at": job.last_finished_at,
                "last_status": job.last_status,
                "last_error": job.last_error,
                "last_detail": job.last_detail,
                "runs": job.runs,
            })
        })
        .collect();
    Ok(Json(json!({ "jobs": jobs })))
}
