use crate::rng::Xoshiro128PlusPlus;

pub const N: usize = 25; // update if el count ever changes, should get warning from swap

#[derive(Debug, Clone)]
pub struct BatchResult {
    pub seed: String,
    pub total_done: u64,
    pub best_correct: i32,
    pub best_arr: [u8; N],
    pub elapsed: f64,
}

#[inline(never)] // for profiling
pub fn run_batch(seed_str: &str, batch_size: u64) -> BatchResult {
    let seed64: u64 = seed_str.parse().unwrap_or(0);
    let mut rng = Xoshiro128PlusPlus::from_seed(seed64);

    let mut arr = [0u8; N];
    for i in 0..N {
        arr[i] = (i + 1) as u8;
    }

    let mut best_correct: i32 = -1;
    let mut best_arr = [0u8; N];

    let start = std::time::Instant::now();

    for _ in 0..batch_size {
        for i in (1..N).rev() {
            let j = rng.next_bounded((i + 1) as u32) as usize;
            arr.swap(i, j);
        }

        let mut correct: i32 = 0;
        for i in 0..N {
            if arr[i] == (i + 1) as u8 {
                correct += 1;
            }
        }

        if correct > best_correct {
            best_correct = correct;
            best_arr = arr;
        }
    }

    let elapsed = start.elapsed().as_secs_f64();

    BatchResult {
        seed: seed_str.to_string(),
        total_done: batch_size,
        best_correct,
        best_arr,
        elapsed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_batch_produces_valid_permutation() {
        let result = run_batch("12345", 100);
        // best_arr must be permutation of [1,.25]
        let mut sorted = result.best_arr;
        sorted.sort();
        for i in 0..N {
            assert_eq!(sorted[i], (i + 1) as u8);
        }
        assert!(result.best_correct >= 0);
        assert!(result.best_correct <= N as i32);
        assert_eq!(result.total_done, 100);
    }

    #[test]
    fn test_deterministic_output() {
        let a = run_batch("9999", 500);
        let b = run_batch("9999", 500);
        assert_eq!(a.best_correct, b.best_correct);
        assert_eq!(a.best_arr, b.best_arr);
    }
}
