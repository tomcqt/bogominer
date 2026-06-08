use super::state::{AccountView, AppState, Contributor, RuntimeStats};
use crate::backend::{api, pool::Pool};
use crate::misc::validate_nick;
use serde::Deserialize;
use std::{
    fs,
    path::PathBuf,
    sync::atomic::Ordering,
    time::{Duration, SystemTime},
};
use tauri::State;

const GITLAB_PROJECT_PATH: &str = "ttomcat/bogominer";
const CONTRIBUTORS_CACHE_MAX_AGE: Duration = Duration::from_secs(60 * 60 * 24);

#[derive(Debug, serde::Serialize)]
pub struct AppView {
    pub account: AccountView,
    pub version: String,
}

#[derive(Debug, Deserialize)]
pub struct NewAccountRequest {
    pub nickname: String,
}

#[derive(Debug, Deserialize)]
pub struct ExistingAccountRequest {
    pub uuid: String,
    pub nickname: String,
    pub recovery_code: String,
}

#[tauri::command]
pub fn get_app_state(state: State<AppState>) -> AppView {
    let config = state.config.lock();
    AppView {
        account: AccountView {
            uuid: config.uuid.clone(),
            nickname: config.nickname.clone(),
            has_recovery_code: config.recovery_code.as_ref().is_some_and(|s| !s.is_empty()),
        },
        version: env!("CARGO_PKG_VERSION").to_string(),
    }
}

#[tauri::command]
pub fn save_new_account(req: NewAccountRequest, state: State<AppState>) -> Result<AppView, String> {
    let nick = validate_nick(&req.nickname)?;
    {
        let mut config = state.config.lock();
        if config.uuid.is_none() {
            config.uuid = Some(api::generate_uuid());
        }
        config.nickname = Some(nick);
        config.save();
    }
    Ok(get_app_state(state))
}

#[tauri::command]
pub fn save_existing_account(
    req: ExistingAccountRequest,
    state: State<AppState>,
) -> Result<AppView, String> {
    let nick = validate_nick(&req.nickname)?;
    let uuid = req.uuid.trim().to_string();
    let recovery_code = req.recovery_code.trim().to_string();

    if uuid.len() < 16 || uuid.len() > 64 {
        return Err("uuid must be 16-64 characters".into());
    }
    if !uuid.chars().all(|c| c.is_ascii_hexdigit() || c == '-') {
        return Err("uuid must be hex characters or dashes".into());
    }
    if recovery_code.is_empty() {
        return Err("recovery code is required".into());
    }

    {
        let mut config = state.config.lock();
        config.uuid = Some(uuid);
        config.nickname = Some(nick);
        config.recovery_code = Some(recovery_code);
        config.save();
    }
    Ok(get_app_state(state))
}

#[tauri::command]
pub fn start_mining(cpu_target: f64, state: State<AppState>) -> Result<(), String> {
    let config = state.config.lock().clone();
    let uuid = config.uuid.ok_or("missing uuid")?;
    let nickname = config.nickname.ok_or("missing nickname")?;
    let code = config.recovery_code.unwrap_or_default();

    let mut pool = state.pool.lock();
    if pool.is_none() {
        *pool = Some(Pool::new(state.stats.clone()));
    }

    let _guard = state.rt.enter();
    pool.as_mut()
        .unwrap()
        .start(&uuid, &nickname, &code, cpu_target.clamp(0.05, 1.0));
    Ok(())
}

#[tauri::command]
pub fn stop_mining(state: State<AppState>) {
    if let Some(pool) = state.pool.lock().as_mut() {
        pool.stop();
    }
}

#[tauri::command]
pub fn set_cpu_target(cpu_target: f64, state: State<AppState>) -> Result<(), String> {
    let config = state.config.lock().clone();
    let uuid = config.uuid.ok_or("missing uuid")?;
    let nickname = config.nickname.ok_or("missing nickname")?;
    let code = config.recovery_code.unwrap_or_default();

    if let Some(pool) = state.pool.lock().as_mut() {
        pool.set_cpu_target(cpu_target.clamp(0.05, 1.0), &uuid, &nickname, &code);
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

    let contributors = fetch_gitlab_contributors().await?;
    write_contributors_cache(&contributors);
    Ok(contributors)
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
struct GitLabProject {
    id: u64,
}

#[derive(Debug, Deserialize)]
struct GitLabContributor {
    name: String,
    email: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GitLabUser {
    name: String,
    avatar_url: Option<String>,
    web_url: String,
}

async fn fetch_gitlab_contributors() -> Result<Vec<Contributor>, String> {
    let client = reqwest::Client::new();
    let encoded = urlencoding::encode(GITLAB_PROJECT_PATH);

    let project: GitLabProject = client
        .get(format!("https://gitlab.com/api/v4/projects/{}", encoded))
        .send()
        .await
        .map_err(|e| format!("gitlab project request failed: {}", e))?
        .error_for_status()
        .map_err(|e| format!("gitlab project error: {}", e))?
        .json()
        .await
        .map_err(|e| format!("gitlab project parse failed: {}", e))?;

    let raw: Vec<GitLabContributor> = client
        .get(format!(
            "https://gitlab.com/api/v4/projects/{}/repository/contributors",
            project.id
        ))
        .query(&[("per_page", "20")])
        .send()
        .await
        .map_err(|e| format!("gitlab contributors request failed: {}", e))?
        .error_for_status()
        .map_err(|e| format!("gitlab contributors error: {}", e))?
        .json()
        .await
        .map_err(|e| format!("gitlab contributors parse failed: {}", e))?;

    let mut out = Vec::new();
    for c in raw.into_iter().take(12) {
        let user = find_gitlab_user(&client, &c).await;
        if let Some(user) = user {
            out.push(Contributor {
                name: user.name,
                avatar_url: user.avatar_url.unwrap_or_default(),
                web_url: user.web_url,
            });
        }
    }
    Ok(out)
}

async fn find_gitlab_user(client: &reqwest::Client, c: &GitLabContributor) -> Option<GitLabUser> {
    let search = c
        .email
        .as_ref()
        .and_then(|e| e.split('@').next())
        .filter(|s| !s.is_empty())
        .unwrap_or(&c.name);

    let users: Vec<GitLabUser> = client
        .get("https://gitlab.com/api/v4/users")
        .query(&[("search", search)])
        .send()
        .await
        .ok()?
        .error_for_status()
        .ok()?
        .json()
        .await
        .ok()?;

    users.into_iter().next()
}
