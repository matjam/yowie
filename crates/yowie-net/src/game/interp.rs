//! Snapshot interpolation.

use std::collections::VecDeque;

use yowie_core::{Quat, Vec2, Vec3};

/// Linear interpolation between two values.
pub trait Lerp: Sized {
    /// Returns the value `t` of the way from `self` to `other`; `t` is in
    /// 0 to 1.
    fn lerp(&self, other: &Self, t: f32) -> Self;
}

impl Lerp for f32 {
    fn lerp(&self, other: &Self, t: f32) -> Self {
        self + (other - self) * t
    }
}

impl Lerp for f64 {
    fn lerp(&self, other: &Self, t: f32) -> Self {
        self + (other - self) * f64::from(t)
    }
}

impl Lerp for Vec2 {
    fn lerp(&self, other: &Self, t: f32) -> Self {
        Vec2::lerp(*self, *other, t)
    }
}

impl Lerp for Vec3 {
    fn lerp(&self, other: &Self, t: f32) -> Self {
        Vec3::lerp(*self, *other, t)
    }
}

impl Lerp for Quat {
    /// Spherical interpolation.
    fn lerp(&self, other: &Self, t: f32) -> Self {
        self.slerp(*other, t)
    }
}

/// Buffers timestamped snapshots and samples between them, so remote
/// entities move smoothly although updates arrive at a lower rate than
/// frames. Sample at a time slightly in the past (a render delay of one or
/// two snapshot intervals) so a newer snapshot has usually arrived.
#[derive(Debug, Clone)]
pub struct Interpolator<T> {
    snaps: VecDeque<(f64, T)>,
    capacity: usize,
}

impl<T: Clone> Interpolator<T> {
    /// Creates a buffer that keeps the newest `capacity` snapshots
    /// (at least 2).
    pub fn new(capacity: usize) -> Self {
        Self {
            snaps: VecDeque::new(),
            capacity: capacity.max(2),
        }
    }

    /// Adds a snapshot taken at `time`. Snapshots may arrive out of order;
    /// one with an existing time replaces it.
    pub fn push(&mut self, time: f64, value: T) {
        let idx = self.snaps.partition_point(|(t, _)| *t < time);
        match self.snaps.get_mut(idx) {
            Some(s) if s.0 == time => s.1 = value,
            _ => self.snaps.insert(idx, (time, value)),
        }
        while self.snaps.len() > self.capacity {
            self.snaps.pop_front();
        }
    }

    /// Number of buffered snapshots.
    pub fn len(&self) -> usize {
        self.snaps.len()
    }

    /// True when no snapshot is buffered.
    pub fn is_empty(&self) -> bool {
        self.snaps.is_empty()
    }

    /// The newest snapshot.
    pub fn latest(&self) -> Option<&T> {
        self.snaps.back().map(|s| &s.1)
    }

    /// Time of the newest snapshot.
    pub fn latest_time(&self) -> Option<f64> {
        self.snaps.back().map(|s| s.0)
    }

    /// Samples at `time` using `blend(a, b, t)` between the snapshots on
    /// either side. Times before the oldest or after the newest snapshot
    /// return that snapshot (no extrapolation). `None` when empty.
    pub fn sample_with(&self, time: f64, blend: impl Fn(&T, &T, f32) -> T) -> Option<T> {
        let first = self.snaps.front()?;
        let last = self.snaps.back()?;
        if time <= first.0 {
            return Some(first.1.clone());
        }
        if time >= last.0 {
            return Some(last.1.clone());
        }
        let hi = self.snaps.partition_point(|(t, _)| *t <= time);
        let (t0, a) = &self.snaps[hi - 1];
        let (t1, b) = &self.snaps[hi];
        let k = ((time - t0) / (t1 - t0)) as f32;
        Some(blend(a, b, k))
    }
}

impl<T: Clone + Lerp> Interpolator<T> {
    /// Samples at `time` using [`Lerp`].
    pub fn sample(&self, time: f64) -> Option<T> {
        self.sample_with(time, |a, b, t| a.lerp(b, t))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use yowie_core::vec2;

    #[test]
    fn interpolates_between_snapshots() {
        let mut i = Interpolator::new(8);
        i.push(1.0, vec2(0.0, 0.0));
        i.push(2.0, vec2(10.0, 20.0));
        let cases = [
            (0.5, vec2(0.0, 0.0)),
            (1.0, vec2(0.0, 0.0)),
            (1.25, vec2(2.5, 5.0)),
            (1.5, vec2(5.0, 10.0)),
            (2.0, vec2(10.0, 20.0)),
            (3.0, vec2(10.0, 20.0)),
        ];
        for (t, want) in cases {
            let got = i.sample(t).expect("snapshots exist");
            assert!((got - want).length() < 1e-5, "t={t}: {got} != {want}");
        }
    }

    #[test]
    fn out_of_order_and_capacity() {
        let mut i = Interpolator::new(3);
        for t in [3.0, 1.0, 2.0, 4.0, 2.0] {
            i.push(t, t as f32);
        }
        assert_eq!(i.len(), 3);
        assert_eq!(i.sample(2.5), Some(2.5));
        assert_eq!(i.sample(0.0), Some(2.0));
        assert_eq!(i.latest_time(), Some(4.0));
        assert!(Interpolator::<f32>::new(2).sample(1.0).is_none());
    }
}
