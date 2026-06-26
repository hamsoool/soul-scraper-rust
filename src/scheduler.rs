use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use tokio::time::{interval, Duration};
use tracing::{info, warn};


use crate::{
    config::{self, Settings},
    scraper::sync_doe_data,
};

/// Shared sync state accessible by routes and the background scheduler.
#[derive(Debug, Default, Clone)]
pub struct SyncState {
    pub is_syncing: bool,
    pub last_sync_time: Option<DateTime<Utc>>,
    pub last_sync_result: Option<serde_json::Value>,
}

pub type SharedSyncState = Arc<Mutex<SyncState>>;

/// Creates a new default shared sync state.
pub fn new_state() -> SharedSyncState {
    Arc::new(Mutex::new(SyncState::default()))
}

/// Runs one sync cycle if not already running.
pub async fn run_sync_once(
    pool: PgPool,
    state: SharedSyncState,
    settings: Arc<Settings>,
    scrape_config: config::ScrapeConfig,
) {
    {
        let mut s = state.lock().unwrap();
        if s.is_syncing {
            warn!("Sync already in progress — skipping scheduled trigger.");
            return;
        }
        s.is_syncing = true;
    }

    info!("Background sync job started.");
    let result = sync_doe_data(&pool, &settings, &scrape_config).await;
    let now = Utc::now();

    let mut s = state.lock().unwrap();
    s.last_sync_time = Some(now);
    s.last_sync_result = serde_json::to_value(&result).ok();
    s.is_syncing = false;
    info!("Background sync job finished. Processed: {}", result.processed_count);
}

/// Spawns a Tokio background task that runs the sync on a fixed interval.
/// Returns a join handle that runs until the process exits.
pub fn start_scheduler(
    pool: PgPool,
    state: SharedSyncState,
    settings: Arc<Settings>,
    scrape_config: config::ScrapeConfig,
) -> tokio::task::JoinHandle<()> {
    let interval_hours = settings.sync_interval_hours;

    tokio::spawn(async move {
        let mut ticker = interval(Duration::from_secs(interval_hours * 3600));
        // The first tick fires immediately — this is intentional; it is guarded
        // by the caller only if the DB is non-empty.
        ticker.tick().await; // tick 0 — consumed here so we skip the first run
        info!("Scheduler started. Sync every {} hour(s).", interval_hours);

        loop {
            ticker.tick().await;
            info!("Scheduler tick — triggering sync.");
            run_sync_once(pool.clone(), state.clone(), settings.clone(), scrape_config.clone()).await;
        }
    })
}
