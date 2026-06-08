use reqwest::Client;
use serde::{Deserialize, Serialize};

const BASE_URL: &str = "https://bogo.swapjs.dev";
const API_URL: &str = "https://bogo.swapjs.dev/api";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountInfo {
    pub uuid: String,
    pub nickname: String,
    pub code: String,
    #[serde(default)]
    pub total: u64,
    #[serde(default)]
    pub all_time_best: u32,
    #[serde(default)]
    pub active_ms: u64,
    #[serde(default)]
    pub max_session_ms: u64,
    #[serde(default)]
    pub yt_linked: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LeaderboardEntry {
    pub nickname: String,
    #[serde(default)]
    pub total: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LeaderboardResponse {
    #[serde(default)]
    pub top: Vec<LeaderboardEntry>,
}

pub async fn get_leaderboard(limit: u32) -> Result<Vec<LeaderboardEntry>, String> {
    let client = Client::new();
    let resp = client
        .get(format!("{}/leaderboard", API_URL))
        .query(&[("limit", limit.to_string())])
        .send()
        .await
        .map_err(|e| format!("network error: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("server error: {}", resp.status()));
    }

    let lb: LeaderboardResponse = resp
        .json()
        .await
        .map_err(|e| format!("parse error: {}", e))?;

    Ok(lb.top)
}

pub fn generate_uuid() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let pid = std::process::id() as u128;
    let mixed = (nanos ^ (pid << 64)) ^ ((nanos >> 7).wrapping_mul(0x9e3779b97f4a7c15));
    format!("{:032x}", mixed)
}
