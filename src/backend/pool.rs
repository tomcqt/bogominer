use crate::backend::stats::Stats;
use crate::backend::worker::{self, Backend, WorkerCmd};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, oneshot}; // "like niko oneshot?" - @PlutonicBird (github)

pub struct Pool {
    worker: Option<mpsc::UnboundedSender<WorkerCmd>>,
    worker_done: Option<oneshot::Receiver<()>>,
    _error_rx: mpsc::UnboundedReceiver<String>,
    error_tx: mpsc::UnboundedSender<String>,
    code_rx: mpsc::UnboundedReceiver<String>,
    code_tx: mpsc::UnboundedSender<String>,
    pub stats: Arc<Stats>,
}

impl Pool {
    pub fn new(stats: Arc<Stats>) -> Self {
        let (error_tx, _error_rx) = mpsc::unbounded_channel();
        let (code_tx, code_rx) = mpsc::unbounded_channel();
        Self {
            worker: None,
            worker_done: None,
            _error_rx,
            error_tx,
            code_rx,
            code_tx,
            stats,
        }
    }

    pub fn start(
        &mut self,
        uuid: &str,
        nickname: &str,
        code: &str,
        cpu_target: f64,
        backend: Backend,
    ) {
        eprintln!(
            "[pool] start (uuid={}...{}, nick={:?}, cpu={}, backend={:?})",
            &uuid[..8.min(uuid.len())],
            &uuid[uuid.len().saturating_sub(4)..],
            nickname,
            cpu_target,
            backend
        );
        self.stop();
        self.stats.reset_session();

        let (cmd_tx, done_rx) = worker::spawn_worker(
            uuid.to_string(),
            nickname.to_string(),
            code.to_string(),
            self.stats.clone(),
            cpu_target,
            backend,
            self.error_tx.clone(),
            self.code_tx.clone(),
        );
        self.worker = Some(cmd_tx);
        self.worker_done = Some(done_rx);
    }

    pub fn stop(&mut self) {
        eprintln!("[pool] stop requested");
        if let Some(w) = self.worker.take() {
            let _ = w.send(WorkerCmd::Stop);
            eprintln!("[pool] sent stop to worker, waiting for exit");
        }
        if let Some(mut done_rx) = self.worker_done.take() {
            let deadline = Instant::now() + Duration::from_secs(5);
            while Instant::now() < deadline {
                if done_rx.try_recv().is_ok() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            if Instant::now() < deadline {
                eprintln!("[pool] worker exited properly");
            } else {
                eprintln!("[pool] worker exit timed out after 5s");
            }
        }
        self.stats.active_workers.store(0, Ordering::Relaxed);
        self.stats.solver_threads.store(0, Ordering::Relaxed);
        eprintln!("[pool] stop complete");
    }

    pub fn set_cpu_target(
        &mut self,
        cpu_target: f64,
        uuid: &str,
        nickname: &str,
        code: &str,
        backend: Backend,
    ) {
        let cores = num_cpus::get().clamp(1, 16);
        let old_threads = self.stats.solver_threads.load(Ordering::Relaxed) as usize;
        let new_threads = ((cpu_target * cores as f64).ceil() as usize)
            .max(1)
            .min(cores);

        eprintln!(
            "[pool] set_cpu_target={} old_threads={} new_threads={}",
            cpu_target, old_threads, new_threads
        );
        if old_threads != new_threads && self.worker.is_some() {
            eprintln!("[pool] thread count changed, rebuilding");
            self.stop();
            self.start(uuid, nickname, code, cpu_target, backend);
        } else if let Some(w) = &self.worker {
            let _ = w.send(WorkerCmd::SetCpuTarget(cpu_target));
        }
    }

    pub fn _poll_error(&mut self) -> Option<String> {
        self._error_rx.try_recv().ok()
    }

    pub fn poll_recovery_code(&mut self) -> Option<String> {
        self.code_rx.try_recv().ok()
    }

    pub fn is_running(&self) -> bool {
        self.stats.active_workers.load(Ordering::Relaxed) > 0
    }

    pub fn _solver_thread_count(&self) -> usize {
        self.stats.solver_threads.load(Ordering::Relaxed) as usize
    }
}

impl Drop for Pool {
    fn drop(&mut self) {
        self.stop();
    }
}
