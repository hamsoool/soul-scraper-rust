use std::sync::Arc;

use axum::{
    middleware,
    routing::{get, post},
    Router,
};
use sqlx::postgres::PgPoolOptions;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
use tracing::info;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};
use utoipa::openapi::security::{ApiKey, ApiKeyValue, SecurityScheme};
use utoipa::{Modify, OpenApi};
use utoipa_swagger_ui::SwaggerUi;

use soul_scrape_rust::{
    auth,
    config::{self, Settings},
    db,
    routes::{documents, system},
    scheduler::{new_state, run_sync_once, start_scheduler},
    security,
    AppState,
};

#[derive(OpenApi)]
#[openapi(
    paths(
        system::health,
        system::get_stats,
        system::trigger_sync,
        documents::list_documents,
        documents::get_document,
        documents::get_latest,
        documents::get_categories,
    ),
    components(
        schemas(
            db::Document,
            db::DocumentListItem,
            config::ScrapeConfig,
            config::AggregatorConfig,
            system::StatsResponse,
            system::SyncResponse,
            documents::ListParams,
        )
    ),
    modifiers(&SecurityAddon),
    tags(
        (name = "system", description = "System endpoints (health, stats, sync trigger)"),
        (name = "documents", description = "Document and category endpoints"),
    )
)]
struct ApiDoc;

struct SecurityAddon;

impl Modify for SecurityAddon {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        if let Some(components) = openapi.components.as_mut() {
            components.add_security_scheme(
                "api_key",
                SecurityScheme::ApiKey(ApiKey::Header(ApiKeyValue::new("X-API-Key"))),
            )
        }
        let requirement: utoipa::openapi::security::SecurityRequirement =
            utoipa::openapi::security::SecurityRequirement::new("api_key", Vec::<String>::new());
        openapi.security = Some(vec![requirement]);
    }
}

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
    if settings.api_key_auto_generated {
        info!("API key auto-generated (set API_KEY in .env to use your own): {}", settings.api_key);
    } else {
        info!("API key loaded from environment.");
    }
    info!(
        "Soul Scraper Rust starting on {}:{}",
        settings.host, settings.port
    );

    // Load scrape sources from sources.json
    let scrape_config = settings
        .load_sources()
        .expect("Failed to load sources.json — check the file exists and is valid JSON");
    info!(
        "Loaded {} aggregator(s) targeting '{}'.",
        scrape_config.aggregators.len(),
        scrape_config.target_url
    );

    // Seed security allowlist from config domains
    let domains = scrape_config.extract_domains();
    security::init_allowed_domains(domains);

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
        start_scheduler(pool.clone(), sync_state.clone(), settings.clone(), scrape_config.clone());

        if db::is_empty(&pool).await? {
            info!("Database is empty — triggering initial sync in background.");
            tokio::spawn(run_sync_once(
                pool.clone(),
                sync_state.clone(),
                settings.clone(),
                scrape_config.clone(),
            ));
        }
    }

    let state = AppState {
        pool,
        sync_state,
        settings: settings.clone(),
        scrape_config,
    };

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        .merge(SwaggerUi::new("/docs").url("/api-docs/openapi.json", ApiDoc::openapi()))
        // Welcome/Index
        .route("/", get(|| async {
            axum::Json(serde_json::json!({
                "message": "Welcome to Soul Scraper API!",
                "endpoints": {
                    "health": "/health",
                    "stats": "/stats",
                    "documents": "/documents",
                    "categories": "/categories",
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
        .route("/categories", get(documents::get_categories))
        .route("/latest", get(documents::get_latest))
        .with_state(state.clone())
        .layer(middleware::from_fn_with_state(
            state,
            auth::require_api_key,
        ))
        .layer(cors)
        .layer(TraceLayer::new_for_http());

    let addr = format!("{}:{}", settings.host, settings.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    info!("Listening on http://{}", addr);

    axum::serve(listener, app).await?;
    Ok(())
}
