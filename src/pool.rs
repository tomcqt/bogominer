use crate::stats::Stats;
use crate::worker::{self, WorkerCmd};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tokio::sync::mpsc;

pub struct Pool {
    workers: Vec<mpsc::UnboundedSender<WorkerCmd>>,
    error_rx: mpsc::UnboundedReceiver<String>,
    error_tx: mpsc::UnboundedSender<String>,
    pub stats: Arc<Stats>,
}

impl Pool {
    pub fn new(stats: Arc<Stats>) -> Self {
        let (error_tx, error_rx) = mpsc::unbounded_channel();
        Self {
            workers: Vec::new(),
            error_rx,
            error_tx,
            stats,
        }
    }

    pub fn pool_size(cpu_target: f64) -> usize {
        let cores = num_cpus::get().max(1).min(16);
        let n = (cpu_target * cores as f64).ceil() as usize;
        n.max(1).min(cores)
    }

    pub fn per_worker_throttle(cpu_target: f64) -> f64 {
        let cores = num_cpus::get().max(1).min(16);
        let workers = Self::pool_size(cpu_target);
        let t = (cpu_target * cores as f64) / workers as f64;
        t.clamp(0.05, 1.0)
    }

    pub fn start(&mut self, uuid: &str, nickname: &str, code: &str, cpu_target: f64) {
        self.stop();
        self.stats.reset_session();

        let count = Self::pool_size(cpu_target);
        let throttle = Self::per_worker_throttle(cpu_target);

        for _ in 0..count {
            let cmd_tx = worker::spawn_worker(
                uuid.to_string(),
                nickname.to_string(),
                code.to_string(),
                self.stats.clone(),
                throttle,
                self.error_tx.clone(),
            );
            self.workers.push(cmd_tx);
        }
    }

    pub fn stop(&mut self) {
        for w in self.workers.drain(..) {
            let _ = w.send(WorkerCmd::Stop);
        }
        self.stats.active_workers.store(0, Ordering::Relaxed);
    }

    pub fn set_cpu_target(&mut self, cpu_target: f64, uuid: &str, nickname: &str, code: &str) {
        let new_count = Self::pool_size(cpu_target);
        let new_throttle = Self::per_worker_throttle(cpu_target);

        if new_count != self.workers.len() {
            self.stop();
            self.start(uuid, nickname, code, cpu_target);
        } else {
            for w in &self.workers {
                let _ = w.send(WorkerCmd::SetThrottle(new_throttle));
            }
        }
    }

    pub fn poll_error(&mut self) -> Option<String> {
        self.error_rx.try_recv().ok()
    }

    pub fn is_running(&self) -> bool {
        self.stats.active_workers.load(Ordering::Relaxed) > 0
    }

    pub fn worker_count(&self) -> usize {
        self.workers.len()
    }
}

impl Drop for Pool {
    fn drop(&mut self) {
        self.stop();
    }
}
