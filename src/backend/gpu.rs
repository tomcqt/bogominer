//! external gpu worker integration (bogo-turbo).
//!
//! when gpu mining is enabled, instead of spawning the native cpu solver pool
//! we launch the standalone `bogo_gpu_turbo` cuda worker (see
//! https://github.com/Mafiosoweb1/bogo-turbo) as a child process. it keeps its
//! own websocket connection to the server, so the cpu pool must be stopped
//! first — the server allows a single connection per account.
//!
//! the worker is started in `--tester` mode, where it logs every protocol
//! frame to stderr as `[12.345s] RECV {...}` / `SEND conn=0 {...}` lines. we
//! parse those to feed the same `Stats` the cpu pool uses, so the dashboard
//! works unchanged.

use crate::backend::config::Config;
use crate::backend::stats::Stats;
use parking_lot::Mutex;
use serde_json::Value;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Instant;

pub const WORKER_EXE: &str = if cfg!(windows) {
    "bogo_gpu_turbo.exe"
} else {
    "bogo_gpu_turbo"
};

// the worker can be fetched automatically so nobody has to place files by
// hand. we track the repo's default branch so the newest worker build is
// always fetched. trade-off vs. a pinned commit: the downloaded bytes can
// change under us — acceptable here since it's our own first-party worker repo.
const WORKER_SOURCE_REPO: &str = "Mafiosoweb1/bogo-turbo";
const WORKER_SOURCE_REF: &str = "main";
#[cfg(windows)]
const WORKER_FILES: &[&str] = &[
    "bogo_gpu_turbo.exe",
    "msvcp140.dll",
    "vcruntime140.dll",
    "vcruntime140_1.dll",
    "z.dll",
];
// sidecar file written next to the worker recording the exe's http etag (the
// git blob hash raw.githubusercontent serves). on launch we compare it against
// the live etag to decide whether a newer build needs downloading.
#[cfg(windows)]
const WORKER_VERSION_FILE: &str = "bogo_turbo.version";

/// where the auto-downloaded worker lives (app data dir, survives updates).
pub fn downloaded_worker_dir() -> Option<PathBuf> {
    directories::ProjectDirs::from("sh", "tomcat", "bogominer")
        .map(|dirs| dirs.data_dir().join("gpu"))
}

/// directory the running bogominer executable lives in. the worker is dropped
/// here so it sits right next to the exe (the build/output folder during dev)
/// and is picked up first by `find_worker`.
pub fn exe_dir() -> Option<PathBuf> {
    Some(std::env::current_exe().ok()?.parent()?.to_path_buf())
}

