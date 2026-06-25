use std::sync::Arc;

use axum::{
    routing::{get, post},
    Router,
};
use sqlx::postgres::PgPoolOptions;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
use tracing::info;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

use soul_scrape_rust::{
    config::Settings,
    db,
    routes::{documents, system},
    scheduler::{new_state, run_sync_once, start_scheduler},
    AppState,
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Load .env file (silently ignore if absent)
    dotenvy::dotenv().ok();

    // Structured logging (RUST_LOG env controls level; defaults to INFO)
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("soul_scrape_rust=info,tower_http=warn")),
        )
        .init();

    let settings = Arc::new(Settings::from_env());
    info!(
        "Soul Scraper Rust starting on {}:{}",
        settings.host, settings.port
    );

    // Build PostgreSQL connection pool
    let pool = PgPoolOptions::new()
        .max_connections(20)
        .min_connections(2)
        .acquire_timeout(std::time::Duration::from_secs(30))
        .connect(&settings.database_url)
        .await?;

    info!("Connected to database.");

    // Run pending migrations
    sqlx::migrate!("./migrations").run(&pool).await?;
    info!("Database migrations applied.");

    let sync_state = new_state();

    // Start background scheduler if enabled
    if settings.enable_scraper_scheduler {
        start_scheduler(pool.clone(), sync_state.clone(), settings.clone());

        if db::is_empty(&pool).await? {
            info!("Database is empty — triggering initial sync in background.");
            tokio::spawn(run_sync_once(
                pool.clone(),
                sync_state.clone(),
                settings.clone(),
            ));
        }
    }

    let state = AppState {
        pool,
        sync_state,
        settings: settings.clone(),
    };

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        // Welcome/Index
        .route("/", get(|| async {
            axum::Json(serde_json::json!({
                "message": "Welcome to Soul Scraper API!",
                "endpoints": {
                    "health": "/health",
                    "stats": "/stats",
                    "documents": "/documents",
                    "latest": "/latest"
                }
            }))
        }))
        // System endpoints
        .route("/health", get(system::health))
        .route("/stats", get(system::get_stats))
        .route("/sync", post(system::trigger_sync))
        // Document endpoints
        .route("/documents", get(documents::list_documents))
        .route("/documents/:id", get(documents::get_document))
        .route("/latest", get(documents::get_latest))
        .with_state(state)
        .layer(cors)
        .layer(TraceLayer::new_for_http());

    let addr = format!("{}:{}", settings.host, settings.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    info!("Listening on http://{}", addr);

    axum::serve(listener, app).await?;
    Ok(())
}
