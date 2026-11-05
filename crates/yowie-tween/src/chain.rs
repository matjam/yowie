use crate::{Lerp, Tween};

/// Tweens played one after another. Time left over when one finishes
/// carries into the next, so a chain has no gaps or hitches.
///
/// Build one with [`Tween::then`] or [`Chain::new`].
#[derive(Clone, Debug)]
pub struct Chain<T> {
    steps: Vec<Tween<T>>,
    index: usize,
    value: T,
}

impl<T: Lerp + Clone> Chain<T> {
    /// Starts a chain with `first`.
    pub fn new(first: Tween<T>) -> Self {
        let value = first.value();
        Self {
            steps: vec![first],
            index: 0,
            value,
        }
    }

    /// Appends a tween to play after the ones already in the chain.
    pub fn then(mut self, next: Tween<T>) -> Self {
        self.steps.push(next);
        self
    }

    /// Advances by `dt` seconds and returns the current value.
    pub fn update(&mut self, mut dt: f32) -> T {
        while let Some(step) = self.steps.get_mut(self.index) {
            self.value = step.update(dt);
            if !step.done() {
                break;
            }
            dt = step.overshoot().max(0.0);
            self.index += 1;
        }
        self.value.clone()
    }

    /// The current value without advancing.
    pub fn value(&self) -> T {
        self.value.clone()
    }

    /// Returns true after the last tween has finished.
    pub fn done(&self) -> bool {
        self.index >= self.steps.len()
    }

    /// Restarts every tween from the beginning.
    pub fn reset(&mut self) {
        self.index = 0;
        for step in &mut self.steps {
            step.reset();
        }
        if let Some(first) = self.steps.first() {
            self.value = first.value();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn carries_time_between_steps() {
        let mut c = Tween::new(0.0_f32, 1.0, 1.0).then(Tween::new(1.0, 3.0, 2.0));
        assert_eq!(c.update(0.5), 0.5);
        // 0.5 finishes the first, 1.0 more goes into the second.
        assert_eq!(c.update(1.5), 2.0);
        assert!(!c.done());
        assert_eq!(c.update(10.0), 3.0);
        assert!(c.done());
        assert_eq!(c.update(1.0), 3.0);
    }

    #[test]
    fn reset_replays() {
        let mut c = Chain::new(Tween::new(0.0_f32, 2.0, 1.0));
        c.update(5.0);
        assert!(c.done());
        c.reset();
        assert!(!c.done());
        assert_eq!(c.value(), 0.0);
        assert_eq!(c.update(0.5), 1.0);
    }
}