/// can we create files in `dir`? used to decide whether the worker can be
/// dropped next to the exe — it can't when installed read-only (Program Files).
fn dir_is_writable(dir: &Path) -> bool {
    if std::fs::create_dir_all(dir).is_err() {
        return false;
    }
    let probe = dir.join(".bogominer_write_test");
    match std::fs::write(&probe, b"") {
        Ok(()) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

/// where the auto-downloaded worker should be written: right next to the exe
/// when that's writable, otherwise the app data dir as a fallback.
pub fn auto_download_dir() -> Option<PathBuf> {
    if let Some(dir) = exe_dir() {
        if dir_is_writable(&dir) {
            return Some(dir);
        }
    }
    downloaded_worker_dir()
}

/// resolve the gpu worker binary: explicit config path first, then next to
/// the bogominer executable, a `gpu/` subfolder beside it, and finally the
/// auto-download location.
pub fn find_worker(config: &Config) -> Option<PathBuf> {
    if let Some(p) = config.gpu_worker_path.as_deref() {
        let p = p.trim();
        if !p.is_empty() {
            let path = PathBuf::from(p);
            if path.is_file() {
                return Some(path);
            }
            return None; // an explicit path that doesn't exist is an error, not a fallthrough
        }
    }
    let exe_dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    let mut candidates = vec![
        exe_dir.join(WORKER_EXE),
        exe_dir.join("gpu").join(WORKER_EXE),
    ];
    if let Some(dir) = downloaded_worker_dir() {
        candidates.push(dir.join(WORKER_EXE));
    }
    candidates.into_iter().find(|c| c.is_file())
}

#[cfg(windows)]
fn worker_url(file: &str) -> String {
    format!(
        "https://raw.githubusercontent.com/{}/{}/dist/{}",
        WORKER_SOURCE_REPO, WORKER_SOURCE_REF, file
    )
}

/// recorded version (the worker exe's etag) of the worker installed in `dir`.
#[cfg(windows)]
fn read_worker_version(dir: &Path) -> Option<String> {
    std::fs::read_to_string(dir.join(WORKER_VERSION_FILE))
        .ok()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
}

#[cfg(windows)]
fn write_worker_version(dir: &Path, etag: &str) {
    if !etag.is_empty() {
        let _ = std::fs::write(dir.join(WORKER_VERSION_FILE), etag);
    }
}

/// download every worker file into `dir` (written to `.part` first and renamed
/// only when complete) and return the exe's http etag for version tracking.
#[cfg(windows)]
async fn fetch_worker_files(client: &reqwest::Client, dir: &Path) -> Result<String, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("could not create {:?}: {}", dir, e))?;

    let mut exe_etag = String::new();
    for file in WORKER_FILES {
        let url = worker_url(file);
        eprintln!("[gpu] downloading {}", url);
        let resp = client
            .get(&url)
            .send()
            .await
            .and_then(|r| r.error_for_status())
            .map_err(|e| format!("download of {} failed: {}", file, e))?;
        if *file == WORKER_EXE {
            if let Some(etag) = resp.headers().get(reqwest::header::ETAG) {
                exe_etag = etag.to_str().unwrap_or("").to_owned();
            }
        }
        let bytes = resp
            .bytes()
            .await
            .map_err(|e| format!("download of {} failed: {}", file, e))?;

        let tmp = dir.join(format!("{}.part", file));
        std::fs::write(&tmp, &bytes).map_err(|e| format!("could not write {}: {}", file, e))?;
        std::fs::rename(&tmp, dir.join(file))
            .map_err(|e| format!("could not finalize {}: {}", file, e))?;
        eprintln!("[gpu] saved {} ({} bytes)", file, bytes.len());
    }
    Ok(exe_etag)
}

/// fetch the prebuilt worker (~2 MB, exe + runtime dlls) into the app data dir
/// and record its version so later launches can tell when it goes stale.
#[cfg(windows)]
pub async fn download_worker() -> Result<PathBuf, String> {
    let dir = auto_download_dir().ok_or("could not resolve a writable worker dir")?;
    let client = reqwest::Client::new();
    let etag = fetch_worker_files(&client, &dir).await?;
    write_worker_version(&dir, &etag);
    Ok(dir.join(WORKER_EXE))
}

/// check whether the worker installed at `exe_path` is still the latest build
/// and re-download it in place if not. the check is a cheap HEAD request whose
/// etag is compared to the one recorded at download time. best-effort: any
/// network/io failure leaves the existing worker untouched (only logged).
#[cfg(windows)]
async fn update_worker_if_outdated(exe_path: &Path) {
    let Some(dir) = exe_path.parent() else { return };
    let client = reqwest::Client::new();

    let remote_etag = match client.head(worker_url(WORKER_EXE)).send().await {
        Ok(resp) => resp
            .headers()
            .get(reqwest::header::ETAG)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned),
        Err(e) => {
            eprintln!("[gpu] update check failed (keeping current worker): {}", e);
            return;
        }
    };
    let Some(remote_etag) = remote_etag else {
        eprintln!("[gpu] could not read remote worker etag — keeping current worker");
        return;
    };

    if read_worker_version(dir).as_deref() == Some(remote_etag.as_str()) {
        eprintln!("[gpu] worker is up to date");
        return;
    }

    eprintln!("[gpu] newer worker available — updating in place at {:?}", dir);
    match fetch_worker_files(&client, dir).await {
        Ok(etag) => {
            write_worker_version(dir, &etag);
            eprintln!("[gpu] worker updated");
        }
        Err(e) => eprintln!("[gpu] worker update failed (keeping current worker): {}", e),
    }
}

#[cfg(not(windows))]
pub async fn download_worker() -> Result<PathBuf, String> {
    Err("the prebuilt gpu worker is windows-only — build bogo-turbo from source and set its path in settings".into())
}

