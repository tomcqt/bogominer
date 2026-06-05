use crate::protocol::{HelloMsg, ResultMsg, ServerMsg, StopMsg};
use crate::solver;
use crate::stats::Stats;
use futures_util::{SinkExt, StreamExt};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;

const WS_URL: &str = "wss://bogo.swapjs.dev/ws";

#[derive(Debug)]
pub enum WorkerCmd {
    SetThrottle(f64),
    Stop,
}

pub fn spawn_worker(
    uuid: String,
    nickname: String,
    code: String,
    stats: Arc<Stats>,
    throttle: f64,
    on_error: mpsc::UnboundedSender<String>,
) -> mpsc::UnboundedSender<WorkerCmd> {
    let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();

    tokio::spawn(async move {
        let result: Result<(), String> =
            run_worker(uuid, nickname, code, stats.clone(), throttle, cmd_rx).await;
        stats.active_workers.fetch_sub(1, Ordering::Relaxed);
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
    initial_throttle: f64,
    mut cmd_rx: mpsc::UnboundedReceiver<WorkerCmd>,
) -> Result<(), String> {
    let (ws_stream, _response) = connect_async(WS_URL)
        .await
        .map_err(|e| format!("ws connect failed: {}", e))?;

    let (mut ws_tx, mut ws_rx) = ws_stream.split();

    let hello = HelloMsg::new(&uuid, &nickname, &code);
    let hello_json = serde_json::to_string(&hello).unwrap();
    ws_tx
        .send(Message::Text(hello_json.into()))
        .await
        .map_err(|e| format!("ws send hello failed: {}", e))?;

    let (result_tx, mut result_rx) = mpsc::unbounded_channel::<solver::BatchResult>();

    let throttle = Arc::new(std::sync::atomic::AtomicU64::new(
        initial_throttle.to_bits(),
    ));
    let throttle_clone = throttle.clone();

    let mut welcomed = false;

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
                    }
                    "job" => {
                        if !welcomed { continue; }
                        let seed = match &server_msg.seed {
                            Some(s) => s.clone(),
                            None => continue,
                        };
                        let batch_size = server_msg.batch_size.unwrap_or(100_000);
                        let tx = result_tx.clone();
                        let thr = throttle_clone.clone();

                        tokio::task::spawn_blocking(move || {
                            let thr_val = f64::from_bits(thr.load(Ordering::Relaxed));
                            let result = solver::run_batch(&seed, batch_size);

                            if thr_val < 0.999 {
                                let sleep_ms = result.elapsed * 1000.0
                                    * ((1.0 - thr_val) / thr_val);
                                if sleep_ms > 0.5 {
                                    std::thread::sleep(std::time::Duration::from_millis(
                                        sleep_ms as u64,
                                    ));
                                }
                            }

                            let _ = tx.send(result);
                        });
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

            Some(result) = result_rx.recv() => {
                let msg = ResultMsg::new(
                    &result.seed,
                    result.total_done,
                    result.best_correct,
                    result.best_arr.to_vec(),
                    result.elapsed,
                );
                let json = serde_json::to_string(&msg).unwrap();
                if let Err(e) = ws_tx.send(Message::Text(json.into())).await {
                    return Err(format!("ws send result failed: {}", e));
                }
            }

            Some(cmd) = cmd_rx.recv() => {
                match cmd{
                    WorkerCmd::SetThrottle(t) => {
                        throttle.store(t.to_bits(), Ordering::Relaxed);
                    }
                    WorkerCmd::Stop => {
                        let stop = StopMsg::new();
                        let json = serde_json::to_string(&stop).unwrap();
                        let _ = ws_tx.send(Message::Text(json.into())).await;
                        let _ = ws_tx.close().await;
                        return Ok(());
                    }
                }
            }
        }
    }
}
