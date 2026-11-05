//! Game time as the game reads it.

/// Game time: the step each update covers, the total so far, how many
/// frames have drawn and where the current frame falls between updates.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Time {
    pub(crate) delta: f32,
    pub(crate) elapsed: f64,
    pub(crate) frame: u64,
    pub(crate) updates: u64,
    pub(crate) alpha: f32,
    pub(crate) frame_delta: f32,
    pub(crate) fps: f32,
}

impl Time {
    /// Seconds the current update covers: the fixed step in a fixed-step
    /// loop, the wall time since the previous turn in a turn-based loop
    /// (zero for the first turn after a pause). During `draw` it is the
    /// last update's value.
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
}
