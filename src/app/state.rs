use crate::backend::{config::Config, pool::Pool, stats::Stats};
use parking_lot::Mutex;
use std::sync::Arc;

pub struct AppState {
    pub rt: tokio::runtime::Runtime,
    pub config: Mutex<Config>,
    pub stats: Arc<Stats>,
    pub pool: Mutex<Option<Pool>>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            rt: tokio::runtime::Runtime::new().expect("failed to create tokio runtime"),
            config: Mutex::new(Config::load()),
            stats: Arc::new(Stats::new()),
            pool: Mutex::new(None),
        }
    }
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountView {
    pub uuid: Option<String>,
    pub nickname: Option<String>,
    pub has_recovery_code: bool,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeStats {
    pub running: bool,
    pub session_shuffles: u64,
    pub lifetime_shuffles: u64,
    pub rate: u64,
    pub session_best: i32,
    pub tick_best: i32,
    pub all_time_best: i32,
    pub active_workers: u64,
    pub solver_threads: u64,
    pub lease_cursor: u64,
    pub lease_count: u64,
    pub last5: Vec<u8>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Contributor {
    pub name: String,
    pub avatar_url: String,
    pub web_url: String,
}
