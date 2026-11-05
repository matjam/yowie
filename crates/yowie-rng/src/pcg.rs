use serde::{Deserialize, Serialize};

const MULT: u64 = 6_364_136_223_846_793_005;
const DEFAULT_STREAM: u64 = 0xda3e_39cb_94b9_5bdb;

/// A PCG32 (XSH RR) random number generator.
///
/// Two generators with the same seed and stream produce identical
/// sequences. The serialised form holds the full state.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "RngState", into = "RngState")]
pub struct Rng {
    state: u64,
    inc: u64,
}

#[derive(Serialize, Deserialize)]
struct RngState {
    state: u64,
    inc: u64,
}

impl From<RngState> for Rng {
    fn from(s: RngState) -> Self {
        Self::from_state(s.state, s.inc)
    }
}

impl From<Rng> for RngState {
    fn from(r: Rng) -> Self {
        Self {
            state: r.state,
            inc: r.inc,
        }
    }
}

impl Rng {
    /// Creates a generator on the default stream.
    pub fn new(seed: u64) -> Self {
        Self::with_stream(seed, DEFAULT_STREAM)
    }

    /// Creates a generator on the given stream. Different streams with the
    /// same seed give independent sequences.
    pub fn with_stream(seed: u64, stream: u64) -> Self {
        let mut r = Self {
            state: 0,
            inc: (stream << 1) | 1,
        };
        r.next_u32();
        r.state = r.state.wrapping_add(seed);
        r.next_u32();
        r
    }

    /// Returns the raw `(state, increment)` pair for [`Rng::from_state`].
    pub fn state(&self) -> (u64, u64) {
        (self.state, self.inc)
    }

    /// Rebuilds a generator from a pair returned by [`Rng::state`]. The
    /// increment is forced odd, as PCG requires.
    pub fn from_state(state: u64, inc: u64) -> Self {
        Self {
            state,
            inc: inc | 1,
        }
    }

    /// Creates a generator with its own stream, derived from this one.
    /// Forking advances this generator; the fork's sequence is independent
    /// of what this generator produces afterwards.
    pub fn fork(&mut self) -> Rng {
        let seed = self.next_u64();
        let stream = self.next_u64();
        Rng::with_stream(seed, stream)
    }

    /// Returns the next 32 random bits.
    pub fn next_u32(&mut self) -> u32 {
        let old = self.state;
        self.state = old.wrapping_mul(MULT).wrapping_add(self.inc);
        let xorshifted = (((old >> 18) ^ old) >> 27) as u32;
        let rot = (old >> 59) as u32;
        xorshifted.rotate_right(rot)
    }

    /// Returns the next 64 random bits (two draws, high word first).
    pub fn next_u64(&mut self) -> u64 {
        let hi = u64::from(self.next_u32());
        (hi << 32) | u64::from(self.next_u32())
    }

