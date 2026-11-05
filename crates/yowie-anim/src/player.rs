//! The clip player: playback state, crossfades and events.

use std::sync::Arc;

use crate::clip::Clip;
use crate::playback::{Playback, PlayerEvent};
use crate::root_motion::{RootMotion, over_sweeps};
use crate::timeline::LoopMode;
use crate::value::{Target, Value};

/// A clip that is fading out.
#[derive(Clone, Debug)]
struct Fade {
    from: Playback,
    elapsed: f32,
    duration: f32,
}

impl Fade {
    /// The incoming clip's weight, 0 to 1.
    fn incoming(&self) -> f32 {
        (self.elapsed / self.duration).clamp(0.0, 1.0)
    }
}

/// Plays one clip at a time, with crossfades between clips.
///
/// Call [`Player::advance`] once per update, then read events with
/// [`Player::take_events`] and apply the pose with [`Player::apply`] or
/// [`Player::evaluate`]. Skeletons read a player through
/// [`Skeleton::pose_from_player`](crate::Skeleton::pose_from_player).
#[derive(Clone, Debug)]
pub struct Player {
    current: Option<Playback>,
    fade: Option<Fade>,
    speed: f32,
    paused: bool,
    events: Vec<PlayerEvent>,
}

impl Default for Player {
    fn default() -> Self {
        Self::new()
    }
}

impl Player {
    /// A player with nothing playing, at normal speed.
    pub fn new() -> Self {
        Self {
            current: None,
            fade: None,
            speed: 1.0,
            paused: false,
            events: Vec::new(),
        }
    }

    /// Starts `clip` from its beginning, dropping any crossfade. The clip's
    /// own loop mode applies.
    pub fn play(&mut self, clip: Arc<Clip>) {
        self.current = Some(Playback::new(clip));
        self.fade = None;
        self.paused = false;
    }

    /// Starts `clip` while the current clip fades out over `seconds` of
    /// update time (independent of playback speed). Plays at once when
    /// nothing is playing or `seconds` is not positive.
    pub fn crossfade(&mut self, clip: Arc<Clip>, seconds: f32) {
        match self.current.take() {
            Some(from) if seconds > 0.0 => {
                self.fade = Some(Fade {
                    from,
                    elapsed: 0.0,
                    duration: seconds,
                });
                self.current = Some(Playback::new(clip));
                self.paused = false;
            }
            _ => self.play(clip),
        }
    }

    /// Stops playback and clears the clip and any crossfade.
    pub fn stop(&mut self) {
        self.current = None;
        self.fade = None;
    }

    /// Freezes playback at the current position.
    pub fn pause(&mut self) {
        self.paused = true;
    }

    /// Continues after [`Player::pause`].
    pub fn resume(&mut self) {
        self.paused = false;
    }

    /// Sets the playback rate: 1 is normal, 2 double, negative plays
    /// backwards. A backwards `Once` or `Clamp` clip finishes at once unless
    /// first moved with [`Player::seek`] to its end.
    pub fn set_speed(&mut self, speed: f32) {
        self.speed = speed;
    }

    /// The playback rate.
    pub fn speed(&self) -> f32 {
        self.speed
    }

    /// Overrides the loop mode of the current clip.
    pub fn set_loop_mode(&mut self, mode: LoopMode) {
        if let Some(p) = &mut self.current {
            p.mode = mode;
            p.finished = false;
        }
    }

    /// Jumps the current clip to `time` seconds. No events fire for the
    /// skipped range; an event exactly at `time` fires on the next advance.
    pub fn seek(&mut self, time: f32) {
        if let Some(p) = &mut self.current {
            p.seek(time);
        }
    }

    /// The current clip, if any.
    pub fn clip(&self) -> Option<&Arc<Clip>> {
        self.current.as_ref().map(|p| &p.clip)
    }

    /// Time inside the current clip in seconds; 0 when nothing plays.
    pub fn time(&self) -> f32 {
        self.current.as_ref().map_or(0.0, Playback::time)
    }

    /// Position in the current clip, 0 to 1.
    pub fn progress(&self) -> f32 {
        match &self.current {
            Some(p) if p.clip.duration() > 0.0 => p.time() / p.clip.duration(),
            _ => 0.0,
        }
    }

    /// True while a clip is advancing: not paused and not finished.
    pub fn is_playing(&self) -> bool {
        !self.paused && self.current.as_ref().is_some_and(|p| !p.finished)
    }

