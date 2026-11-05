//! Wrapping 16-bit sequence arithmetic and the round-trip estimator.

use std::time::Duration;

/// True when `a` is newer than `b`, treating the 16-bit space as a circle.
pub(crate) fn seq_gt(a: u16, b: u16) -> bool {
    a != b && a.wrapping_sub(b) < 0x8000
}

/// Smoothed round-trip time (RFC 6298).
#[derive(Debug, Clone, Default)]
pub(crate) struct Rtt {
    srtt: Option<f64>,
    var: f64,
}

const MIN_RTO: f64 = 0.05;
const MAX_RTO: f64 = 1.0;
const INITIAL_RTO: f64 = 0.2;

impl Rtt {
    pub(crate) fn update(&mut self, sample: Duration) {
        let r = sample.as_secs_f64();
        match self.srtt {
            None => {
                self.srtt = Some(r);
                self.var = r / 2.0;
            }
            Some(s) => {
                self.var = 0.75 * self.var + 0.25 * (s - r).abs();
                self.srtt = Some(0.875 * s + 0.125 * r);
            }
        }
    }

    /// Smoothed RTT, or `None` before the first sample.
    pub(crate) fn smoothed(&self) -> Option<Duration> {
        self.srtt.map(Duration::from_secs_f64)
    }

    /// Retransmission timeout.
    pub(crate) fn rto(&self) -> Duration {
        let secs = match self.srtt {
            None => INITIAL_RTO,
            Some(s) => (s + 4.0 * self.var).clamp(MIN_RTO, MAX_RTO),
        };
        Duration::from_secs_f64(secs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrapping_comparison() {
        let cases = [
            (1u16, 0u16, true),
            (0, 1, false),
            (5, 5, false),
            (0, 65535, true),
            (65535, 0, false),
            (100, 65500, true),
        ];
        for (a, b, want) in cases {
            assert_eq!(seq_gt(a, b), want, "{a} > {b}");
        }
    }

    #[test]
    fn rtt_converges_to_steady_samples() {
        let mut r = Rtt::default();
        assert!(r.smoothed().is_none());
        r.update(Duration::from_millis(200));
        for _ in 0..60 {
            r.update(Duration::from_millis(40));
        }
        let s = r.smoothed().map(|d| d.as_secs_f64()).unwrap_or(0.0);
        assert!((s - 0.04).abs() < 0.002, "srtt {s}");
        assert!(r.rto() >= Duration::from_millis(50));
    }
}
