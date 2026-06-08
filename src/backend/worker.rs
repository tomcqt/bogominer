use crate::backend::protocol::{HelloMsg, ResultMsg, ServerMsg, StopMsg};
use crate::backend::solver;
use crate::backend::stats::Stats;
use futures_util::{SinkExt, StreamExt};
use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, watch};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;

const WS_URL: &str = "wss://bogo.swapjs.dev/ws";
const CHUNK_SIZE: u64 = 2_000_000;

#[derive(Debug)]
pub enum WorkerCmd {
    SetCpuTarget(f64),
    Stop,
}

struct LeaseState {
    seed: u64,
    seed_str: String,
    count: u64,
    cursor: AtomicU64,
    total_done: AtomicU64,
    best_correct: AtomicI32,
    best_result: Mutex<(u64, [u8; solver::N])>,
    exhausted: AtomicBool,
}

impl LeaseState {
    fn new(seed_str: String, count: u64) -> Self {
        let seed: u64 = seed_str.parse().unwrap_or(0);
        Self {
            seed,
            seed_str,
            count,
            cursor: AtomicU64::new(0),
            total_done: AtomicU64::new(0),
            best_correct: AtomicI32::new(-1),
            best_result: Mutex::new((0, [0u8; solver::N])),
            exhausted: AtomicBool::new(false),
        }
    }

