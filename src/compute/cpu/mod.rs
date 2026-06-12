use crate::backend::miner::Miner;
use crate::backend::solver::{self, RangeResult};

pub struct CpuMiner;

impl CpuMiner {
    pub fn new() -> Self {
        eprintln!("[cpu] backend init");
        Self
    }
}
impl Default for CpuMiner {
    fn default() -> Self {
        Self::new()
    }
}

const CPU_CHUNK_SIZE: u64 = 2_000_000;

impl Miner for CpuMiner {
    fn name(&self) -> String {
        "cpu".to_string()
    }

    fn compute_range(&mut self, seed: u64, lo: u64, hi: u64, threshold: i32) -> RangeResult {
        solver::run_range_with_threshold(seed, lo, hi, threshold)
    }

    fn chunk_size(&self) -> u64 {
        CPU_CHUNK_SIZE
    }
}
