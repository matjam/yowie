//! Frame-by-frame sprite animation.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::timeline::{LoopMode, local_time};

/// One frame of a [`Flipbook`].
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct FlipFrame {
    /// Index of the frame in the sprite sheet or atlas.
    pub index: usize,
    /// How long the frame shows, in seconds.
    pub duration: f32,
}

/// The order a tag plays its frames in. Matches Aseprite's tag directions.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TagDirection {
    /// First to last.
    #[default]
    Forward,
    /// Last to first.
    Reverse,
    /// First to last, then back to the first.
    PingPong,
}

/// A named range of frames, such as `"walk"` or `"attack"`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Tag {
    /// Tag name.
    pub name: String,
    /// Position in the flipbook's frame list of the first frame.
    pub from: usize,
    /// Position of the last frame, inclusive.
    pub to: usize,
    /// Playback order.
    #[serde(default)]
    pub direction: TagDirection,
}

/// A list of frames with durations and named tags.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Flipbook {
    /// The frames in order.
    pub frames: Vec<FlipFrame>,
    /// Named ranges of `frames`.
    #[serde(default)]
    pub tags: Vec<Tag>,
}

impl Flipbook {
    /// A flipbook of `indices` that all show for `1 / fps` seconds.
    pub fn from_fps(indices: impl IntoIterator<Item = usize>, fps: f32) -> Self {
        let duration = if fps > 0.0 { 1.0 / fps } else { 0.0 };
        Self {
            frames: indices
                .into_iter()
                .map(|index| FlipFrame { index, duration })
                .collect(),
            tags: Vec::new(),
        }
    }

    /// Adds a tag over frame positions `from..=to`.
    pub fn with_tag(
        mut self,
        name: impl Into<String>,
        from: usize,
        to: usize,
        direction: TagDirection,
    ) -> Self {
        self.tags.push(Tag {
            name: name.into(),
            from,
            to,
            direction,
        });
        self
    }

    /// The tag called `name`.
    pub fn tag(&self, name: &str) -> Option<&Tag> {
        self.tags.iter().find(|t| t.name == name)
    }
}

/// Plays a [`Flipbook`] or one of its tags.
#[derive(Clone, Debug)]
pub struct FlipbookPlayer {
    book: Arc<Flipbook>,
    /// Frames being played as (sheet index, duration), in play order.
    sequence: Vec<FlipFrame>,
    mode: LoopMode,
    x: f64,
    speed: f32,
    finished: bool,
}

impl FlipbookPlayer {
    /// Plays every frame of `book` in order, looping.
    pub fn new(book: Arc<Flipbook>) -> Self {
        let sequence = book.frames.clone();
        Self {
            book,
            sequence,
            mode: LoopMode::Loop,
            x: 0.0,
            speed: 1.0,
            finished: false,
        }
    }

    /// Switches to the frames of the tag `name` from its start, using the
    /// tag's direction. Returns false when the tag does not exist.
    pub fn play_tag(&mut self, name: &str, mode: LoopMode) -> bool {
        let Some(tag) = self.book.tag(name).cloned() else {
            return false;
        };
        let (lo, hi) = (tag.from.min(tag.to), tag.from.max(tag.to));
        let hi = hi.min(self.book.frames.len().saturating_sub(1));
        let mut seq: Vec<FlipFrame> = self.book.frames.get(lo..=hi).unwrap_or(&[]).to_vec();
        if tag.direction == TagDirection::Reverse {
            seq.reverse();
        }
        self.sequence = seq;
        self.mode = if tag.direction == TagDirection::PingPong && mode == LoopMode::Loop {
            LoopMode::PingPong
        } else {
            mode
        };
        self.x = 0.0;
        self.finished = false;
        true
    }

    /// Sets the playback rate; 1 is normal.
    pub fn set_speed(&mut self, speed: f32) {
        self.speed = speed;
    }

    /// Total length of the frames being played, in seconds.
    pub fn duration(&self) -> f32 {
        self.sequence.iter().map(|f| f.duration).sum()
    }

    /// Moves forward by `dt` seconds of update time.
    pub fn advance(&mut self, dt: f32) {
        let d = f64::from(self.duration());
        self.x += f64::from(dt) * f64::from(self.speed);
        if d > 0.0 {
            match self.mode {
                LoopMode::Loop => self.x = self.x.rem_euclid(d),
                LoopMode::PingPong => self.x = self.x.rem_euclid(2.0 * d),
                LoopMode::Once | LoopMode::Clamp => {
                    if self.x >= d {
                        self.finished = true;
                    }
                    self.x = self.x.clamp(0.0, d);
                }
            }
        }
    }

    /// True once a `Once` or `Clamp` run has reached its last frame's end.
    pub fn is_finished(&self) -> bool {
        self.finished
    }

    /// The sheet index of the frame showing now, or `None` for an empty
    /// flipbook.
    pub fn frame(&self) -> Option<usize> {
        let t = local_time(self.mode, self.duration(), self.x);
        let mut end = 0.0;
        for f in &self.sequence {
            end += f.duration;
            if t < end {
                return Some(f.index);
            }
        }
        self.sequence.last().map(|f| f.index)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn book() -> Arc<Flipbook> {
        let mut b = Flipbook::from_fps([10, 11, 12, 13], 10.0)
            .with_tag("idle", 0, 1, TagDirection::Forward)
            .with_tag("back", 1, 3, TagDirection::Reverse)
            .with_tag("bounce", 0, 2, TagDirection::PingPong);
        b.frames[3].duration = 0.3;
        Arc::new(b)
    }

    /// Samples mid-frame of a 0.1 s grid to stay clear of rounding at edges.
    fn frames(p: &mut FlipbookPlayer, steps: usize) -> Vec<usize> {
        p.advance(0.05);
        (0..steps)
            .map(|_| {
                let f = p.frame().unwrap();
                p.advance(0.1);
                f
            })
            .collect()
    }

    #[test]
    fn plays_whole_book_and_loops() {
        let mut p = FlipbookPlayer::new(book());
        assert_eq!(frames(&mut p, 8), [10, 11, 12, 13, 13, 13, 10, 11]);
    }

    #[test]
    fn tags_follow_direction() {
        let mut p = FlipbookPlayer::new(book());
        assert!(p.play_tag("back", LoopMode::Loop));
        assert_eq!(frames(&mut p, 5), [13, 13, 13, 12, 11]);
        assert!(p.play_tag("bounce", LoopMode::Loop));
        assert_eq!(frames(&mut p, 7), [10, 11, 12, 12, 11, 10, 10]);
        assert!(!p.play_tag("missing", LoopMode::Loop));
    }

    #[test]
    fn once_finishes_on_last_frame() {
        let mut p = FlipbookPlayer::new(book());
        p.play_tag("idle", LoopMode::Once);
        p.advance(0.15);
        assert_eq!((p.frame(), p.is_finished()), (Some(11), false));
        p.advance(1.0);
        assert_eq!((p.frame(), p.is_finished()), (Some(11), true));
    }
}
