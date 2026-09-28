//! Deterministic RNG. The sim never touches thread/global randomness so that
//! replays and boss-bot tests are reproducible.

use bevy_ecs::prelude::*;
use rand_chacha::ChaCha8Rng;
use rand_core::{Rng, SeedableRng};

#[derive(Resource, Clone, Debug)]
pub struct SimRng(ChaCha8Rng);

impl SimRng {
    pub fn new(seed: u64) -> Self {
        Self(ChaCha8Rng::seed_from_u64(seed))
    }

    /// Uniform in `[0, 1)`.
    pub fn f32(&mut self) -> f32 {
        (self.0.next_u32() >> 8) as f32 / (1u32 << 24) as f32
    }

    /// Uniform in `[lo, hi)`.
    pub fn range_f32(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.f32()
    }

    /// Uniform integer in `[0, n)`. `n` must be non-zero.
    pub fn below(&mut self, n: u32) -> u32 {
        debug_assert!(n > 0);
        ((self.0.next_u32() as u64 * n as u64) >> 32) as u32
    }

    pub fn chance(&mut self, p: f32) -> bool {
        self.f32() < p
    }
}

impl Default for SimRng {
    fn default() -> Self {
        Self::new(0x48_4B_33_44) // "HK3D"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_seed_same_stream() {
        let mut a = SimRng::new(7);
        let mut b = SimRng::new(7);
        for _ in 0..100 {
            assert_eq!(a.below(1000), b.below(1000));
        }
    }

    #[test]
    fn ranges_hold() {
        let mut r = SimRng::new(1);
        for _ in 0..1000 {
            let f = r.f32();
            assert!((0.0..1.0).contains(&f));
            assert!(r.below(5) < 5);
            let x = r.range_f32(-2.0, 3.0);
            assert!((-2.0..3.0).contains(&x));
        }
    }
}