    fn claim_chunk(&self, size: u64) -> Option<(u64, u64)> {
        loop {
            let cur = self.cursor.load(Ordering::Relaxed);
            if cur >= self.count {
                self.exhausted.store(true, Ordering::Relaxed);
                return None;
            }
            let hi = (cur + size).min(self.count);
            if self
                .cursor
                .compare_exchange_weak(cur, hi, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
            {
                return Some((cur, hi));
            }
        }
    }

    fn report_chunk(&self, result: &solver::RangeResult) {
        self.total_done.fetch_add(result.count, Ordering::Relaxed);

        if result.best_correct > self.best_correct.load(Ordering::Relaxed) {
            let mut best = self.best_result.lock();
            if result.best_correct > self.best_correct.load(Ordering::Relaxed) {
                self.best_correct
                    .store(result.best_correct, Ordering::Relaxed);
                *best = (result.best_index, result.best_arr);
            }
        }
    }

    fn snapshot(&self) -> (u64, i32, u64, [u8; solver::N]) {
        let total = self.total_done.load(Ordering::Relaxed);
        let bc = self.best_correct.load(Ordering::Relaxed);
        let best = self.best_result.lock();
        (total, bc, best.0, best.1)
    }
}

pub fn spawn_worker(
    uuid: String,
    nickname: String,
    code: String,
    stats: Arc<Stats>,
    cpu_target: f64,
    on_error: mpsc::UnboundedSender<String>,
    on_code: mpsc::UnboundedSender<String>,
) -> mpsc::UnboundedSender<WorkerCmd> {
    let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();

    let stats_inner = stats.clone();
    tokio::spawn(async move {
        let result: Result<(), String> = run_worker(
            uuid,
            nickname,
            code,
            stats_inner.clone(),
            cpu_target,
            cmd_rx,
            on_code,
        )
        .await;
        stats_inner.active_workers.fetch_sub(1, Ordering::Relaxed);
        if let Err(e) = result {
            let _ = on_error.send(e);
        }
    });

    stats.active_workers.fetch_add(1, Ordering::Relaxed);
    cmd_tx
}

async fn run_worker(
    uuid: String,
    nickname: String,
    code: String,
    stats: Arc<Stats>,
    initial_cpu_target: f64,
    mut cmd_rx: mpsc::UnboundedReceiver<WorkerCmd>,
    on_code: mpsc::UnboundedSender<String>,
) -> Result<(), String> {
    let (ws_stream, _) = connect_async(WS_URL)
        .await
        .map_err(|e| format!("ws connect failed: {}", e))?;

    let (mut ws_tx, mut ws_rx) = ws_stream.split();

    let hello = HelloMsg::new(&uuid, &nickname, &code);
    let hello_json = serde_json::to_string(&hello).unwrap();
    ws_tx
        .send(Message::Text(hello_json.into()))
        .await
        .map_err(|e| format!("ws send hello failed: {}", e))?;

    let cpu_target = Arc::new(AtomicU64::new(initial_cpu_target.to_bits()));

    let lease: Arc<Mutex<Option<Arc<LeaseState>>>> = Arc::new(Mutex::new(None));

    let (stop_tx, _) = watch::channel(false);

    let num_threads = {
        let cores = num_cpus::get().max(1).min(16);
        let target = f64::from_bits(cpu_target.load(Ordering::Relaxed));
        ((target * cores as f64).ceil() as usize).max(1).min(cores)
    };

    let solver_handles = spawn_solvers(
        num_threads,
        lease.clone(),
        stats.clone(),
        cpu_target.clone(),
        stop_tx.subscribe(),
    );
    stats
        .solver_threads
        .store(num_threads as u64, Ordering::Relaxed);

    let mut report_interval = tokio::time::interval(Duration::from_secs(1));
    report_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    let mut welcomed = false;
    let mut last_reported_total: u64 = 0;

    loop {
        tokio::select! {
            ws_msg = ws_rx.next() => {
                let Some(msg) = ws_msg else {
                    return Err("server closed connection".into());
                };
                let msg = msg.map_err(|e| format!("ws error: {}", e))?;

                let Message::Text(text) = msg else { continue };
                let server_msg: ServerMsg = match serde_json::from_str(&text) {
                    Ok(m) => m,
                    Err(_) => continue,
                };

                match server_msg.msg_type.as_str() {
                    "welcome" => {
                        welcomed = true;
                        if let Some(lifetime) = server_msg.lifetime_shuffles {
                            stats.lifetime_shuffles.store(lifetime, Ordering::Relaxed);
                        }
                        if let Some(atb) = server_msg.all_time_best {
                            stats.all_time_best.store(atb as i32, Ordering::Relaxed);
                        }
                        if let Some(c) = &server_msg.code {
                            if !c.is_empty() {
                                let _ = on_code.send(c.clone());
                            }
                        }
                    }
                    "job" => {
                        if !welcomed { continue; }
                        let seed_str = match &server_msg.seed {
                            Some(s) => s.clone(),
                            None => continue,
                        };
                        let count = server_msg.count.unwrap_or(0);
                        let new_lease = Arc::new(LeaseState::new(seed_str, count));

                        if let Some(old) = lease.lock().take() {
                            old.exhausted.store(true, Ordering::Relaxed);
                        }
                        *lease.lock() = Some(new_lease.clone());
                        last_reported_total = 0;

                        stats.lease_count.store(count, Ordering::Relaxed);
                        stats.lease_cursor.store(0, Ordering::Relaxed);
                    }
                    "credited" => {
                        if let Some(credit) = server_msg.credit {
                            stats.session_shuffles.fetch_add(credit, Ordering::Relaxed);
                        }
                        if let Some(lifetime) = server_msg.lifetime_shuffles {
                            loop {
                                let old = stats.lifetime_shuffles.load(Ordering::Relaxed);
                                if lifetime <= old { break; }
                                if stats.lifetime_shuffles
                                    .compare_exchange_weak(old, lifetime, Ordering::Relaxed, Ordering::Relaxed)
                                    .is_ok() { break; }
                            }
                        }
                        if let Some(rate) = server_msg.rate {
                            stats.rate.store(rate, Ordering::Relaxed);
                        }
                        if let Some(best) = server_msg.batch_best {
                            stats.note_batch_best(best as i32);
                        }
                        if let Some(atb) = server_msg.all_time_best {
                            let old = stats.all_time_best.load(Ordering::Relaxed);
                            if atb as i32 > old {
                                stats.all_time_best.store(atb as i32, Ordering::Relaxed);
                            }
                        }
                        if let Some(sb) = server_msg.session_best {
                            stats.note_batch_best(sb as i32);
                        }
                    }
                    "rejected" => {
                        let reason = server_msg.reason.unwrap_or_else(|| "unknown".into());
                        eprintln!("[worker] rejected: {}", reason);
                    }
                    "client_outdated" => {
                        return Err("client outdated! make an issue in the github/gitlab".into());
                    }
                    "banned" => {
                        let reason = server_msg.reason.unwrap_or_else(|| "unknown".into());
                        return Err(format!("banned: {}", reason));
                    }
                    "contributions_closed" => {
                        return Err("contributions closed".into());
                    }
                    _ => {}
                }
            }

            _ = report_interval.tick() => {
                let lease_snapshot = lease.lock().clone();
                if let Some(l) = lease_snapshot {
                    let (total, best_correct, best_index, best_arr) = l.snapshot();

                    stats.lease_cursor.store(total, Ordering::Relaxed);

                    if total > last_reported_total && best_correct >= 0 {
                        let msg = ResultMsg::new(
                            &l.seed_str,
                            total,
                            best_correct,
                            best_arr.to_vec(),
                            best_index,
                        );
                        let json = serde_json::to_string(&msg).unwrap();
                        if let Err(e) = ws_tx.send(Message::Text(json.into())).await {
                            return Err(format!("ws send result failed: {}", e));
                        }
                        last_reported_total = total;
                    }
                }
            }

            Some(cmd) = cmd_rx.recv() => {
                match cmd{
                    WorkerCmd::SetCpuTarget(t) => {
                        cpu_target.store(t.to_bits(), Ordering::Relaxed);
                    }
                    WorkerCmd::Stop => {
                        let _ = stop_tx.send(true);
                        let stop = StopMsg::new();
                        let json = serde_json::to_string(&stop).unwrap();
                        let _ = ws_tx.send(Message::Text(json.into())).await;
                        let _ = ws_tx.close().await;
                        for h in solver_handles {
                            let _ = h.join();
                        }
                        stats.solver_threads.store(0, Ordering::Relaxed);
                        return Ok(());
                    }
                }
            }
        }
    }
}

fn spawn_solvers(
    n: usize,
    lease: Arc<Mutex<Option<Arc<LeaseState>>>>,
    stats: Arc<Stats>,
    cpu_target: Arc<AtomicU64>,
    stop_rx: watch::Receiver<bool>,
) -> Vec<std::thread::JoinHandle<()>> {
    let mut handles = Vec::with_capacity(n);
    for _ in 0..n {
        let lease = lease.clone();
        let stats = stats.clone();
        let cpu_target = cpu_target.clone();
        let stop_rx = stop_rx.clone();
        let handle = std::thread::spawn(move || loop {
            if *stop_rx.borrow() {
                return;
            }

            let current = lease.lock().clone();
            let Some(l) = current else {
                std::thread::sleep(Duration::from_millis(50));
                continue;
            };

            let Some((lo, hi)) = l.claim_chunk(CHUNK_SIZE) else {
                std::thread::sleep(Duration::from_millis(100));
                continue;
            };

            let t0 = Instant::now();
            let result = solver::run_range(l.seed, lo, hi);
            let work_ms = t0.elapsed().as_secs_f64() * 1000.0;

            if result.best_correct >= 0 {
                stats.note_batch_best(result.best_correct);
            }

            let chunk_rate = if work_ms > 0.0 {
                (result.count as f64 / (work_ms / 1000.0)) as u64
            } else {
                0
            };

            stats.rate.store(
                stats.rate.load(Ordering::Relaxed).max(chunk_rate),
                Ordering::Relaxed,
            );

            l.report_chunk(&result);

            let target = f64::from_bits(cpu_target.load(Ordering::Relaxed));
            if target < 0.999 {
                let sleep_ms = work_ms * ((1.0 - target) / target);
                if sleep_ms > 0.5 {
                    let _ = stop_rx.has_changed();
                    std::thread::sleep(Duration::from_millis(sleep_ms as u64));
                }
            }
        });
        handles.push(handle);
    }
    handles
}
