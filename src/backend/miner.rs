use crate::backend::solver::RangeResult;

// based on https://github.com/mnhttn-cafe/bogoforge/tree/master/src/compute/mod.rs ComputeBackend
pub trait Miner: Send {
    fn name(&self) -> String;
    fn compute_range(&mut self, seed: u64, lo: u64, hi: u64, threshold: i32) -> RangeResult;
    fn chunk_size(&self) -> u64;
}
