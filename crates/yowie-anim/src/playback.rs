//! Playback of one clip: position, loop handling and event crossing.

use std::sync::Arc;

use crate::clip::Clip;
use crate::timeline::{LoopMode, Sweep, local_time, sweeps};

/// Something a [`Player`](crate::Player) reports after an advance.
#[derive(Clone, Debug, PartialEq)]
pub enum PlayerEvent {
    /// Playback crossed a clip event.
    Marker {
        /// Name of the clip that holds the event.
        clip: String,
        /// The event name.
        name: String,
        /// The event time in the clip, in seconds.
        time: f32,
    },
    /// A [`LoopMode::Once`] or [`LoopMode::Clamp`] clip reached its end.
    Finished {
        /// Name of the clip that ended.
        clip: String,
    },
}

/// One clip being played: where it is and what the last step covered.
#[derive(Clone, Debug)]
pub(crate) struct Playback {
    pub clip: Arc<Clip>,
    pub mode: LoopMode,
    /// Unwrapped position in seconds; kept inside one period for loops.
    x: f64,
    /// True until the first advance, so an event at time 0 fires.
    first: bool,
    pub finished: bool,
    /// Clip-time ranges covered by the last advance.
    pub last_sweeps: Vec<Sweep>,
}

impl Playback {
    pub fn new(clip: Arc<Clip>) -> Self {
        let mode = clip.mode;
        Self {
            clip,
            mode,
            x: 0.0,
            first: true,
            finished: false,
            last_sweeps: Vec::new(),
        }
    }

    /// The current time inside the clip.
    pub fn time(&self) -> f32 {
        local_time(self.mode, self.clip.duration(), self.x)
    }

    /// Moves to `time` seconds into the clip without firing events.
    pub fn seek(&mut self, time: f32) {
        let d = f64::from(self.clip.duration());
        self.x = f64::from(time).clamp(0.0, d.max(0.0));
        self.first = true;
        self.finished = false;
        self.last_sweeps.clear();
    }

    /// True when the clip no longer drives its targets.
    pub fn is_spent(&self) -> bool {
        self.finished && self.mode == LoopMode::Once
    }

    /// Moves by `delta` seconds (negative plays backwards), pushing the
    /// events crossed. `report_finish` controls the finished event.
    pub fn advance(&mut self, delta: f64, out: &mut Vec<PlayerEvent>, report_finish: bool) {
        let d = f64::from(self.clip.duration());
        let x1 = self.x + delta;
        self.last_sweeps = sweeps(self.mode, self.clip.duration(), self.x, x1, self.first);
        self.first = false;
        for sweep in &self.last_sweeps {
            let hits = self.clip.events.iter().filter(|e| sweep.contains(e.time));
            let mut hits: Vec<_> = hits.collect();
            if sweep.to < sweep.from {
                hits.reverse();
            }
            out.extend(hits.into_iter().map(|e| PlayerEvent::Marker {
                clip: self.clip.name.clone(),
                name: e.name.clone(),
                time: e.time,
            }));
        }
        self.x = match self.mode {
            _ if d <= 0.0 => 0.0,
            LoopMode::Once | LoopMode::Clamp => x1.clamp(0.0, d),
            LoopMode::Loop => x1.rem_euclid(d),
            LoopMode::PingPong => x1.rem_euclid(2.0 * d),
        };
        let ends = matches!(self.mode, LoopMode::Once | LoopMode::Clamp);
        let at_end = if delta >= 0.0 { x1 >= d } else { x1 <= 0.0 };
        if ends && at_end && !self.finished {
            self.finished = true;
            if report_finish {
                out.push(PlayerEvent::Finished {
                    clip: self.clip.name.clone(),
                });
            }
        }
    }
}
