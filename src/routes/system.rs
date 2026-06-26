use axum::{extract::State, http::StatusCode, Json};
use chrono::Utc;
use serde::Serialize;
use serde_json::json;
use utoipa::ToSchema;

use crate::{
    db,
    error::Result,
    scheduler::run_sync_once,
    AppState,
};

/// `GET /health`
#[utoipa::path(
    get,
    path = "/health",
    tag = "system",
    responses(
        (status = 200, description = "Service is healthy")
    )
)]
pub async fn health() -> Json<serde_json::Value> {
    Json(json!({
        "status": "ok",
        "timestamp": Utc::now().to_rfc3339()
    }))
}

/// Response body for `GET /stats`.
#[derive(Debug, Serialize, ToSchema)]
pub struct StatsResponse {
    pub total_documents: i64,
    pub documents_by_category: std::collections::HashMap<String, i64>,
    pub last_sync_time: Option<chrono::DateTime<Utc>>,
    pub system_status: String,
}

/// `GET /stats`
#[utoipa::path(
    get,
    path = "/stats",
    tag = "system",
    responses(
        (status = 200, description = "Database statistics and scraper status", body = StatsResponse)
    )
)]
pub async fn get_stats(State(state): State<AppState>) -> Result<Json<StatsResponse>> {
    let (total, by_cat) = db::get_counts(&state.pool).await?;

    let (is_syncing, last_sync_time) = {
        let s = state.sync_state.lock().unwrap();
        (s.is_syncing, s.last_sync_time)
    };

    // Fall back to most-recent document created_at if scheduler hasn't run yet
    let last_sync = match last_sync_time {
        Some(t) => Some(t),
        None => db::get_latest_created_at(&state.pool).await?,
    };

    Ok(Json(StatsResponse {
        total_documents: total,
        documents_by_category: by_cat,
        last_sync_time: last_sync,
        system_status: if is_syncing {
            "syncing".to_string()
        } else {
            "idle".to_string()
        },
    }))
}

/// Response body for `POST /sync`.
#[derive(Debug, Serialize, ToSchema)]
pub struct SyncResponse {
    pub status: String,
    pub message: String,
    pub processed_count: u64,
    pub errors: Vec<String>,
}

/// `POST /sync` — triggers manual scrape in the background (202 Accepted).
#[utoipa::path(
    post,
    path = "/sync",
    tag = "system",
    responses(
        (status = 202, description = "Sync accepted and running in background", body = SyncResponse),
        (status = 202, description = "Sync already in progress", body = SyncResponse)
    )
)]
pub async fn trigger_sync(
    State(state): State<AppState>,
) -> (StatusCode, Json<SyncResponse>) {
    let already_running = {
        let s = state.sync_state.lock().unwrap();
        s.is_syncing
    };

    if already_running {
        return (
            StatusCode::ACCEPTED,
            Json(SyncResponse {
                status: "running".to_string(),
                message: "A synchronization session is already in progress.".to_string(),
                processed_count: 0,
                errors: vec![],
            }),
        );
    }

    tokio::spawn(run_sync_once(
        state.pool.clone(),
        state.sync_state.clone(),
        state.settings.clone(),
        state.scrape_config.clone(),
    ));

    (
        StatusCode::ACCEPTED,
        Json(SyncResponse {
            status: "accepted".to_string(),
            message: "Manual synchronization has been queued and is executing in the background."
                .to_string(),
            processed_count: 0,
            errors: vec![],
        }),
    )
}
