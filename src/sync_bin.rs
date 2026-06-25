/// Standalone sync binary — equivalent to the original `sync.py`.
/// Run with: `cargo run --bin sync`
/// Useful for scheduling via Windows Task Scheduler or cron.

use std::sync::Arc;

use sqlx::postgres::PgPoolOptions;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

use soul_scrape_rust::{config, scraper};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::from_default_env().add_directive("soul_scrape_rust=info".parse()?))
        .init();

    let settings = Arc::new(config::Settings::from_env());

    println!("==============================================================");
    println!("  Soul Scraper — Local Sync Session");
    println!("==============================================================");

    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&settings.database_url)
        .await?;

    sqlx::migrate!("./migrations").run(&pool).await?;

    let result = scraper::sync_doe_data(&pool, &settings).await;

    println!();
    println!("==============================================================");
    println!("  Sync Finished!");
    println!("==============================================================");
    println!("Status:          {}", result.status);
    println!("Processed:       {} new PDF(s)", result.processed_count);
    println!("Time Taken:      {:.2}s", result.duration_seconds);
    println!("Cleanup:         {} outdated, {} duplicates removed",
        result.cleanup_outdated, result.cleanup_duplicates);

    if !result.errors.is_empty() {
        println!("\nErrors:");
        for e in &result.errors {
            println!("  - {e}");
        }
    }

    println!("==============================================================");

    if result.status == "error" {
        std::process::exit(1);
    }

    Ok(())
}
