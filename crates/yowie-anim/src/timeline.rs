//! Loop modes and the mapping from playback position to clip time.

use serde::{Deserialize, Serialize};

/// What playback does at the end of a clip.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum LoopMode {
    /// Play to the end, fire the finished event and stop driving targets.
    Once,
    /// Start over at the end.
    #[default]
    Loop,
    /// Play forwards, then backwards, then forwards again.
    PingPong,
    /// Play to the end, fire the finished event and keep applying the last
    /// pose.
    Clamp,
}

/// The most clip-length pieces one advance reports; a step covering more
/// loops than this keeps the most recent ones.
const MAX_PIECES: i64 = 4096;

/// A stretch of clip time that one advance swept over.
///
/// A sweep with `to >= from` moves forwards and covers `(from, to]`; a
/// backwards sweep covers `[to, from)`. With `inclusive_start` the starting
/// time is covered too.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sweep {
    /// Clip time the sweep starts at, in seconds.
    pub from: f32,
    /// Clip time the sweep ends at, in seconds.
    pub to: f32,
    /// Whether `from` itself counts as crossed.
    pub inclusive_start: bool,
}

impl Sweep {
    /// True when `time` lies in the swept range.
    pub fn contains(&self, time: f32) -> bool {
        if self.to >= self.from {
            let after = if self.inclusive_start {
                time >= self.from
            } else {
                time > self.from
            };
            after && time <= self.to
        } else {
            let before = if self.inclusive_start {
                time <= self.from
            } else {
                time < self.from
            };
            before && time >= self.to
        }
    }
}

/// Maps an unwrapped playback position `x` (seconds travelled, may be
/// negative when playing backwards) to a time inside a clip of `duration`.
pub fn local_time(mode: LoopMode, duration: f32, x: f64) -> f32 {
    let d = f64::from(duration);
    if d <= 0.0 {
        return 0.0;
    }
    let t = match mode {
        LoopMode::Once | LoopMode::Clamp => x.clamp(0.0, d),
        LoopMode::Loop => x.rem_euclid(d),
        LoopMode::PingPong => {
            let m = x.rem_euclid(2.0 * d);
            if m <= d { m } else { 2.0 * d - m }
        }
    };
    t as f32
}

/// The clip-time sweeps made by moving from position `x0` to `x1`.
///
/// `first` marks the first advance after playback starts or seeks, which
/// covers the start time too so an event at time 0 fires. Loops split the
/// movement into one sweep per lap, so crossings stay ordered across wraps
/// and across steps longer than the clip.
pub fn sweeps(mode: LoopMode, duration: f32, x0: f64, x1: f64, first: bool) -> Vec<Sweep> {
    let d = f64::from(duration);
    if d <= 0.0 {
        return Vec::new();
    }
    if matches!(mode, LoopMode::Once | LoopMode::Clamp) {
        let (a, b) = (local_time(mode, duration, x0), local_time(mode, duration, x1));
        if a == b && !first {
            return Vec::new();
        }
        return vec![Sweep {
            from: a,
            to: b,
            inclusive_start: first,
        }];
    }
    if x0 == x1 {
        return if first {
            let t = local_time(mode, duration, x0);
            vec![Sweep {
                from: t,
                to: t,
                inclusive_start: true,
            }]
        } else {
            Vec::new()
        };
    }
    let forward = x1 > x0;
    let (lo, hi) = if forward { (x0, x1) } else { (x1, x0) };
    let (h_lo, h_hi) = ((lo / d).floor() as i64, (hi / d).floor() as i64);
    let ping = mode == LoopMode::PingPong;
    let at = |x: f64, h: i64| -> f32 {
        let off = x - h as f64 * d;
        let t = if ping && h.rem_euclid(2) == 1 { d - off } else { off };
        t as f32
    };
    let laps: Vec<i64> = if forward {
        (h_lo..=h_hi).collect()
    } else {
        (h_lo..=h_hi).rev().collect()
    };
    let skip = laps.len().saturating_sub(MAX_PIECES as usize);
    let mut out = Vec::with_capacity(laps.len() - skip);
    for (n, &h) in laps.iter().enumerate().skip(skip) {
        let (p_lo, p_hi) = (lo.max(h as f64 * d), hi.min((h + 1) as f64 * d));
        if !forward && p_lo == p_hi {
            continue;
        }
        let (xa, xb) = if forward { (p_lo, p_hi) } else { (p_hi, p_lo) };
        let inclusive_start = if n == 0 { first } else { !ping };
        out.push(Sweep {
            from: at(xa, h),
            to: at(xb, h),
            inclusive_start,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fired(sw: &[Sweep], times: &[f32]) -> Vec<f32> {
        sw.iter()
            .flat_map(|s| {
                let mut hit: Vec<f32> = times.iter().copied().filter(|t| s.contains(*t)).collect();
                hit.sort_by(|a, b| a.total_cmp(b));
                if s.to < s.from {
                    hit.reverse();
                }
                hit
            })
            .collect()
    }

    #[test]
    fn local_time_by_mode() {
        // (mode, x, expected) for a 2 second clip.
        let cases = [
            (LoopMode::Once, 3.0, 2.0),
            (LoopMode::Clamp, -1.0, 0.0),
            (LoopMode::Loop, 2.5, 0.5),
            (LoopMode::Loop, -0.5, 1.5),
            (LoopMode::PingPong, 2.5, 1.5),
            (LoopMode::PingPong, 4.5, 0.5),
        ];
        for (mode, x, want) in cases {
            assert!((local_time(mode, 2.0, x) - want).abs() < 1e-6, "{mode:?} {x}");
        }
    }

    #[test]
    fn loop_wrap_fires_in_order() {
        // Clip of 1 s with events at 0.2 and 0.9; move 0.8 -> 1.5.
        let sw = sweeps(LoopMode::Loop, 1.0, 0.8, 1.5, false);
        assert_eq!(fired(&sw, &[0.2, 0.9]), vec![0.9, 0.2]);
    }

    #[test]
    fn large_step_fires_every_lap() {
        let sw = sweeps(LoopMode::Loop, 1.0, 0.0, 3.5, true);
        let got = fired(&sw, &[0.0, 0.25]);
        // Time 0 at the start and on each of the three wraps; 0.25 on four laps.
        assert_eq!(got.iter().filter(|t| **t == 0.0).count(), 4);
        assert_eq!(got.iter().filter(|t| **t == 0.25).count(), 4);
        let once = sweeps(LoopMode::Once, 1.0, 0.0, 99.0, true);
        assert_eq!(fired(&once, &[0.0, 0.5, 1.0]), vec![0.0, 0.5, 1.0]);
    }

    #[test]
    fn ping_pong_turnaround_fires_once() {
        let sw = sweeps(LoopMode::PingPong, 1.0, 0.5, 1.5, false);
        assert_eq!(fired(&sw, &[1.0, 0.75]), vec![0.75, 1.0, 0.75]);
        // Walking back over the same range fires in reverse order.
        let back = sweeps(LoopMode::Loop, 1.0, 0.6, 0.1, false);
        assert_eq!(fired(&back, &[0.2, 0.5]), vec![0.5, 0.2]);
    }

    #[test]
    fn no_movement_fires_nothing_after_start() {
        assert!(sweeps(LoopMode::Loop, 1.0, 0.3, 0.3, false).is_empty());
        assert_eq!(sweeps(LoopMode::Loop, 1.0, 0.0, 0.0, true).len(), 1);
    }
}
