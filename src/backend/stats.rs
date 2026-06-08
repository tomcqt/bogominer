use std::sync::atomic::{AtomicI32, AtomicU64, Ordering};

const ORD: Ordering = Ordering::Relaxed;

pub struct Stats {
    pub session_shuffles: AtomicU64,
    pub lifetime_shuffles: AtomicU64,
    pub rate: AtomicU64,
    pub session_best: AtomicI32,
    pub tick_best: AtomicI32,
    pub all_time_best: AtomicI32,
    pub active_workers: AtomicU64,
    pub total_credited: AtomicU64,
    pub current_second_best: AtomicI32,
    pub last5_packed: AtomicU64,
    pub _error_gen: AtomicU64,
    pub lease_cursor: AtomicU64,
    pub lease_count: AtomicU64,
    pub solver_threads: AtomicU64,
}

impl Stats {
    pub fn new() -> Self {
        Self {
            session_shuffles: AtomicU64::new(0),
            lifetime_shuffles: AtomicU64::new(0),
            rate: AtomicU64::new(0),
            session_best: AtomicI32::new(-1),
            tick_best: AtomicI32::new(-1),
            all_time_best: AtomicI32::new(0),
            active_workers: AtomicU64::new(0),
            total_credited: AtomicU64::new(0),
            current_second_best: AtomicI32::new(-1),
            last5_packed: AtomicU64::new(0),
            _error_gen: AtomicU64::new(0),
            lease_cursor: AtomicU64::new(0),
            lease_count: AtomicU64::new(0),
            solver_threads: AtomicU64::new(0),
        }
    }

    pub fn reset_session(&self) {
        self.session_shuffles.store(0, ORD);
        self.rate.store(0, ORD);
        self.session_best.store(-1, ORD);
        self.tick_best.store(-1, ORD);
        self.current_second_best.store(-1, ORD);
        self.last5_packed.store(0, ORD);
        self.total_credited.store(0, ORD);
        self.lease_count.store(0, ORD);
        self.lease_cursor.store(0, ORD);
    }

    pub fn note_batch_best(&self, best: i32) {
        loop {
            let old = self.current_second_best.load(ORD);
            if best <= old {
                break;
            }
            if self
                .current_second_best
                .compare_exchange_weak(old, best, ORD, ORD)
                .is_ok()
            {
                break;
            }
        }
        loop {
            let old = self.session_best.load(ORD);
            if best <= old {
                break;
            }
            if self
                .session_best
                .compare_exchange_weak(old, best, ORD, ORD)
                .is_ok()
            {
                break;
            }
        }
    }

    pub fn _tick_second(&self) {
        let completed = self.current_second_best.swap(-1, ORD);
        self.tick_best.store(completed, ORD);

        if completed >= 0 {
            let old_packed = self.last5_packed.load(ORD);
            let shifted = (old_packed << 8) & 0xFF_FF_FF_FF_00;
            let new_packed = shifted | (completed as u8 as u64);
            self.last5_packed.store(new_packed, ORD);
        }
    }

    pub fn get_last5(&self) -> Vec<u8> {
        let packed = self.last5_packed.load(ORD);
        let mut result = Vec::new();
        for i in 0..5 {
            let v = ((packed >> (i * 8)) & 0xFF) as u8;
            if v > 0 || packed != 0 {
                result.push(v);
            }
        }

        while result.last() == Some(&0) && result.len() > 1 {
            result.pop();
        }
        if result == vec![0] && packed == 0 {
            return Vec::new();
        }

        result
    }
}
