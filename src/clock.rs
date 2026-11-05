//! The fixed-step accumulator.

/// Turns wall time into a whole number of fixed updates.
#[derive(Clone, Copy, Debug)]
pub(crate) struct FixedClock {
    step: f64,
    accumulator: f64,
    max_steps: u32,
}

impl FixedClock {
    /// A clock of `step` seconds per update that runs at most `max_steps`
    /// updates per frame.
    pub(crate) fn new(step: f64, max_steps: u32) -> Self {
        Self {
            step: if step > 0.0 { step } else { 1.0 / 60.0 },
            accumulator: 0.0,
            max_steps: max_steps.max(1),
        }
    }

    /// Adds `dt` seconds of wall time and returns how many updates are due.
    /// A backlog beyond the cap is dropped, keeping only the fraction of a
    /// step, so a stall does not cause a burst of updates later.
    pub(crate) fn advance(&mut self, dt: f64) -> u32 {
        self.accumulator += dt.max(0.0);
        // The small bias stops rounding error turning exactly one step of
        // wall time into zero updates.
        let due = (self.accumulator / self.step + 1e-9).floor();
        if due > f64::from(self.max_steps) {
            self.accumulator = self.accumulator.rem_euclid(self.step);
            return self.max_steps;
        }
        self.accumulator = (self.accumulator - due * self.step).max(0.0);
        due as u32
    }

    /// The fraction of a step accumulated towards the next update.
    pub(crate) fn alpha(&self) -> f32 {
        (self.accumulator / self.step).clamp(0.0, 1.0) as f32
    }

    /// Drops any accumulated time, after a pause.
    pub(crate) fn reset(&mut self) {
        self.accumulator = 0.0;
    }

    /// Seconds per update.
    pub(crate) fn step(&self) -> f64 {
        self.step
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_follow_wall_time() {
        let step = 1.0 / 60.0;
        let cases: [(&[f64], &[u32]); 4] = [
            // Exactly one step per frame.
            (&[step, step, step], &[1, 1, 1]),
            // A 120 Hz display: an update every other frame.
            (&[step / 2.0; 4], &[0, 1, 0, 1]),
            // A slow frame catches up.
            (&[step * 3.0], &[3]),
            // A long stall is capped and the backlog dropped.
            (&[10.0, step], &[8, 1]),
        ];
        for (frames, want) in cases {
            let mut c = FixedClock::new(step, 8);
            let got: Vec<u32> = frames.iter().map(|dt| c.advance(*dt)).collect();
            assert_eq!(got, want, "{frames:?}");
        }
    }

    #[test]
    fn alpha_is_the_leftover_fraction() {
        let mut c = FixedClock::new(0.1, 8);
        assert_eq!(c.advance(0.25), 2);
        assert!((c.alpha() - 0.5).abs() < 1e-4);
        c.reset();
        assert_eq!(c.alpha(), 0.0);
    }
}
