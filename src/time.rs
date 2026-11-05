//! Game time as the game reads it.

/// The largest time scale; larger values are clamped so one frame cannot
/// owe more updates than the loop's cap allows.
pub const MAX_TIME_SCALE: f32 = 64.0;

/// Game time: the step each update covers, the total so far, how many
/// frames have drawn and where the current frame falls between updates.
///
/// Game time can run at a different speed from the wall clock:
/// [`set_scale`](Time::set_scale) slows or speeds it and
/// [`set_paused`](Time::set_paused) stops it. In a fixed-step loop the step
/// keeps its size and the loop runs more or fewer updates per wall second,
/// still limited by
/// [`Config::max_updates_per_frame`](crate::Config::max_updates_per_frame).
/// In a turn-based loop the delta is multiplied by the scale. Timers,
/// tweens and anything else driven by [`delta`](Time::delta) follow.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Time {
    pub(crate) delta: f32,
    pub(crate) elapsed: f64,
    pub(crate) frame: u64,
    pub(crate) updates: u64,
    pub(crate) alpha: f32,
    pub(crate) frame_delta: f32,
    pub(crate) fps: f32,
    pub(crate) scale: f32,
    pub(crate) paused: bool,
}

impl Default for Time {
    fn default() -> Self {
        Self {
            delta: 0.0,
            elapsed: 0.0,
            frame: 0,
            updates: 0,
            alpha: 0.0,
            frame_delta: 0.0,
            fps: 0.0,
            scale: 1.0,
            paused: false,
        }
    }
}

impl Time {
    /// Seconds the current update covers: the fixed step in a fixed-step
    /// loop, the wall time since the previous turn times the time scale in
    /// a turn-based loop (zero for the first turn after a pause). During
    /// `draw` it is the last update's value.
    pub fn delta(&self) -> f32 {
        self.delta
    }

    /// Seconds of game time: the sum of every update's delta.
    pub fn elapsed(&self) -> f64 {
        self.elapsed
    }

    /// Frames drawn before this one.
    pub fn frame(&self) -> u64 {
        self.frame
    }

    /// Updates run so far.
    pub fn updates(&self) -> u64 {
        self.updates
    }

    /// How far the drawn frame falls between the last update and the next,
    /// `0..1`. Draw moving things at `previous.lerp(current, alpha)` for
    /// smooth motion at any refresh rate. Zero in turn-based loops and
    /// under a fixed clock.
    pub fn alpha(&self) -> f32 {
        self.alpha
    }

    /// Wall seconds between the previous frame and this one.
    pub fn frame_delta(&self) -> f32 {
        self.frame_delta
    }

    /// Frames per second, smoothed over recent frames.
    pub fn fps(&self) -> f32 {
        self.fps
    }

    /// How fast game time runs compared with the wall clock: 1 is normal,
    /// 0.5 half speed, 2 double speed.
    pub fn scale(&self) -> f32 {
        self.scale
    }

    /// Sets the time scale, clamped to `0..=`[`MAX_TIME_SCALE`]. A scale of
    /// 0 stops game time like [`set_paused`](Time::set_paused) but keeps
    /// the pause flag as it is. NaN is ignored.
    pub fn set_scale(&mut self, scale: f32) {
        if !scale.is_nan() {
            self.scale = scale.clamp(0.0, MAX_TIME_SCALE);
        }
    }

    /// Whether game time is paused.
    pub fn paused(&self) -> bool {
        self.paused
    }

    /// Pauses or resumes game time. While paused no updates run in a
    /// fixed-step loop (draw still runs, so menus and the console stay
    /// live) and turn-based turns have a delta of zero.
    pub fn set_paused(&mut self, paused: bool) {
        self.paused = paused;
    }

    /// The factor applied to wall time: the scale, or 0 while paused.
    pub(crate) fn rate(&self) -> f64 {
        if self.paused {
            0.0
        } else {
            f64::from(self.scale)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scale_is_clamped_and_pause_stops_the_rate() {
        let mut t = Time::default();
        assert_eq!(t.rate(), 1.0);
        t.set_scale(-3.0);
        assert_eq!(t.scale(), 0.0);
        t.set_scale(1e9);
        assert_eq!(t.scale(), MAX_TIME_SCALE);
        t.set_scale(f32::NAN);
        assert_eq!(t.scale(), MAX_TIME_SCALE);
        t.set_scale(0.5);
        assert_eq!(t.rate(), 0.5);
        t.set_paused(true);
        assert_eq!(t.rate(), 0.0);
        assert_eq!(t.scale(), 0.5);
    }
}
