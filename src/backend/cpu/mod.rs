use std::simd::cmp::SimdPartialOrd;
use std::simd::{Mask, Simd};

use crate::backend::compute::ComputeBackend;
use crate::backend::rng::shuffle_seed;
use crate::backend::solver::{one_shuffle, run_range_with_threshold, RangeResult, N};

const LANES: usize = 16;
const GOLDEN: u64 = 0x9e3779b97f4a7c15; // https://arxiv.org/pdf/1805.01407

type U32s = Simd<u32, LANES>;

#[inline(always)]
fn rotl_simd(x: U32s, k: u32) -> U32s {
    (x << k) | (x >> (32 - k))
}

#[inline(always)]
fn xnext_simd(s: &mut [U32s; 4]) -> U32s {
    let res = rotl_simd(s[0] + s[3], 7) + s[0];
    let t = s[1] << 9;
    s[2] ^= s[0];
    s[3] ^= s[1];
    s[1] ^= s[2];
    s[0] ^= s[3];
    s[2] ^= t;
    s[3] = rotl_simd(s[3], 11);
    res
}

#[inline(always)]
fn load_state(seed64: u64, base_index: u64) -> [U32s; 4] {
    let mut s0 = [0u32; LANES];
    let mut s1 = [0u32; LANES];
    let mut s2 = [0u32; LANES];
    let mut s3 = [0u32; LANES];
    for lane in 0..LANES {
        let seed_i = shuffle_seed(seed64, base_index + lane as u64);
        let mut z = seed_i;
        z = z.wrapping_add(GOLDEN);
        let mut a = z;
        a = (a ^ (a >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        a = (a ^ (a >> 27)).wrapping_mul(0x94d049bb133111eb);
        a ^= a >> 31;
        z = z.wrapping_add(GOLDEN);
        let mut b = z;
        b = (b ^ (b >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        b = (b ^ (b >> 27)).wrapping_mul(0x94d049bb133111eb);
        b ^= b >> 31;
        let mut st = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32];
        if st == [0, 0, 0, 0] {
            st[0] = 1;
        }
        s0[lane] = st[0];
        s1[lane] = st[1];
        s2[lane] = st[2];
        s3[lane] = st[3];
    }
    [
        U32s::from_array(s0),
        U32s::from_array(s1),
        U32s::from_array(s2),
        U32s::from_array(s3),
    ]
}

pub struct CpuBackend;

impl CpuBackend {
    pub fn new() -> Self {
        CpuBackend
    }
}

impl Default for CpuBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl ComputeBackend for CpuBackend {
    fn name(&self) -> &'static str {
        "cpu-simd"
    }

    fn compute_range(&mut self, seed64: u64, lo: u64, hi: u64, threshold: i32) -> RangeResult {
        let mut best_correct: i32 = -1;
        let mut best_arr = [0u8; N];
        let mut best_index: u64 = lo;
        let mut thr = threshold;

        let total = hi - lo;
        let simd_count = total - (total % LANES as u64);
        let simd_end = lo + simd_count;

        let mut base = lo;
        while base < simd_end {
            let mut state = load_state(seed64, base);

            // arr[pos] / lane, arr[pos][lane]
            let mut arr = [[0u8; LANES]; N];
            for (pos, row) in arr.iter_mut().enumerate() {
                *row = [(pos + 1) as u8; LANES];
            }

            let mut tainted: Mask<i32, LANES> = Mask::splat(false);

            macro_rules! fy_step {
                ($i:expr) => {{
                    const MAX: u32 = ($i + 1) as u32;
                    const THR: u32 = (0x1_0000_0000u64 % MAX as u64) as u32;
                    let x = xnext_simd(&mut state);
                    if THR != 0 {
                        tainted |= x.simd_lt(U32s::splat(THR));
                    }
                    let j = x % U32s::splat(MAX);
                    let jarr = j.to_array();
                    for lane in 0..LANES {
                        let jj = jarr[lane] as usize;
                        let tmp = arr[$i][lane];
                        arr[$i][lane] = arr[jj][lane];
                        arr[jj][lane] = tmp;
                    }
                }};
            }

            fy_step!(24);
            fy_step!(23);
            fy_step!(22);
            fy_step!(21);
            fy_step!(20);
            fy_step!(19);
            fy_step!(18);
            fy_step!(17);
            fy_step!(16);
            fy_step!(15);
            fy_step!(14);
            fy_step!(13);
            fy_step!(12);
            fy_step!(11);
            fy_step!(10);
            fy_step!(9);
            fy_step!(8);
            fy_step!(7);
            fy_step!(6);
            fy_step!(5);
            fy_step!(4);
            fy_step!(3);
            fy_step!(2);
            fy_step!(1);

            let tainted_arr = tainted.to_array();
            for lane in 0..LANES {
                let index = base + lane as u64;
                let (correct, out) = if tainted_arr[lane] {
                    one_shuffle(seed64, index)
                } else {
                    let mut out = [0u8; N];
                    let mut c: i32 = 0;
                    for pos in 0..N {
                        let v = arr[pos][lane];
                        out[pos] = v;
                        if v == (pos + 1) as u8 {
                            c += 1;
                        }
                    }
                    (c, out)
                };
                if correct > thr {
                    best_correct = correct;
                    best_arr = out;
                    best_index = index;
                    thr = correct;
                }
            }

            base += LANES as u64;
        }

        // scalar tail with the same threshold prune as the scalar backend
        if simd_end < hi {
            let tail = run_range_with_threshold(seed64, simd_end, hi, thr);
            if tail.best_correct > best_correct {
                best_correct = tail.best_correct;
                best_arr = tail.best_arr;
                best_index = tail.best_index;
            }
        }

        RangeResult {
            count: total,
            best_correct,
            best_arr,
            best_index,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::solver::run_range;

    #[test]
    fn batch_matches_scalar() {
        let seed = 12345u64;
        let mut be = CpuBackend::new();
        let expected = run_range(seed, 0, 5000);
        let got = be.compute_range(seed, 0, 5000, -1);
        assert_eq!(got.count, expected.count);
        assert_eq!(got.best_correct, expected.best_correct);
        assert_eq!(got.best_arr, expected.best_arr);
        assert_eq!(got.best_index, expected.best_index);
    }

    #[test]
    fn batch_matches_scalar_sweep() {
        let mut be = CpuBackend::new();
        for seed in [1u64, 2, 99, 12345, 7777777] {
            for (lo, hi) in [(0u64, 1000u64), (37, 2099), (100, 100 + 16), (0, 7)] {
                let expected = run_range(seed, lo, hi);
                let got = be.compute_range(seed, lo, hi, -1);
                assert_eq!(
                    got.best_correct, expected.best_correct,
                    "seed {seed} {lo}..{hi}"
                );
                assert_eq!(got.best_arr, expected.best_arr, "seed {seed} {lo}..{hi}");
                assert_eq!(
                    got.best_index, expected.best_index,
                    "seed {seed} {lo}..{hi}"
                );
            }
        }
    }
}
