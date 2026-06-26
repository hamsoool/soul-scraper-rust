use axum::{
    extract::{Path, Query, State},
    Json,
};
use serde::Deserialize;
use utoipa::{IntoParams, ToSchema};

use crate::{config, db, error::Result, AppState};

/// Query parameters for `GET /documents`.
#[derive(Debug, Deserialize, ToSchema, IntoParams)]
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
#[utoipa::path(
    get,
    path = "/documents",
    tag = "documents",
    params(ListParams),
    responses(
        (status = 200, description = "Paginated list of documents", body = [db::DocumentListItem])
    )
)]
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
#[utoipa::path(
    get,
    path = "/documents/{id}",
    tag = "documents",
    params(
        ("id" = i32, Path, description = "Document ID")
    ),
    responses(
        (status = 200, description = "Document with full PDF text content", body = db::Document),
        (status = 404, description = "Document not found")
    )
)]
pub async fn get_document(
    State(state): State<AppState>,
    Path(id): Path<i32>,
) -> Result<Json<db::Document>> {
    let doc = db::get_document_by_id(&state.pool, id).await?;
    Ok(Json(doc))
}

/// `GET /latest` — most recent document per category.
#[utoipa::path(
    get,
    path = "/latest",
    tag = "documents",
    responses(
        (status = 200, description = "Latest document per category", body = [db::DocumentListItem])
    )
)]
pub async fn get_latest(
    State(state): State<AppState>,
) -> Result<Json<Vec<db::DocumentListItem>>> {
    let docs = db::get_latest_per_category(&state.pool).await?;
    Ok(Json(docs))
}

/// `GET /categories` — returns the full scrape configuration (target URL + aggregators).
#[utoipa::path(
    get,
    path = "/categories",
    tag = "documents",
    responses(
        (status = 200, description = "Configured scrape targets", body = config::ScrapeConfig)
    )
)]
pub async fn get_categories(
    State(state): State<AppState>,
) -> Json<config::ScrapeConfig> {
    Json(state.scrape_config)
}
