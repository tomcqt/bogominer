use super::state::{AccountView, AppState, Contributor, RuntimeStats};
use crate::backend::{api, config::Config, pool::Pool};
use crate::misc::validate_nick;
use serde::Deserialize;
use std::{
    fs,
    path::PathBuf,
    sync::atomic::Ordering,
    time::{Duration, SystemTime},
};
use tauri::State;

const GITHUB_REPO: &str = "tomcqt/bogominer";
const CONTRIBUTORS_CACHE_MAX_AGE: Duration = Duration::from_secs(60 * 60 * 24);

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppView {
    pub account: AccountView,
    pub version: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewAccountRequest {
    pub nickname: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExistingAccountRequest {
    pub recovery_code: String,
}

fn app_view_from_config(config: &Config) -> AppView {
    AppView {
        account: AccountView {
            uuid: config.uuid.clone(),
            nickname: config.nickname.clone(),
            has_recovery_code: config.recovery_code.as_ref().is_some_and(|s| !s.is_empty()),
            ready: config.has_credentials(),
        },
        version: env!("CARGO_PKG_VERSION").to_string(),
    }
}

#[tauri::command]
pub fn get_app_state(state: State<AppState>) -> AppView {
    let config = state.config.lock();
    app_view_from_config(&config)
}

#[tauri::command]
pub fn save_new_account(req: NewAccountRequest, state: State<AppState>) -> Result<AppView, String> {
    let nick = validate_nick(&req.nickname)?;
    eprintln!("[account:create] nick={}", nick);
    let info = state.rt.block_on(api::create_account(&nick)).map_err(|e| {
        eprintln!("[account:create] error={}", e);
        e
    })?;
    eprintln!(
        "[account:create] api ok uuid={} nickname={} code_present={}",
        info.uuid,
        info.nickname,
        !info.code.is_empty()
    );

    if info.code.trim().is_empty() {
        return Err("server created account but did not return a recovery code".into());
    }

    {
        let mut config = state.config.lock();
        config.uuid = Some(info.uuid);
        config.nickname = Some(info.nickname);
        config.recovery_code = Some(info.code);
        config.save();

        let view = app_view_from_config(&config);
        eprintln!(
            "[account:create] returning ready={} uuid_present={} nick_present={} code_present={}",
            view.account.ready,
            view.account.uuid.is_some(),
            view.account.nickname.is_some(),
            view.account.has_recovery_code,
        );
        return Ok(view);
    }
}

#[tauri::command]
pub fn save_existing_account(
    req: ExistingAccountRequest,
    state: State<AppState>,
) -> Result<AppView, String> {
    let recovery_code = req.recovery_code.trim().to_string();

    if recovery_code.is_empty() {
        return Err("recovery code is required".into());
    }

    eprintln!("[account:login] code={}", recovery_code);
    let info = state
        .rt
        .block_on(api::login_with_code(&recovery_code))
        .map_err(|e| {
            eprintln!("[account:login] error={}", e);
            e
        })?;
    eprintln!(
        "[account:login] ok uuid={} nickname={}",
        info.uuid, info.nickname
    );

    let code_to_save = if info.code.trim().is_empty() {
        recovery_code
    } else {
        info.code
    };

    eprintln!(
        "[account:login] api ok uuid={} nickname={} code_present={}",
        info.uuid,
        info.nickname,
        !code_to_save.is_empty()
    );

    {
        let mut config = state.config.lock();
        config.uuid = Some(info.uuid);
        config.nickname = Some(info.nickname);
        config.recovery_code = Some(code_to_save);
        config.save();

        let view = app_view_from_config(&config);
        eprintln!(
            "[account:login] returning ready={} uuid_present={} nick_present={} code_present={}",
            view.account.ready,
            view.account.uuid.is_some(),
            view.account.nickname.is_some(),
            view.account.has_recovery_code
        );
        return Ok(view);
    }
}

#[tauri::command]
pub fn clear_account(state: State<AppState>) -> AppView {
    {
        let mut pool = state.pool.lock();
        if let Some(p) = pool.as_mut() {
            p.stop();
        }
        *pool = None;
    }

    {
        let mut config = state.config.lock();
        config.clear();
    }

    get_app_state(state)
}

#[tauri::command]
pub fn start_mining(cpu_target: f64, state: State<AppState>) -> Result<(), String> {
    let config = state.config.lock().clone();
    let uuid = config.uuid.ok_or("missing uuid")?;
    let nickname = config.nickname.ok_or("missing nickname")?;
    let code = config.recovery_code.unwrap_or_default();

    let mut pool = state.pool.lock();
    *pool = Some(Pool::new(state.stats.clone()));

    let _guard = state.rt.enter();
    pool.as_mut().unwrap().start(
        Some(uuid),
        Some(nickname),
        code,
        cpu_target.clamp(0.05, 1.0),
    );
    Ok(())
}

#[tauri::command]
pub fn stop_mining(state: State<AppState>) {
    let mut pool = state.pool.lock();
    if let Some(pool) = pool.as_mut() {
        pool.stop();
    }
    *pool = None;
}

#[tauri::command]
pub fn set_cpu_target(cpu_target: f64, state: State<AppState>) -> Result<(), String> {
    let config = state.config.lock().clone();
    let uuid = config.uuid.as_deref().ok_or("missing uuid")?;
    let nickname = config.nickname.as_deref().ok_or("missing nickname")?;
    let code = config
        .recovery_code
        .as_deref()
        .ok_or("missing recovery code")?;

    let _guard = state.rt.enter();
    let mut pool = state.pool.lock();
    match pool.as_mut() {
        Some(p) => {
            p.set_cpu_target(cpu_target.clamp(0.05, 1.0), uuid, nickname, code);
        }
        None => {
            return Err("not running".into());
        }
    }
    Ok(())
}

#[tauri::command]
pub fn get_runtime_stats(state: State<AppState>) -> RuntimeStats {
    if let Some(pool) = state.pool.lock().as_mut() {
        if let Some(code) = pool.poll_recovery_code() {
            let mut config = state.config.lock();
            config.recovery_code = Some(code);
            config.save();
        }
    }

    let running = state
        .pool
        .lock()
        .as_ref()
        .is_some_and(|pool| pool.is_running());

    RuntimeStats {
        running,
        session_shuffles: state.stats.session_shuffles.load(Ordering::Relaxed),
        lifetime_shuffles: state.stats.lifetime_shuffles.load(Ordering::Relaxed),
        rate: state.stats.rate.load(Ordering::Relaxed),
        session_best: state.stats.session_best.load(Ordering::Relaxed),
        tick_best: state.stats.tick_best.load(Ordering::Relaxed),
        all_time_best: state.stats.all_time_best.load(Ordering::Relaxed),
        active_workers: state.stats.active_workers.load(Ordering::Relaxed),
        solver_threads: state.stats.solver_threads.load(Ordering::Relaxed),
        lease_cursor: state.stats.lease_cursor.load(Ordering::Relaxed),
        lease_count: state.stats.lease_count.load(Ordering::Relaxed),
        last5: state.stats.get_last5(),
    }
}

#[tauri::command]
pub async fn get_contributors() -> Result<Vec<Contributor>, String> {
    if let Some(cached) = read_contributors_cache() {
        return Ok(cached);
    }

    match fetch_github_contributors().await {
        Ok(contributors) if !contributors.is_empty() => {
            write_contributors_cache(&contributors);
            Ok(contributors)
        }
        Ok(_) => read_stale_contributors_cache().ok_or_else(|| "no contributors found".into()),
        Err(e) => read_stale_contributors_cache().ok_or(e),
    }
}

#[tauri::command]
pub async fn get_leaderboard() -> Result<Vec<crate::backend::api::LeaderboardEntry>, String> {
    crate::backend::api::get_leaderboard(20).await
}

#[tauri::command]
pub fn open_external(url: String, app: tauri::AppHandle) -> Result<(), String> {
    if !(url.starts_with("https://github.com/") || url.starts_with("https://www.github.com/")) {
        return Err("only github links can be opened from here".into());
    }

    tauri_plugin_opener::OpenerExt::opener(&app)
        .open_url(url, None::<&str>)
        .map_err(|e| e.to_string())
}

// contributor helper functions
fn contributors_cache_path() -> Option<PathBuf> {
    directories::ProjectDirs::from("sh", "tomcat", "bogominer")
        .map(|dirs| dirs.cache_dir().join("contributors.json"))
}

fn read_contributors_cache() -> Option<Vec<Contributor>> {
    let path = contributors_cache_path()?;
    let meta = fs::metadata(&path).ok()?;
    let modified = meta.modified().ok()?;
    if SystemTime::now().duration_since(modified).ok()? > CONTRIBUTORS_CACHE_MAX_AGE {
        return None;
    }
    let data = fs::read_to_string(path).ok()?;
    serde_json::from_str(&data).ok()
}

fn read_stale_contributors_cache() -> Option<Vec<Contributor>> {
    let path = contributors_cache_path()?;
    let data = fs::read_to_string(path).ok()?;
    serde_json::from_str(&data).ok()
}

fn write_contributors_cache(contributors: &[Contributor]) {
    let Some(path) = contributors_cache_path() else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(data) = serde_json::to_string_pretty(contributors) {
        let _ = fs::write(path, data);
    }
}

#[derive(Debug, Deserialize)]
struct GithubContributor {
    login: String,
    avatar_url: String,
    html_url: String,
}

async fn fetch_github_contributors() -> Result<Vec<Contributor>, String> {
    let client = reqwest::Client::new();

    let resp = client
        .get(format!(
            "https://api.github.com/repos/{}/contributors",
            GITHUB_REPO
        ))
        .query(&[("per_page", "20")])
        .header("User-Agent", "Bogominer")
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| format!("github request failed: {}", e))?;

    let status = resp.status();
    let body = resp
        .text()
        .await
        .map_err(|e| format!("body error: {}", e))?;
    eprintln!(
        "[github] status={} body={}",
        status,
        &body[..body.len().min(500)]
    );

    if !status.is_success() {
        return Err(format!("github error: {} {}", status, body));
    }

    let raw: Vec<GithubContributor> =
        serde_json::from_str(&body).map_err(|e| format!("github parse failed: {}", e))?;

    eprintln!("[github] found {} contributors", raw.len());

    Ok(raw
        .into_iter()
        .take(12)
        .map(|c| Contributor {
            name: c.login,
            avatar_url: c.avatar_url,
            web_url: c.html_url,
        })
        .collect())
}