    /// Returns an unbiased integer in `0..n`, or 0 when `n` is 0.
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        if let Ok(bound) = u32::try_from(n) {
            let threshold = bound.wrapping_neg() % bound;
            loop {
                let m = u64::from(self.next_u32()) * u64::from(bound);
                if (m as u32) >= threshold {
                    return m >> 32;
                }
            }
        }
        let threshold = n.wrapping_neg() % n;
        loop {
            let m = u128::from(self.next_u64()) * u128::from(n);
            if (m as u64) >= threshold {
                return (m >> 64) as u64;
            }
        }
    }

    /// Returns an integer in `lo..=hi`. The bounds may be given in either
    /// order.
    pub fn range(&mut self, lo: i32, hi: i32) -> i32 {
        self.range_i64(i64::from(lo), i64::from(hi)) as i32
    }

    /// Returns an integer in `lo..=hi` for 64-bit bounds, in either order.
    pub fn range_i64(&mut self, lo: i64, hi: i64) -> i64 {
        let (lo, hi) = if hi < lo { (hi, lo) } else { (lo, hi) };
        let span = (i128::from(hi) - i128::from(lo) + 1) as u128;
        if span > u128::from(u64::MAX) {
            return self.next_u64() as i64;
        }
        (i128::from(lo) + i128::from(self.below(span as u64))) as i64
    }

    /// Returns a float in `[0, 1)` with 24 bits of precision.
    pub fn float(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 / (1u32 << 24) as f32
    }

    /// Returns a float in `[0, 1)` with 53 bits of precision.
    pub fn float64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Returns a float in `[lo, hi)`.
    pub fn between(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.float()
    }

    /// Returns a float in `[lo, hi)` at double precision.
    pub fn between64(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.float64()
    }

    /// Returns true with probability `p`. Values at or below 0 never
    /// succeed; values at or above 1 always do.
    pub fn chance(&mut self, p: f32) -> bool {
        self.float() < p
    }

    /// Rolls `dice` dice with `sides` sides each and returns the sum.
    /// Returns 0 when either count is 0.
    pub fn roll(&mut self, dice: u32, sides: u32) -> u64 {
        if sides == 0 {
            return 0;
        }
        (0..dice)
            .map(|_| 1 + self.below(u64::from(sides)))
            .sum::<u64>()
    }

    /// Returns a normally distributed value (Box-Muller transform).
    pub fn normal(&mut self, mean: f32, stddev: f32) -> f32 {
        self.normal64(f64::from(mean), f64::from(stddev)) as f32
    }

    /// Returns a normally distributed value at double precision.
    pub fn normal64(&mut self, mean: f64, stddev: f64) -> f64 {
        let u1 = self.float64().max(1e-12);
        let u2 = self.float64();
        let z = (-2.0 * u1.ln()).sqrt() * (std::f64::consts::TAU * u2).cos();
        mean + stddev * z
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_pcg32_reference_output() {
        // pcg32-demo: srandom(42, 54).
        let mut r = Rng::with_stream(42, 54);
        let got: Vec<u32> = (0..6).map(|_| r.next_u32()).collect();
        let want = [
            0xa15c_02b7,
            0x7b47_f409,
            0xba1d_3330,
            0x83d2_f293,
            0xbfa4_784b,
            0xcbed_606e,
        ];
        assert_eq!(got, want);
    }

    #[test]
    fn same_seed_same_sequence() {
        let mut a = Rng::new(7);
        let mut b = Rng::new(7);
        let mut c = Rng::new(8);
        let sa: Vec<u32> = (0..16).map(|_| a.next_u32()).collect();
        let sb: Vec<u32> = (0..16).map(|_| b.next_u32()).collect();
        let sc: Vec<u32> = (0..16).map(|_| c.next_u32()).collect();
        assert_eq!(sa, sb);
        assert_ne!(sa, sc);
    }

    #[test]
    fn ranges_stay_in_bounds() {
        let mut r = Rng::new(1);
        let cases = [(0, 0), (-5, 5), (10, -10), (i32::MIN, i32::MAX), (3, 4)];
        for (lo, hi) in cases {
            let (min, max) = (lo.min(hi), lo.max(hi));
            for _ in 0..500 {
                let v = r.range(lo, hi);
                assert!((min..=max).contains(&v), "{v} outside {min}..={max}");
            }
        }
        for _ in 0..500 {
            let v = r.range_i64(i64::MIN, i64::MAX);
            let _ = v;
            assert!(r.below(0) == 0);
            assert!(r.below(1 << 40) < (1 << 40));
        }
    }

    #[test]
    fn range_covers_every_value() {
        let mut r = Rng::new(2);
        let mut seen = [false; 6];
        for _ in 0..200 {
            seen[(r.range(1, 6) - 1) as usize] = true;
        }
        assert!(seen.iter().all(|&s| s));
    }

    #[test]
    fn floats_are_in_unit_interval() {
        let mut r = Rng::new(3);
        for _ in 0..1000 {
            assert!((0.0..1.0).contains(&r.float()));
            assert!((0.0..1.0).contains(&r.float64()));
            assert!((2.0..4.0).contains(&r.between(2.0, 4.0)));
        }
    }

    #[test]
    fn chance_extremes() {
        let mut r = Rng::new(4);
        assert!((0..100).all(|_| !r.chance(0.0)));
        assert!((0..100).all(|_| r.chance(1.0)));
        let hits = (0..10_000).filter(|_| r.chance(0.25)).count();
        assert!((2200..2800).contains(&hits), "{hits}");
    }

    #[test]
    fn roll_sums_dice() {
        let mut r = Rng::new(5);
        assert_eq!(r.roll(0, 6), 0);
        assert_eq!(r.roll(3, 0), 0);
        for _ in 0..200 {
            assert!((3..=18).contains(&r.roll(3, 6)));
        }
    }

    #[test]
    fn normal_has_requested_moments() {
        let mut r = Rng::new(6);
        let n = 20_000;
        let xs: Vec<f64> = (0..n).map(|_| r.normal64(10.0, 2.0)).collect();
        let mean = xs.iter().sum::<f64>() / n as f64;
        let var = xs.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / n as f64;
        assert!((mean - 10.0).abs() < 0.1, "{mean}");
        assert!((var.sqrt() - 2.0).abs() < 0.1, "{var}");
    }

    #[test]
    fn forks_are_independent_streams() {
        let mut parent = Rng::new(9);
        let mut f1 = parent.fork();
        let mut f2 = parent.fork();
        let a: Vec<u32> = (0..8).map(|_| f1.next_u32()).collect();
        let b: Vec<u32> = (0..8).map(|_| f2.next_u32()).collect();
        assert_ne!(a, b);
        // A fork does not depend on what the parent draws later.
        let mut p1 = Rng::new(9);
        let mut g1 = p1.fork();
        p1.next_u32();
        let mut p2 = Rng::new(9);
        let mut g2 = p2.fork();
        assert_eq!(g1.next_u32(), g2.next_u32());
    }

    #[test]
    fn serde_round_trip_resumes_sequence() {
        let mut r = Rng::new(11);
        for _ in 0..10 {
            r.next_u32();
        }
        let json = serde_json::to_string(&r).unwrap();
        let mut back: Rng = serde_json::from_str(&json).unwrap();
        assert_eq!(back, r);
        for _ in 0..10 {
            assert_eq!(back.next_u32(), r.next_u32());
        }
        let (s, i) = r.state();
        assert_eq!(Rng::from_state(s, i), r);
    }
}
