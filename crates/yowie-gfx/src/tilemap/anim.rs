//! Animated tiles.

use std::collections::HashMap;

/// Frames a tile cycles through, such as water or torches.
#[derive(Clone, Debug, PartialEq)]
pub struct TileAnimation {
    /// The sheet frames in play order.
    pub frames: Vec<u32>,
    /// Seconds each frame shows. A shorter list repeats its last value; an
    /// empty list means 0.1 seconds.
    pub durations: Vec<f32>,
}

impl TileAnimation {
    /// An animation showing each of `frames` for `seconds`.
    pub fn uniform(frames: impl IntoIterator<Item = u32>, seconds: f32) -> Self {
        Self {
            frames: frames.into_iter().collect(),
            durations: vec![seconds],
        }
    }

    fn duration(&self, index: usize) -> f32 {
        self.durations
            .get(index)
            .or(self.durations.last())
            .copied()
            .unwrap_or(0.1)
    }
}

#[derive(Clone, Debug)]
struct AnimState {
    anim: TileAnimation,
    clock: f32,
    index: usize,
}

/// A set of tile animations keyed by the sheet frame they replace. A cell
/// showing the keyed frame draws whichever frame of the animation is
/// current.
#[derive(Clone, Debug, Default)]
pub struct TileAnimations {
    anims: HashMap<u32, AnimState>,
}

impl TileAnimations {
    /// Makes `frame` play `animation`. An animation with no frames is
    /// ignored.
    pub fn insert(&mut self, frame: u32, animation: TileAnimation) {
        if !animation.frames.is_empty() {
            self.anims.insert(
                frame,
                AnimState {
                    anim: animation,
                    clock: 0.0,
                    index: 0,
                },
            );
        }
    }

    /// Moves every animation forward by `dt` seconds.
    pub fn advance(&mut self, dt: f32) {
        for a in self.anims.values_mut() {
            a.clock += dt;
            // A zero duration would never end the loop; stop after a few
            // laps.
            for _ in 0..a.anim.frames.len().max(1) * 64 {
                let d = a.anim.duration(a.index);
                if d <= 0.0 || a.clock < d {
                    break;
                }
                a.clock -= d;
                a.index = (a.index + 1) % a.anim.frames.len();
            }
        }
    }

    /// The frame a cell showing `frame` draws now.
    pub fn current(&self, frame: u32) -> u32 {
        self.anims
            .get(&frame)
            .and_then(|a| a.anim.frames.get(a.index).copied())
            .unwrap_or(frame)
    }

    /// Whether no animation is set.
    pub fn is_empty(&self) -> bool {
        self.anims.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn animation_advances_and_wraps() {
        let mut set = TileAnimations::default();
        set.insert(5, TileAnimation::uniform([5, 6, 7], 0.5));
        set.insert(9, TileAnimation::uniform([], 1.0));
        let mut seen = Vec::new();
        for _ in 0..8 {
            set.advance(0.25);
            seen.push(set.current(5));
        }
        assert_eq!(seen, [5, 6, 6, 7, 7, 5, 5, 6]);
        assert_eq!(set.current(9), 9, "an empty animation is ignored");
        assert_eq!(set.current(1), 1, "unanimated frames pass through");
    }

    #[test]
    fn per_frame_durations_apply() {
        let mut set = TileAnimations::default();
        set.insert(
            0,
            TileAnimation {
                frames: vec![0, 1, 2],
                durations: vec![1.0, 0.1],
            },
        );
        set.advance(1.05);
        assert_eq!(set.current(0), 1);
        set.advance(0.1);
        assert_eq!(set.current(0), 2);
        set.advance(0.1);
        assert_eq!(set.current(0), 0);
    }
}
