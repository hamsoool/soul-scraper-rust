use std::env;

/// All application settings loaded from environment variables / `.env`.
#[derive(Debug, Clone)]
pub struct Settings {
    /// PostgreSQL connection URL (standard `postgresql://...` or `postgres://...`)
    pub database_url: String,

    /// Maximum PDF download size in bytes (default 10 MB)
    pub max_pdf_size_bytes: usize,

    /// HTTP request timeout in seconds (default 30)
    pub http_timeout_seconds: u64,

    /// How often to run the background sync, in hours (default 24)
    pub sync_interval_hours: u64,

    /// Whether to run the APScheduler-equivalent background sync (default true)
    pub enable_scraper_scheduler: bool,

    /// Bind host (default "0.0.0.0")
    pub host: String,

    /// Bind port (default 8000)
    pub port: u16,
}

impl Settings {
    /// Loads settings from environment variables, falling back to sane defaults.
    /// Call `dotenvy::dotenv().ok()` before this to pick up `.env` files.
    pub fn from_env() -> Self {
        let database_url = env::var("DATABASE_URL")
            .unwrap_or_else(|_| {
                "postgresql://postgres:postgres@localhost:5432/doe_scraper".to_string()
            });

        // Normalise postgres:// → postgresql://
        let database_url = if database_url.starts_with("postgres://") {
            database_url.replacen("postgres://", "postgresql://", 1)
        } else {
            database_url
        };

        Settings {
            database_url,
            max_pdf_size_bytes: env_parse("MAX_PDF_SIZE_BYTES", 10 * 1024 * 1024),
            http_timeout_seconds: env_parse("HTTP_TIMEOUT_SECONDS", 30),
            sync_interval_hours: env_parse("SYNC_INTERVAL_HOURS", 24),
            enable_scraper_scheduler: env_bool("ENABLE_SCRAPER_SCHEDULER", true),
            host: env::var("HOST").unwrap_or_else(|_| "0.0.0.0".to_string()),
            port: env_parse("PORT", 8000),
        }
    }
}

fn env_parse<T: std::str::FromStr>(key: &str, default: T) -> T {
    env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn env_bool(key: &str, default: bool) -> bool {
    match env::var(key).as_deref() {
        Ok("true") | Ok("1") | Ok("yes") => true,
        Ok("false") | Ok("0") | Ok("no") => false,
        _ => default,
    }
}
