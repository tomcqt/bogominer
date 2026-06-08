use crate::backend::stats::Stats;
use crate::backend::worker::{self, WorkerCmd};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tokio::sync::mpsc;

pub struct Pool {
    worker: Option<mpsc::UnboundedSender<WorkerCmd>>,
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
            _error_rx,
            error_tx,
            code_rx,
            code_tx,
            stats,
        }
    }

    pub fn start(&mut self, uuid: &str, nickname: &str, code: &str, cpu_target: f64) {
        self.stop();
        self.stats.reset_session();

        let cmd_tx = worker::spawn_worker(
            uuid.to_string(),
            nickname.to_string(),
            code.to_string(),
            self.stats.clone(),
            cpu_target,
            self.error_tx.clone(),
            self.code_tx.clone(),
        );
        self.worker = Some(cmd_tx);
    }

    pub fn stop(&mut self) {
        if let Some(w) = self.worker.take() {
            let _ = w.send(WorkerCmd::Stop);
        }
        self.stats.active_workers.store(0, Ordering::Relaxed);
        self.stats.solver_threads.store(0, Ordering::Relaxed);
    }

    pub fn set_cpu_target(&mut self, cpu_target: f64, uuid: &str, nickname: &str, code: &str) {
        let cores = num_cpus::get().max(1).min(16);
        let old_threads = self.stats.solver_threads.load(Ordering::Relaxed) as usize;
        let new_threads = ((cpu_target * cores as f64).ceil() as usize)
            .max(1)
            .min(cores);

        if old_threads != new_threads && self.worker.is_some() {
            self.stop();
            self.start(uuid, nickname, code, cpu_target);
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