/// startup hook: make sure an up-to-date gpu worker is on disk. if none is
/// found it's downloaded next to the executable; if one is present its version
/// is checked against the latest published build and re-downloaded if stale, so
/// it's ready the moment gpu acceleration is enabled. an explicit, user-set
/// worker path is respected (never downloaded over). best-effort — failures are
/// only logged, since gpu mining is opt-in.
pub async fn ensure_worker_present() {
    let config = Config::load();

    let has_explicit_path = config
        .gpu_worker_path
        .as_deref()
        .map(str::trim)
        .is_some_and(|p| !p.is_empty());
    if has_explicit_path {
        return;
    }

    if let Some(found) = find_worker(&config) {
        #[cfg(windows)]
        {
            eprintln!("[gpu] worker present at {:?}, checking for a newer version", found);
            update_worker_if_outdated(&found).await;
        }
        #[cfg(not(windows))]
        {
            eprintln!("[gpu] worker already present at {:?}, skipping auto-download", found);
        }
        return;
    }

    eprintln!("[gpu] worker not found — auto-downloading next to the executable");
    match download_worker().await {
        Ok(path) => eprintln!("[gpu] auto-download complete: {:?}", path),
        Err(e) => eprintln!("[gpu] auto-download failed: {}", e),
    }
}

pub struct GpuWorker {
    child: Child,
    pub status: Arc<Mutex<String>>,
    stats: Arc<Stats>,
}

impl GpuWorker {
    pub fn spawn(
        path: &Path,
        uuid: &str,
        nickname: &str,
        code: &str,
        stats: Arc<Stats>,
    ) -> Result<Self, String> {
        eprintln!("[gpu] spawning worker {:?}", path);
        stats.reset_session();

        let mut cmd = Command::new(path);
        cmd.arg("--tester")
            .env("BOGO_UUID", uuid)
            .env("BOGO_NICKNAME", nickname)
            .env("BOGO_CODE", code)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(dir) = path.parent() {
            cmd.current_dir(dir);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }

        let mut child = cmd
            .spawn()
            .map_err(|e| format!("failed to start gpu worker: {}", e))?;

        let status = Arc::new(Mutex::new(String::from("starting gpu worker")));

        // drain stdout (tester mode only prints the startup banner there, but
        // a full pipe would block the child)
        if let Some(out) = child.stdout.take() {
            std::thread::spawn(
                move || {
                    for _ in BufReader::new(out).lines().map_while(Result::ok) {}
                },
            );
        }

        // stderr carries the protocol log — this is our stats feed
        if let Some(err) = child.stderr.take() {
            let stats = stats.clone();
            let status = status.clone();
            std::thread::spawn(move || {
                let mut last_credit = Instant::now();
                for line in BufReader::new(err).lines().map_while(Result::ok) {
                    handle_line(&line, &stats, &status, &mut last_credit);
                }
                eprintln!("[gpu] worker stderr closed (process exited)");
                stats.active_workers.store(0, Ordering::Relaxed);
                stats.solver_threads.store(0, Ordering::Relaxed);
                stats.rate.store(0, Ordering::Relaxed);
            });
        }

        stats.active_workers.store(1, Ordering::Relaxed);
        Ok(Self {
            child,
            status,
            stats,
        })
    }

