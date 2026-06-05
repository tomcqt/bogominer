// TODO: add source urls for algorithms

pub struct SplitMix64 {
    z: u64,
}

impl SplitMix64 {
    pub fn new(seed: u64) -> Self {
        Self { z: seed }
    }

    pub fn next(&mut self) -> u64 {
        self.z = self.z.wrapping_add(0x9e3779b97f4a7c15);
        let mut z = self.z;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
        z ^ (z >> 31)
    }
}

pub struct Xoshiro128PlusPlus {
    s: [u32; 4],
}

impl Xoshiro128PlusPlus {
    pub fn from_seed(seed64: u64) -> Self {
        let mut sm = SplitMix64::new(seed64);
        let a = sm.next();
        let b = sm.next();
        let mut s = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32];
        if s[0] == 0 && s[1] == 0 && s[2] == 0 && s[3] == 0 {
            s[0] = 1;
        }
        Self { s }
    }

    #[inline(always)]
    pub fn next_u32(&mut self) -> u32 {
        let res = (self.s[0].wrapping_add(self.s[3]))
            .rotate_left(7)
            .wrapping_add(self.s[0]);
        let t = self.s[1] << 9;
        self.s[2] ^= self.s[0];
        self.s[3] ^= self.s[1];
        self.s[1] ^= self.s[2];
        self.s[0] ^= self.s[3];
        self.s[2] ^= t;
        self.s[3] = self.s[3].rotate_left(11);
        res
    }

    #[inline(always)]
    pub fn next_bounded(&mut self, max: u32) -> u32 {
        let threshold = (0x1_0000_0000u64 % max as u64) as u32;
        loop {
            let x = self.next_u32();
            if x >= threshold {
                return x % max;
            }
        }
    }
}

#[cfg(test)]

mod tests {
    use super::*;

    #[test]
    fn test_splitmix64_matches_js() {
        let mut sm = SplitMix64::new(12345);
        let a = sm.next();
        let b = sm.next();
        // TODO: add proper exact values from js impl
        assert_ne!(a, 0);
        assert_ne!(b, 0);
    }

    #[test]
    fn test_xoshiro_deterministic() {
        let mut rng1 = Xoshiro128PlusPlus::from_seed(42);
        let mut rng2 = Xoshiro128PlusPlus::from_seed(42);
        for _ in 0..1000 {
            assert_eq!(rng1.next_u32(), rng2.next_u32());
        }
    }

    #[test]
    fn text_bounded_range() {
        let mut rng = Xoshiro128PlusPlus::from_seed(999);
        for _ in 0..10000 {
            let v = rng.next_bounded(25);
            assert!(v < 25);
        }
    }
}
