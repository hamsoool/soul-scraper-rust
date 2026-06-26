use std::env;

use serde::{Deserialize, Serialize};
use url::Url;
use utoipa::ToSchema;

/// A single scrape target sourced from `sources.json`.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AggregatorConfig {
    pub url: String,
    pub category: String,
    pub file_types: Vec<String>,
    /// Scraping strategy: "nuxt" (Nuxt.js SSR state) or "html" (generic link extraction)
    #[serde(default = "default_scraper_type")]
    pub scraper_type: String,
}

fn default_scraper_type() -> String {
    "nuxt".to_string()
}

/// Top-level scrape configuration from `sources.json`.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ScrapeConfig {
    pub target_url: String,
    pub aggregators: Vec<AggregatorConfig>,
}

impl ScrapeConfig {
    /// Collects all unique hostnames referenced in the config (target + all aggregator URLs).
    pub fn extract_domains(&self) -> Vec<String> {
        let mut domains: Vec<String> = Vec::new();
        for url_str in Some(&self.target_url)
            .into_iter()
            .chain(self.aggregators.iter().map(|a| &a.url))
        {
            if let Ok(parsed) = Url::parse(url_str) {
                if let Some(host) = parsed.host_str() {
                    let host = host.to_lowercase();
                    if !domains.contains(&host) {
                        domains.push(host);
                    }
                }
            }
        }
        domains
    }
}

/// All application settings loaded from environment variables / `.env`.
#[derive(Debug, Clone)]
pub struct Settings {
    /// PostgreSQL connection URL (standard `postgresql://...` or `postgres://`)
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

    /// Path to the JSON file listing scrape sources (default "sources.json")
    pub sources_config_path: String,

    /// API key for protecting endpoints (auto-generated if not provided via env)
    pub api_key: String,
    /// Whether the API key was auto-generated (true) or user-provided (false)
    pub api_key_auto_generated: bool,
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

        let mut settings = Settings {
            database_url,
            max_pdf_size_bytes: env_parse("MAX_PDF_SIZE_BYTES", 10 * 1024 * 1024),
            http_timeout_seconds: env_parse("HTTP_TIMEOUT_SECONDS", 30),
            sync_interval_hours: env_parse("SYNC_INTERVAL_HOURS", 24),
            enable_scraper_scheduler: env_bool("ENABLE_SCRAPER_SCHEDULER", true),
            host: env::var("HOST").unwrap_or_else(|_| "0.0.0.0".to_string()),
            port: env_parse("PORT", 8000),
            sources_config_path: env::var("SOURCES_CONFIG_PATH")
                .unwrap_or_else(|_| "sources.json".to_string()),
            api_key: env::var("API_KEY").unwrap_or_default(),
            api_key_auto_generated: false,
        };

        if settings.api_key.is_empty() {
            settings.api_key = generate_api_key();
            settings.api_key_auto_generated = true;
        }

        settings
    }

    /// Reads and parses the `sources.json` file into a `ScrapeConfig`.
    pub fn load_sources(&self) -> anyhow::Result<ScrapeConfig> {
        let text = std::fs::read_to_string(&self.sources_config_path)?;
        let config: ScrapeConfig = serde_json::from_str(&text)?;
        Ok(config)
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

/// Generates a cryptographically random 256-bit API key.
fn generate_api_key() -> String {
    let mut bytes = [0u8; 32];
    getrandom::getrandom(&mut bytes).expect("failed to generate random API key");
    let hex: String = bytes.iter().map(|b| format!("{:02x}", b)).collect();
    format!("sk-{}", hex)
}
