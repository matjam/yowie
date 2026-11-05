/// The most times a repeating timer fires in one update. A very short
/// interval with a very long `dt` stops here instead of looping for ages.
const MAX_FIRES: u32 = 10_000;

/// A countdown on game time, one-shot or repeating.
///
/// A one-shot timer fires once and then stays finished until
/// [`Timer::reset`]. A repeating timer fires every `duration` seconds and
/// keeps the time left over, so it does not drift.
#[derive(Clone, Debug, PartialEq)]
pub struct Timer {
    duration: f64,
    elapsed: f64,
    repeating: bool,
    paused: bool,
    finished: bool,
}

impl Timer {
    /// Creates a timer that fires once after `seconds`.
    pub fn once(seconds: f64) -> Self {
        Self::new(seconds, false)
    }

    /// Creates a timer that fires every `seconds`. An interval of zero or
    /// less fires once per update.
    pub fn repeating(seconds: f64) -> Self {
        Self::new(seconds, true)
    }

    fn new(duration: f64, repeating: bool) -> Self {
        Self {
            duration,
            elapsed: 0.0,
            repeating,
            paused: false,
            finished: false,
        }
    }

    /// Advances the timer by `dt` seconds and returns how many times it
    /// fired. A one-shot timer returns at most 1.
    pub fn update(&mut self, dt: f64) -> u32 {
        self.tick(dt, |_| {})
    }

    /// Like [`Timer::update`], and calls `on_fire` for each firing with the
    /// seconds into this update at which it fired (between 0 and `dt`).
    pub fn tick(&mut self, dt: f64, mut on_fire: impl FnMut(f64)) -> u32 {
        if self.paused || self.finished {
            return 0;
        }
        self.elapsed += dt.max(0.0);
        let dt = dt.max(0.0);
        let mut fires = 0;
        while self.elapsed >= self.duration {
            fires += 1;
            on_fire((dt - (self.elapsed - self.duration)).clamp(0.0, dt));
            if !self.repeating {
                self.finished = true;
                self.elapsed = self.duration;
                break;
            }
            if self.duration <= 0.0 || fires >= MAX_FIRES {
                self.elapsed = 0.0;
                break;
            }
            self.elapsed -= self.duration;
        }
        fires
    }

    /// Stops the timer from advancing until [`Timer::resume`].
    pub fn pause(&mut self) {
        self.paused = true;
    }

    /// Lets a paused timer advance again.
    pub fn resume(&mut self) {
        self.paused = false;
    }

    /// Returns true while paused.
    pub fn paused(&self) -> bool {
        self.paused
    }

    /// Returns true once a one-shot timer has fired. Repeating timers never
    /// finish.
    pub fn finished(&self) -> bool {
        self.finished
    }

    /// Restarts the countdown from zero and clears the finished state.
    pub fn reset(&mut self) {
        self.elapsed = 0.0;
        self.finished = false;
    }

    /// Seconds until the next firing.
    pub fn remaining(&self) -> f64 {
        (self.duration - self.elapsed).max(0.0)
    }

    /// Progress through the current countdown, from 0 to 1.
    pub fn fraction(&self) -> f64 {
        if self.duration <= 0.0 {
            return 1.0;
        }
        (self.elapsed / self.duration).clamp(0.0, 1.0)
    }

    /// The countdown length in seconds.
    pub fn duration(&self) -> f64 {
        self.duration
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn once_fires_one_time() {
        let mut t = Timer::once(1.0);
        let fires: Vec<u32> = [0.4, 0.4, 0.4, 5.0]
            .iter()
            .map(|&dt| t.update(dt))
            .collect();
        assert_eq!(fires, [0, 0, 1, 0]);
        assert!(t.finished());
        t.reset();
        assert_eq!(t.update(1.0), 1);
    }

    #[test]
    fn repeating_keeps_remainder() {
        let mut t = Timer::repeating(0.5);
        // Binary fractions keep the arithmetic exact.
        let cases = [(0.25, 0), (0.25, 1), (1.25, 2), (0.0, 0)];
        for (dt, want) in cases {
            assert_eq!(t.update(dt), want, "dt {dt}");
        }
        assert_eq!(t.remaining(), 0.25);
    }

    #[test]
    fn pause_and_resume() {
        let mut t = Timer::once(1.0);
        t.pause();
        assert_eq!(t.update(10.0), 0);
        assert!(t.paused());
        t.resume();
        assert_eq!(t.update(1.0), 1);
    }

    #[test]
    fn fire_offsets_lie_inside_the_update() {
        let mut t = Timer::repeating(1.0);
        t.update(0.5);
        let mut offsets = Vec::new();
        t.tick(2.0, |o| offsets.push(o));
        assert_eq!(offsets.len(), 2);
        assert!((offsets[0] - 0.5).abs() < 1e-9);
        assert!((offsets[1] - 1.5).abs() < 1e-9);
    }

    #[test]
    fn degenerate_intervals_terminate() {
        let mut t = Timer::repeating(0.0);
        assert_eq!(t.update(1.0), 1);
        let mut t = Timer::repeating(1e-9);
        assert_eq!(t.update(1000.0), MAX_FIRES);
    }

    #[test]
    fn fraction_reports_progress() {
        let mut t = Timer::once(2.0);
        t.update(0.5);
        assert!((t.fraction() - 0.25).abs() < 1e-12);
        assert_eq!(t.duration(), 2.0);
    }
}
