//! Clips: named tracks plus timed events.

use serde::{Deserialize, Serialize};
use yowie_core::Transform;

use crate::timeline::LoopMode;
use crate::value::{Channel, Target, Value};

/// A channel aimed at a named target.
///
/// For a skeleton the name is a joint name; for a sprite or a component it
/// is whatever the [`Target`] implementation matches on, such as `"frame"`
/// or `"tint"`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Track {
    /// The target this track drives.
    pub target: String,
    /// The keyed values.
    pub channel: Channel,
}

/// A named moment in a clip, such as a footstep.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ClipEvent {
    /// Time in seconds from the start of the clip.
    pub time: f32,
    /// The event name reported when playback crosses `time`.
    pub name: String,
}

/// A set of tracks that play together, with events.
///
/// Clips are plain data and can be written in code or loaded with serde.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Clip {
    /// Name used to find the clip in blend spaces and libraries.
    pub name: String,
    /// Default loop mode when a player starts the clip.
    #[serde(default)]
    pub mode: LoopMode,
    /// Overrides the duration, which otherwise is the last key time of any
    /// track or event.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub length: Option<f32>,
    /// The animated properties.
    #[serde(default)]
    pub tracks: Vec<Track>,
    /// Events in time order.
    #[serde(default)]
    pub events: Vec<ClipEvent>,
}

impl Clip {
    /// An empty looping clip.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ..Self::default()
        }
    }

    /// Sets the loop mode.
    pub fn with_mode(mut self, mode: LoopMode) -> Self {
        self.mode = mode;
        self
    }

    /// Sets a fixed duration in seconds.
    pub fn with_length(mut self, seconds: f32) -> Self {
        self.length = Some(seconds);
        self
    }

    /// Adds a track.
    pub fn with_track(mut self, target: impl Into<String>, channel: Channel) -> Self {
        self.tracks.push(Track {
            target: target.into(),
            channel,
        });
        self
    }

    /// Adds an event at `time` seconds, keeping events sorted.
    pub fn with_event(mut self, time: f32, name: impl Into<String>) -> Self {
        self.events.push(ClipEvent {
            time,
            name: name.into(),
        });
        self.events.sort_by(|a, b| a.time.total_cmp(&b.time));
        self
    }

    /// The duration in seconds.
    pub fn duration(&self) -> f32 {
        self.length.unwrap_or_else(|| {
            let tracks = self.tracks.iter().map(|t| t.channel.duration());
            let events = self.events.iter().map(|e| e.time);
            tracks.chain(events).fold(0.0, f32::max)
        })
    }

    /// Calls `f` with the sampled value of every track at `time`.
    pub fn for_each_value(&self, time: f32, mut f: impl FnMut(&str, Value)) {
        for track in &self.tracks {
            if let Some(v) = track.channel.sample(time) {
                f(&track.target, v);
            }
        }
    }

    /// Writes every track's value at `time` into `target`.
    pub fn apply(&self, time: f32, target: &mut impl Target) {
        self.for_each_value(time, |name, v| target.set(name, &v));
    }

    /// The local transform the translation, rotation, scale and transform
    /// tracks of `target` give at `time`, starting from `base`. Returns
    /// `None` when no track names `target`.
    pub fn sample_transform(&self, target: &str, time: f32, base: Transform) -> Option<Transform> {
        let mut found = false;
        let mut out = base;
        for track in self.tracks.iter().filter(|t| t.target == target) {
            match track.channel.sample(time) {
                Some(Value::Translation(v)) => out.translation = v,
                Some(Value::Rotation(q)) => out.rotation = q,
                Some(Value::Scale(s)) => out.scale = s,
                Some(Value::Transform(t)) => out = t,
                _ => continue,
            }
            found = true;
        }
        found.then_some(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curve::Curve;
    use yowie_core::{Vec2, Vec3};

    #[derive(Default)]
    struct Sprite {
        frame: usize,
        pos: Vec2,
    }

    impl Target for Sprite {
        fn set(&mut self, track: &str, value: &Value) {
            match (track, value) {
                ("frame", Value::Frame(f)) => self.frame = *f,
                ("pos", Value::Vec2(p)) => self.pos = *p,
                _ => {}
            }
        }
    }

    fn sample_clip() -> Clip {
        Clip::new("walk")
            .with_track("frame", Channel::Frame(Curve::step([(0.0, 0.0), (0.5, 1.0)])))
            .with_track(
                "pos",
                Channel::Vec2(Curve::linear([(0.0, Vec2::ZERO), (2.0, Vec2::new(4.0, 0.0))])),
            )
            .with_event(1.5, "b")
            .with_event(0.5, "a")
    }

    #[test]
    fn apply_to_trait_target_and_closure() {
        let clip = sample_clip();
        let mut s = Sprite::default();
        clip.apply(1.0, &mut s);
        assert_eq!((s.frame, s.pos), (1, Vec2::new(2.0, 0.0)));
        let mut seen = Vec::new();
        clip.apply(0.0, &mut |name: &str, _: &Value| seen.push(name.to_string()));
        assert_eq!(seen, ["frame", "pos"]);
    }

    #[test]
    fn duration_and_events() {
        let clip = sample_clip();
        assert_eq!(clip.duration(), 2.0);
        assert_eq!(clip.events[0].name, "a");
        assert_eq!(clip.clone().with_length(5.0).duration(), 5.0);
    }

    #[test]
    fn sample_transform_merges_channels() {
        let clip = Clip::new("c").with_track(
            "hip",
            Channel::Translation(Curve::linear([(0.0, Vec3::ZERO), (1.0, Vec3::Y)])),
        );
        let t = clip
            .sample_transform("hip", 0.5, Transform::IDENTITY)
            .unwrap();
        assert_eq!(t.translation, Vec3::new(0.0, 0.5, 0.0));
        assert!(clip.sample_transform("none", 0.5, Transform::IDENTITY).is_none());
    }

    #[test]
    fn serde_round_trip_from_data() {
        let clip = sample_clip();
        let json = serde_json::to_string(&clip).unwrap();
        let back: Clip = serde_json::from_str(&json).unwrap();
        assert_eq!(back, clip);
    }
}