    pub fn is_running(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    pub fn status_line(&self) -> String {
        self.status.lock().clone()
    }

    pub fn stop(&mut self) {
        eprintln!("[gpu] stopping worker");
        let _ = self.child.kill();
        let _ = self.child.wait();
        self.stats.active_workers.store(0, Ordering::Relaxed);
        self.stats.solver_threads.store(0, Ordering::Relaxed);
        self.stats.rate.store(0, Ordering::Relaxed);
        eprintln!("[gpu] worker stopped");
    }
}

impl Drop for GpuWorker {
    fn drop(&mut self) {
        self.stop();
    }
}

fn handle_line(line: &str, stats: &Stats, status: &Mutex<String>, last_credit: &mut Instant) {
    let Some(brace) = line.find('{') else {
        // non-protocol stderr lines: surface failures in the status
        if line.contains("[CUDA ERROR]") {
            *status.lock() = format!("error: {}", line.trim());
        } else if line.contains("Set BOGO_UUID") {
            *status.lock() = "error: gpu worker got no credentials".into();
        }
        return;
    };
    let head = &line[..brace];
    // RECV frames are truncated to 200 chars by the worker; a parse failure on
    // a long frame is expected — fall back to plucking the fields we need.
    let json: Option<Value> = serde_json::from_str(&line[brace..]).ok();

    if head.contains("RECV") {
        let msg_type = json
            .as_ref()
            .and_then(|j| j.get("type").and_then(Value::as_str).map(str::to_owned))
            .or_else(|| extract_str(line, "type"));
        match msg_type.as_deref() {
            Some("welcome") => {
                if let Some(lifetime) = get_u64(&json, line, "lifetime_shuffles") {
                    stats.lifetime_shuffles.store(lifetime, Ordering::Relaxed);
                }
                if let Some(atb) = get_u64(&json, line, "all_time_best") {
                    stats.all_time_best.store(atb as i32, Ordering::Relaxed);
                }
                *status.lock() = "connected; waiting for lease".into();
            }
            Some("job") => {
                if let Some(count) = get_u64(&json, line, "count") {
                    stats.lease_count.store(count, Ordering::Relaxed);
                    stats.lease_cursor.store(0, Ordering::Relaxed);
                }
                *status.lock() = "mining on gpu".into();
            }
            Some("credited") => {
                let now = Instant::now();
                let elapsed = now.duration_since(*last_credit).as_secs_f64();
                *last_credit = now;

                if let Some(credit) = get_u64(&json, line, "credit") {
                    stats.session_shuffles.fetch_add(credit, Ordering::Relaxed);
                    // the server's credited frame carries a rate; if truncation
                    // ate it, estimate from the credit cadence (~1 report/s)
                    let rate = get_u64(&json, line, "rate").unwrap_or_else(|| {
                        if elapsed > 0.05 {
                            (credit as f64 / elapsed) as u64
                        } else {
                            0
                        }
                    });
                    if rate > 0 {
                        stats.rate.store(rate, Ordering::Relaxed);
                    }
                }
                if let Some(lifetime) = get_u64(&json, line, "lifetime_shuffles") {
                    loop {
                        let old = stats.lifetime_shuffles.load(Ordering::Relaxed);
                        if lifetime <= old {
                            break;
                        }
                        if stats
                            .lifetime_shuffles
                            .compare_exchange_weak(
                                old,
                                lifetime,
                                Ordering::Relaxed,
                                Ordering::Relaxed,
                            )
                            .is_ok()
                        {
                            break;
                        }
                    }
                }
                if let Some(bb) = get_u64(&json, line, "batch_best") {
                    stats.note_batch_best(bb as i32);
                    stats._tick_second();
                }
                if let Some(atb) = get_u64(&json, line, "all_time_best") {
                    let old = stats.all_time_best.load(Ordering::Relaxed);
                    if atb as i32 > old {
                        stats.all_time_best.store(atb as i32, Ordering::Relaxed);
                    }
                }
            }
            Some("rejected") => {
                *status.lock() = "report rejected by server".into();
            }
            Some("banned") => {
                *status.lock() = "error: account banned".into();
            }
            Some("client_outdated") => {
                *status.lock() = "error: gpu worker outdated".into();
            }
            Some("contributions_closed") => {
                *status.lock() = "error: contributions closed".into();
            }
            _ => {}
        }
    } else if head.contains("SEND") {
        if let Some(json) = &json {
            if json.get("type").and_then(Value::as_str) == Some("result") {
                if let Some(done) = json.get("total_done").and_then(Value::as_u64) {
                    stats.lease_cursor.store(done, Ordering::Relaxed);
                }
            }
        }
    }
}

fn get_u64(json: &Option<Value>, line: &str, key: &str) -> Option<u64> {
    json.as_ref()
        .and_then(|j| j.get(key).and_then(Value::as_u64))
        .or_else(|| extract_u64(line, key))
}

/// pull `"key":123` out of a (possibly truncated) json line.
fn extract_u64(line: &str, key: &str) -> Option<u64> {
    let pat = format!("\"{}\":", key);
    let start = line.find(&pat)? + pat.len();
    let rest = line[start..].trim_start();
    let end = rest
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(rest.len());
    if end == 0 {
        return None;
    }
    rest[..end].parse().ok()
}

/// pull `"key":"value"` out of a (possibly truncated) json line.
fn extract_str(line: &str, key: &str) -> Option<String> {
    let pat = format!("\"{}\":\"", key);
    let start = line.find(&pat)? + pat.len();
    let rest = &line[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}
