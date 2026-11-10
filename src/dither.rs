//! TPDF dither for word-length reduction (used when writing 16-bit files).

/// Triangular-PDF dither source producing noise in `[-1, 1)` LSB.
///
/// Uses xorshift32: dither only needs decorrelated noise, not cryptographic randomness.
#[derive(Debug, Clone)]
pub struct Tpdf {
    state: u32,
}

impl Tpdf {
    /// `seed` should differ between channels so their noise is uncorrelated.
    pub fn new(seed: u32) -> Self {
        // xorshift must never hold 0; mix the seed so small seeds still diverge quickly.
        let mixed = seed.wrapping_mul(0x9E37_79B9) ^ 0x6C8E_9CF5;
        Self {
            state: if mixed == 0 { 1 } else { mixed },
        }
    }

    fn next_unit(&mut self) -> f32 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.state = x;
        // Top 24 bits -> [0, 1)
        (x >> 8) as f32 / (1u32 << 24) as f32
    }

    /// Next dither value in LSB units, triangular over `[-1, 1)`.
    pub fn sample(&mut self) -> f32 {
        self.next_unit() - self.next_unit()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_stay_within_one_lsb() {
        let mut d = Tpdf::new(1);
        for _ in 0..100_000 {
            let v = d.sample();
            assert!((-1.0..1.0).contains(&v), "{v}");
        }
    }

    #[test]
    fn mean_is_near_zero() {
        let mut d = Tpdf::new(7);
        let n = 200_000;
        let sum: f64 = (0..n).map(|_| d.sample() as f64).sum();
        assert!((sum / n as f64).abs() < 0.01);
    }

    #[test]
    fn different_seeds_give_different_sequences() {
        let mut a = Tpdf::new(0);
        let mut b = Tpdf::new(1);
        let same = (0..1000).filter(|_| a.sample() == b.sample()).count();
        assert!(same < 10);
    }
}
