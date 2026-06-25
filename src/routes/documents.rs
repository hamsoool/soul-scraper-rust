use axum::{
    extract::{Path, Query, State},
    Json,
};
use serde::Deserialize;

use crate::{db, error::Result, AppState};

/// Query parameters for `GET /documents`.
#[derive(Debug, Deserialize)]
pub struct ListParams {
    pub category: Option<String>,
    #[serde(default = "default_limit")]
    pub limit: i64,
    #[serde(default)]
    pub offset: i64,
}

fn default_limit() -> i64 {
    20
}

/// `GET /documents` — paginated document list.
pub async fn list_documents(
    State(state): State<AppState>,
    Query(params): Query<ListParams>,
) -> Result<Json<Vec<db::DocumentListItem>>> {
    let limit = params.limit.clamp(1, 100);
    let docs = db::get_documents(
        &state.pool,
        params.category.as_deref(),
        limit,
        params.offset,
    )
    .await?;
    Ok(Json(docs))
}

/// `GET /documents/:id` — single document with full content.
pub async fn get_document(
    State(state): State<AppState>,
    Path(id): Path<i32>,
) -> Result<Json<db::Document>> {
    let doc = db::get_document_by_id(&state.pool, id).await?;
    Ok(Json(doc))
}

/// `GET /latest` — most recent document per category.
pub async fn get_latest(
    State(state): State<AppState>,
) -> Result<Json<Vec<db::DocumentListItem>>> {
    let docs = db::get_latest_per_category(&state.pool).await?;
    Ok(Json(docs))
}
