use crate::backend::rng::{shuffle_seed, Xoshiro128PlusPlus};

pub const N: usize = 25; // update if el count ever changes, should get warning from swap

#[derive(Debug, Clone)]
pub struct RangeResult {
    pub count: u64,
    pub best_correct: i32,
    pub best_arr: [u8; N],
    pub best_index: u64,
}

#[inline(always)]
pub fn one_shuffle(seed64: u64, index: u64) -> (i32, [u8; N]) {
    let seed_i = shuffle_seed(seed64, index);
    let mut rng = Xoshiro128PlusPlus::from_seed(seed_i);

    let mut arr = [0u8; N];
    for i in 0..N {
        arr[i] = (i + 1) as u8;
    }

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

    (correct, arr)
}

pub fn run_range(seed64: u64, lo: u64, hi: u64) -> RangeResult {
    let mut best_correct: i32 = -1;
    let mut best_arr = [0u8; N];
    let mut best_index: u64 = lo;

    for i in lo..hi {
        let (correct, arr) = one_shuffle(seed64, i);
        if correct > best_correct {
            best_correct = correct;
            best_arr = arr;
            best_index = i;
        }
    }

    RangeResult {
        count: hi - lo,
        best_correct,
        best_arr,
        best_index,
    }
}

#[inline(always)]
fn shuffle_if_above_threshold(seed64: u64, index: u64, threshold: i32) -> Option<(i32, [u8; N])> {
    let seed_i = shuffle_seed(seed64, index);
    let mut rng = Xoshiro128PlusPlus::from_seed(seed_i);
    let mut arr = [0u8; N];
    for i in 0..N {
        arr[i] = (i + 1) as u8;
    }

    let mut fixed: i32 = 0;
    let mut active_mask: u32 = (1u32 << N) - 1;

    // after fisher-yates step i, position i is final. the active value mask
    // bounds how many remaining positions can still be fixed.
    for i in (1..N).rev() {
        let j = rng.next_bounded((i + 1) as u32) as usize;
        let placed = arr[j];
        arr.swap(i, j);

        if placed == (i + 1) as u8 {
            fixed += 1;
        }
        active_mask &= !(1u32 << (placed - 1));

        let future_mask = (1u32 << i) - 1;
        let possible_future = (active_mask & future_mask).count_ones() as i32;
        if fixed + possible_future <= threshold {
            return None;
        }
    }

    let correct = fixed + i32::from(arr[0] == 1);
    (correct > threshold).then_some((correct, arr))
}

pub fn run_range_with_threshold(
    seed64: u64,
    lo: u64,
    hi: u64,
    initial_threshold: i32,
) -> RangeResult {
    let mut best_correct: i32 = -1;
    let mut best_arr = [0u8; N];
    let mut best_index: u64 = lo;
    let mut threshold = initial_threshold;

    for i in lo..hi {
        let Some((correct, arr)) = shuffle_if_above_threshold(seed64, i, threshold) else {
            continue;
        };
        best_correct = correct;
        best_arr = arr;
        best_index = i;
        threshold = correct;
    }

    RangeResult {
        count: hi - lo,
        best_correct,
        best_arr,
        best_index,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_one_shuffle_valid_permutation() {
        let (correct, arr) = one_shuffle(12345, 0);
        let mut sorted = arr;
        sorted.sort();
        for i in 0..N {
            assert_eq!(sorted[i], (i + 1) as u8);
        }
        assert!(correct >= 0 && correct <= N as i32);
    }

    #[test]
    fn test_one_shuffle_deterministic() {
        let (c1, a1) = one_shuffle(12345, 42);
        let (c2, a2) = one_shuffle(12345, 42);
        assert_eq!(c1, c2);
        assert_eq!(a1, a2);
    }

    #[test]
    fn test_run_range_tracks_best_index() {
        let result = run_range(12345, 0, 10000);
        let (correct, arr) = one_shuffle(12345, result.best_index);
        assert_eq!(correct, result.best_correct);
        assert_eq!(arr, result.best_arr);
    }

    #[test]
    fn test_run_range_count() {
        let result = run_range(999, 100, 600);
        assert_eq!(result.count, 500);
    }

    #[test]
    fn test_run_range_with_threshold_matches_run_range() {
        let seed = 12345;
        let lo = 100;
        let hi = 1100;
        let expected = run_range(seed, lo, hi);
        let result = run_range_with_threshold(seed, lo, hi, -1);
        assert_eq!(result.count, expected.count);
        assert_eq!(result.best_correct, expected.best_correct);
        assert_eq!(result.best_arr, expected.best_arr);
        assert_eq!(result.best_index, expected.best_index);
    }
}
