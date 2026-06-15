use crate::backend::{config::Config, gpu::GpuWorker, pool::Pool, stats::Stats};
use parking_lot::Mutex;
use std::sync::Arc;

pub struct AppState {
    pub rt: tokio::runtime::Runtime,
    pub config: Mutex<Config>,
    pub stats: Arc<Stats>,
    pub pool: Mutex<Option<Pool>>,
    pub gpu: Mutex<Option<GpuWorker>>,
    pub last_cpu_target: Mutex<f64>,
}

impl AppState {
    pub fn new() -> Self {
        let config = Config::load();
        if config.has_credentials() {
            eprintln!(
                "[boot] saved account found: {}",
                config.nickname.as_deref().unwrap_or("?")
            );
        } else if config.nickname.is_some() {
            eprintln!("[boot] partial account found (missing recovery code or uuid)");
        } else {
            eprintln!("[boot] no saved account");
        }

        Self {
            rt: tokio::runtime::Runtime::new().expect("failed to create tokio runtime"),
            config: Mutex::new(config),
            stats: Arc::new(Stats::new()),
            pool: Mutex::new(None),
            gpu: Mutex::new(None),
            last_cpu_target: Mutex::new(1.0),
        }
    }
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountView {
    pub uuid: Option<String>,
    pub nickname: Option<String>,
    pub has_recovery_code: bool,
    pub ready: bool,
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
    pub backend: String,
    pub gpu_status: String,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Contributor {
    pub name: String,
    pub avatar_url: String,
    pub web_url: String,
}
