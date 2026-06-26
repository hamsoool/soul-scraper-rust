pub mod config;
pub mod db;
pub mod error;
pub mod routes;
pub mod scheduler;
pub mod scraper;
pub mod security;

use std::sync::Arc;

/// Shared application state passed to every route handler.
#[derive(Clone)]
pub struct AppState {
    pub pool: sqlx::PgPool,
    pub sync_state: scheduler::SharedSyncState,
    pub settings: Arc<config::Settings>,
    pub scrape_config: config::ScrapeConfig,
}
