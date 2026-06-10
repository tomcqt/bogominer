use reqwest::Client;
use serde::{Deserialize, Serialize};

const BASE_URL: &str = "https://bogo.swapjs.dev";
const API_URL: &str = "https://bogo.swapjs.dev/api";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountInfo {
    #[serde(default)]
    pub uuid: String,
    #[serde(default)]
    pub nickname: String,
    #[serde(default)]
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
#[serde(rename_all = "camelCase")]
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

#[derive(Debug, Clone, Serialize)]
struct CreatePayload<'a> {
    nickname: &'a str,
}

#[derive(Debug, Clone, Serialize)]
struct LoginPayload<'a> {
    code: &'a str,
}

pub async fn create_account(nickname: &str) -> Result<AccountInfo, String> {
    let client = reqwest::Client::new();
    eprintln!("[api] create_account nickname={}", nickname);
    let resp = client
        .post(format!("{}/account/create", API_URL))
        .json(&CreatePayload { nickname })
        .send()
        .await
        .map_err(|e| format!("network error: {}", e))?;

    if resp.status() == reqwest::StatusCode::BAD_REQUEST {
        let e = resp.json::<serde_json::Value>().await.unwrap_or_default();
        let msg = e
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("bad request");
        return Err(msg.to_string());
    }
    if !resp.status().is_success() {
        return Err(format!("server error: {}", resp.status()));
    }

    eprintln!("[api] create_account success");
    resp.json::<AccountInfo>()
        .await
        .map_err(|e| format!("parse error: {}", e))
}

pub async fn login_with_code(code: &str) -> Result<AccountInfo, String> {
    let client = reqwest::Client::new();
    eprintln!(
        "[api] login_with_code code={}...{}",
        &code[..4.min(code.len())],
        &code[code.len().saturating_sub(4)..]
    );
    let resp = client
        .post(format!("{}/api/account/login", BASE_URL))
        .json(&LoginPayload { code })
        .send()
        .await
        .map_err(|e| format!("network error: {}", e))?;

    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        return Err("no account with that code.".into());
    }
    if !resp.status().is_success() {
        return Err(format!("server error: {}", resp.status()));
    }

    eprintln!("[api] login_with_code success");
    resp.json::<AccountInfo>()
        .await
        .map_err(|e| format!("parse error: {}", e))
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