    /// True when the current `Once` or `Clamp` clip has reached its end.
    pub fn is_finished(&self) -> bool {
        self.current.as_ref().is_some_and(|p| p.finished)
    }

    /// The clips contributing to the pose with their weights. During a
    /// crossfade the incoming clip comes first; weights sum to one.
    pub fn weights(&self) -> Vec<(&str, f32)> {
        let Some(cur) = &self.current else {
            return Vec::new();
        };
        match &self.fade {
            Some(f) => vec![
                (cur.clip.name.as_str(), f.incoming()),
                (f.from.clip.name.as_str(), 1.0 - f.incoming()),
            ],
            None => vec![(cur.clip.name.as_str(), 1.0)],
        }
    }

    /// Moves playback forward by `dt` seconds of update time, scaled by the
    /// speed. Events crossed are queued for [`Player::take_events`], in
    /// playback order, including across loop points and steps longer than
    /// the clip.
    pub fn advance(&mut self, dt: f32) {
        if self.paused {
            return;
        }
        let delta = f64::from(dt) * f64::from(self.speed);
        if let Some(p) = &mut self.current {
            if !p.finished {
                p.advance(delta, &mut self.events, true);
            } else {
                p.last_sweeps.clear();
            }
        }
        if let Some(f) = &mut self.fade {
            f.from.advance(delta, &mut self.events, false);
            f.elapsed += dt;
            if f.elapsed >= f.duration {
                self.fade = None;
            }
        }
    }

    /// Events queued since the last call, oldest first.
    pub fn events(&self) -> &[PlayerEvent] {
        &self.events
    }

    /// Removes and returns the queued events.
    pub fn take_events(&mut self) -> Vec<PlayerEvent> {
        std::mem::take(&mut self.events)
    }

    /// Calls `f` with every track's current value, blended across a
    /// crossfade. A track only the outgoing clip has keeps its last value.
    /// A finished `Once` clip yields nothing.
    pub fn evaluate(&self, mut f: impl FnMut(&str, Value)) {
        let Some(cur) = self.current.as_ref().filter(|p| !p.is_spent()) else {
            return;
        };
        let Some(fade) = &self.fade else {
            cur.clip.for_each_value(cur.time(), f);
            return;
        };
        let w = fade.incoming();
        let mut old: Vec<(String, Value)> = Vec::new();
        fade.from
            .clip
            .for_each_value(fade.from.time(), |n, v| old.push((n.to_string(), v)));
        cur.clip.for_each_value(cur.time(), |name, v| {
            match old.iter().position(|(n, _)| n == name) {
                Some(i) => f(name, old.swap_remove(i).1.blend(&v, w)),
                None => f(name, v),
            }
        });
        for (name, v) in old {
            f(&name, v);
        }
    }

    /// Writes the current pose into `target`.
    pub fn apply(&self, target: &mut impl Target) {
        self.evaluate(|name, v| target.set(name, &v));
    }

