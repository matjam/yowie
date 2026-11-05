use crate::Timer;

/// Identifies a timer inside a [`Timers`] collection.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TimerId(u64);

/// One firing reported by [`Timers::update`].
#[derive(Clone, Debug, PartialEq)]
pub struct Fired<T> {
    /// The timer that fired.
    pub id: TimerId,
    /// A copy of the data given when the timer was added.
    pub data: T,
    /// Seconds into the update at which the timer fired, between 0 and
    /// `dt`.
    pub at: f64,
}

struct Entry<T> {
    id: TimerId,
    timer: Timer,
    data: T,
}

/// A collection of timers that carry data of type `T` and report which
/// fired on each update.
///
/// Typical `T` is an enum of game events. After [`Timers::update`], match on
/// each [`Fired::data`]. One-shot timers are removed after firing.
pub struct Timers<T> {
    entries: Vec<Entry<T>>,
    next: u64,
}

impl<T> Default for Timers<T> {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            next: 0,
        }
    }
}

impl<T: Clone> Timers<T> {
    /// Creates an empty collection.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a one-shot timer that fires after `seconds`.
    pub fn after(&mut self, seconds: f64, data: T) -> TimerId {
        self.add(Timer::once(seconds), data)
    }

    /// Adds a timer that fires every `seconds` until cancelled.
    pub fn every(&mut self, seconds: f64, data: T) -> TimerId {
        self.add(Timer::repeating(seconds), data)
    }

    /// Adds a prepared [`Timer`].
    pub fn add(&mut self, timer: Timer, data: T) -> TimerId {
        self.next += 1;
        let id = TimerId(self.next);
        self.entries.push(Entry { id, timer, data });
        id
    }

    /// Removes a timer. Returns false when it does not exist or already
    /// fired and was removed.
    pub fn cancel(&mut self, id: TimerId) -> bool {
        let before = self.entries.len();
        self.entries.retain(|e| e.id != id);
        self.entries.len() != before
    }

    /// Pauses one timer. Returns false when it does not exist.
    pub fn pause(&mut self, id: TimerId) -> bool {
        self.with(id, Timer::pause)
    }

    /// Resumes one timer. Returns false when it does not exist.
    pub fn resume(&mut self, id: TimerId) -> bool {
        self.with(id, Timer::resume)
    }

    fn with(&mut self, id: TimerId, f: impl FnOnce(&mut Timer)) -> bool {
        match self.entries.iter_mut().find(|e| e.id == id) {
            Some(e) => {
                f(&mut e.timer);
                true
            }
            None => false,
        }
    }

    /// Returns the timer for `id`, for reading its remaining time.
    pub fn get(&self, id: TimerId) -> Option<&Timer> {
        self.entries.iter().find(|e| e.id == id).map(|e| &e.timer)
    }

    /// Removes every timer.
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Number of timers waiting to fire.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns true when no timers are waiting.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Advances every timer by `dt` seconds and returns the firings in the
    /// order they happened. Firings at the same instant are ordered by
    /// the order the timers were added.
    pub fn update(&mut self, dt: f64) -> Vec<Fired<T>> {
        let mut fired = Vec::new();
        for e in &mut self.entries {
            let (id, data) = (e.id, &e.data);
            e.timer.tick(dt, |at| {
                fired.push(Fired {
                    id,
                    data: data.clone(),
                    at,
                });
            });
        }
        self.entries.retain(|e| !e.timer.finished());
        fired.sort_by(|a, b| a.at.total_cmp(&b.at).then(a.id.cmp(&b.id)));
        fired
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_firings_in_time_order() {
        let mut t = Timers::new();
        t.after(1.5, "late");
        t.every(1.0, "tick");
        t.after(0.5, "early");
        let names: Vec<&str> = t.update(2.2).into_iter().map(|f| f.data).collect();
        assert_eq!(names, ["early", "tick", "late", "tick"]);
        // One-shots are gone; the repeater stays.
        assert_eq!(t.len(), 1);
    }

    #[test]
    fn cancel_pause_resume() {
        let mut t = Timers::new();
        let a = t.after(1.0, 1);
        let b = t.after(1.0, 2);
        assert!(t.pause(a));
        assert!(t.cancel(b));
        assert!(!t.cancel(b));
        assert!(t.update(2.0).is_empty());
        assert!(t.resume(a));
        let fired = t.update(1.0);
        assert_eq!(fired.len(), 1);
        assert_eq!(fired[0].id, a);
        assert!(t.is_empty());
        assert!(!t.pause(a));
    }

    #[test]
    fn get_and_clear() {
        let mut t = Timers::new();
        let id = t.after(2.0, ());
        t.update(0.5);
        assert!((t.get(id).map(Timer::remaining).unwrap_or(0.0) - 1.5).abs() < 1e-9);
        t.clear();
        assert!(t.get(id).is_none());
    }
}
