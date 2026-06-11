use crate::backend::solver::RangeResult;

pub trait ComputeBackend: Send {
    fn name(&self) -> &'static str;

    fn compute_range(&mut self, seed64: u64, lo: u64, hi: u64, threshold: i32) -> RangeResult;
}