    /// The motion of the joint `root` over the last [`Player::advance`],
    /// blended through a crossfade and summed over loop points.
    pub fn root_motion(&self, root: &str) -> RootMotion {
        let Some(cur) = &self.current else {
            return RootMotion::ZERO;
        };
        let w = self.fade.as_ref().map_or(1.0, Fade::incoming);
        let mut m = over_sweeps(&cur.clip, root, &cur.last_sweeps).scaled(w);
        if let Some(f) = &self.fade {
            m = m.plus(over_sweeps(&f.from.clip, root, &f.from.last_sweeps).scaled(1.0 - w));
        }
        m
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curve::Curve;
    use crate::value::Channel;

    fn clip(name: &str, mode: LoopMode, len: f32, events: &[(f32, &str)]) -> Arc<Clip> {
        let mut c = Clip::new(name)
            .with_mode(mode)
            .with_track("x", Channel::Float(Curve::linear([(0.0, 0.0), (len, len)])));
        for (t, n) in events {
            c = c.with_event(*t, *n);
        }
        Arc::new(c)
    }

    fn markers(p: &mut Player) -> Vec<String> {
        p.take_events()
            .into_iter()
            .filter_map(|e| match e {
                PlayerEvent::Marker { name, .. } => Some(name),
                PlayerEvent::Finished { .. } => None,
            })
            .collect()
    }

    fn value(p: &Player) -> f32 {
        let mut out = f32::NAN;
        p.evaluate(|_, v| {
            if let Value::Float(f) = v {
                out = f;
            }
        });
        out
    }

    #[test]
    fn events_fire_across_loop_wrap() {
        let mut p = Player::new();
        p.play(clip("walk", LoopMode::Loop, 1.0, &[(0.0, "start"), (0.2, "a"), (0.9, "b")]));
        p.advance(0.5);
        assert_eq!(markers(&mut p), ["start", "a"]);
        p.advance(0.8);
        assert_eq!(markers(&mut p), ["b", "start", "a"]);
        p.advance(0.1);
        assert!(markers(&mut p).is_empty());
    }

    #[test]
    fn events_fire_for_dt_larger_than_the_clip() {
        let mut p = Player::new();
        p.play(clip("walk", LoopMode::Loop, 1.0, &[(0.5, "step")]));
        p.advance(3.25);
        assert_eq!(markers(&mut p), ["step", "step", "step"]);
        // Once clips fire everything up to the end once, then finish.
        p.play(clip("hit", LoopMode::Once, 1.0, &[(0.5, "mid"), (1.0, "end")]));
        p.advance(10.0);
        assert_eq!(markers(&mut p), ["mid", "end"]);
        assert!(p.is_finished());
        p.advance(1.0);
        assert!(markers(&mut p).is_empty());
    }

    #[test]
    fn finished_fires_once_and_modes_differ() {
        let mut p = Player::new();
        p.play(clip("hit", LoopMode::Once, 1.0, &[]));
        p.advance(0.6);
        assert!(p.events().is_empty());
        p.advance(0.6);
        p.advance(0.6);
        assert_eq!(p.take_events(), [PlayerEvent::Finished { clip: "hit".into() }]);
        assert!(value(&p).is_nan(), "a finished Once clip stops driving targets");

        p.play(clip("hold", LoopMode::Clamp, 1.0, &[]));
        p.advance(5.0);
        assert_eq!(p.take_events().len(), 1);
        assert_eq!(value(&p), 1.0);

        p.play(clip("pp", LoopMode::PingPong, 1.0, &[]));
        p.advance(1.25);
        assert!((value(&p) - 0.75).abs() < 1e-6);
    }

    #[test]
    fn pause_seek_and_speed() {
        let mut p = Player::new();
        p.play(clip("c", LoopMode::Loop, 4.0, &[(1.0, "e")]));
        p.pause();
        p.advance(1.0);
        assert_eq!(p.time(), 0.0);
        p.resume();
        p.set_speed(2.0);
        p.advance(1.0);
        assert_eq!(p.time(), 2.0);
        assert_eq!(markers(&mut p), ["e"]);
        p.seek(0.5);
        assert_eq!(p.time(), 0.5);
        assert!((p.progress() - 0.125).abs() < 1e-6);
        p.set_speed(1.0);
        p.advance(1.0);
        assert_eq!(markers(&mut p), ["e"]);
    }

    #[test]
    fn crossfade_weights_sum_to_one_and_blend_values() {
        let mut p = Player::new();
        p.play(clip("a", LoopMode::Loop, 10.0, &[]));
        p.crossfade(clip("b", LoopMode::Loop, 10.0, &[]), 1.0);
        for _ in 0..5 {
            let w = p.weights();
            assert_eq!(w.len(), 2);
            assert!((w.iter().map(|(_, w)| w).sum::<f32>() - 1.0).abs() < 1e-6);
            p.advance(0.2);
        }
        // Mid-fade: both clips sit at the same time so the blend equals it.
        let mut p = Player::new();
        p.play(Arc::new(Clip::new("a").with_track("x", Channel::Float(Curve::linear([(0.0, 0.0)])))));
        p.crossfade(Arc::new(Clip::new("b").with_track("x", Channel::Float(Curve::linear([(0.0, 10.0)])))), 1.0);
        p.advance(0.25);
        assert!((value(&p) - 2.5).abs() < 1e-5);
        assert_eq!(p.weights()[0].0, "b");
        p.advance(1.0);
        assert_eq!(p.weights(), [("b", 1.0)]);
        assert_eq!(value(&p), 10.0);
    }

    #[test]
    fn outgoing_clip_keeps_firing_events_during_a_fade() {
        let mut p = Player::new();
        p.play(clip("walk", LoopMode::Loop, 1.0, &[(0.3, "step")]));
        p.crossfade(clip("run", LoopMode::Loop, 1.0, &[]), 1.0);
        p.advance(0.5);
        assert_eq!(markers(&mut p), ["step"]);
    }
}
