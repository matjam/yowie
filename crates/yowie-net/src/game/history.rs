//! Lag compensation: a short history of world state to rewind to.

use std::collections::VecDeque;

/// Keeps timestamped values for a window of recent time. On the server,
/// record each entity's state every tick; when a client's action arrives,
/// look up the state at the time that client saw (its send time minus its
/// interpolation delay) and test the action against that.
#[derive(Debug, Clone)]
pub struct History<T> {
    items: VecDeque<(f64, T)>,
    window: f64,
}

impl<T: Clone> History<T> {
    /// Creates a history that forgets entries older than `window` seconds
    /// before the newest.
    pub fn new(window: f64) -> Self {
        Self {
            items: VecDeque::new(),
            window,
        }
    }

    /// Records `value` at `time`. Times should not decrease; an entry older
    /// than the newest is ignored.
    pub fn record(&mut self, time: f64, value: T) {
        if self.items.back().is_some_and(|(t, _)| time < *t) {
            return;
        }
        self.items.push_back((time, value));
        while self
            .items
            .front()
            .is_some_and(|(t, _)| time - *t > self.window)
        {
            self.items.pop_front();
        }
    }

    /// Number of entries kept.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// True when nothing is recorded.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// The entry recorded closest in time to `time`. `None` when empty.
    pub fn nearest(&self, time: f64) -> Option<&T> {
        let hi = self.items.partition_point(|(t, _)| *t < time);
        let after = self.items.get(hi);
        let before = hi.checked_sub(1).and_then(|i| self.items.get(i));
        match (before, after) {
            (Some(b), Some(a)) => Some(if time - b.0 <= a.0 - time { &b.1 } else { &a.1 }),
            (Some(e), None) | (None, Some(e)) => Some(&e.1),
            (None, None) => None,
        }
    }

    /// The value at `time`, blended between the entries on either side with
    /// `blend(a, b, t)`. Times outside the recorded range clamp to the
    /// nearest end.
    pub fn at_with(&self, time: f64, blend: impl Fn(&T, &T, f32) -> T) -> Option<T> {
        let first = self.items.front()?;
        let last = self.items.back()?;
        if time <= first.0 {
            return Some(first.1.clone());
        }
        if time >= last.0 {
            return Some(last.1.clone());
        }
        let hi = self.items.partition_point(|(t, _)| *t <= time);
        let (t0, a) = &self.items[hi - 1];
        let (t1, b) = &self.items[hi];
        Some(blend(a, b, ((time - t0) / (t1 - t0)) as f32))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rewinds_and_expires() {
        let mut h = History::new(1.0);
        for i in 0..=30 {
            h.record(f64::from(i) * 0.1, f64::from(i));
        }
        assert!(h.len() <= 11, "len {}", h.len());
        assert_eq!(h.nearest(2.04), Some(&20.0));
        assert_eq!(h.nearest(2.06), Some(&21.0));
        assert_eq!(h.nearest(100.0), Some(&30.0));
        let mid = h.at_with(2.55, |a, b, t| a + (b - a) * f64::from(t));
        assert!((mid.expect("in range") - 25.5).abs() < 1e-4);
        assert!(History::<f64>::new(1.0).nearest(0.0).is_none());
    }
}
