/// A step in a [`Sequence`].
enum Step {
    Wait(f64),
    Call(Box<dyn FnMut() + Send>),
    Until(Box<dyn FnMut() -> bool + Send>),
    Run(Box<dyn FnMut(f64) -> bool + Send>),
    Parallel(Vec<Sequence>),
}

/// A list of steps run one after another, advanced by `dt`. Use it for
/// cutscenes and scripted events.
///
/// Time left over when a wait ends carries into the next step, so a long
/// `dt` can run several steps in one update.
///
/// ```
/// use yowie_timer::Sequence;
///
/// let mut seq = Sequence::new().wait(1.0).call(|| println!("boom"));
/// assert!(!seq.update(0.5));
/// assert!(seq.update(0.5));
/// ```
#[derive(Default)]
pub struct Sequence {
    steps: Vec<Step>,
    index: usize,
    waited: f64,
    looping: bool,
    /// Per-child finish state while a parallel step runs: the unused time
    /// of the update in which the child finished.
    parallel: Vec<Option<f64>>,
}

impl Sequence {
    /// Creates an empty sequence. An empty sequence is already done.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a step that waits `seconds` of game time.
    pub fn wait(mut self, seconds: f64) -> Self {
        self.steps.push(Step::Wait(seconds));
        self
    }

    /// Adds a step that calls `f` once.
    pub fn call(mut self, f: impl FnMut() + Send + 'static) -> Self {
        self.steps.push(Step::Call(Box::new(f)));
        self
    }

    /// Adds a step that waits until `cond` returns true. It is checked on
    /// every update.
    pub fn wait_until(mut self, cond: impl FnMut() -> bool + Send + 'static) -> Self {
        self.steps.push(Step::Until(Box::new(cond)));
        self
    }

    /// Adds a step that calls `f(dt)` on every update until it returns
    /// true. Use it to drive an animation for as long as it needs.
    pub fn run(mut self, f: impl FnMut(f64) -> bool + Send + 'static) -> Self {
        self.steps.push(Step::Run(Box::new(f)));
        self
    }

    /// Adds a group of sequences that run at the same time. The step ends
    /// when every one of them is done.
    pub fn parallel(mut self, group: Vec<Sequence>) -> Self {
        self.steps.push(Step::Parallel(group));
        self
    }

    /// Makes the sequence start again from the first step after the last.
    /// A looping sequence never reports done.
    pub fn looped(mut self) -> Self {
        self.looping = true;
        self
    }

    /// Advances by `dt` seconds. Returns true once every step is done.
    pub fn update(&mut self, dt: f64) -> bool {
        self.advance(dt).is_some()
    }

    /// Returns true when every step has run.
    pub fn done(&self) -> bool {
        self.index >= self.steps.len()
    }

    /// Goes back to the first step.
    pub fn reset(&mut self) {
        self.index = 0;
        self.waited = 0.0;
        self.parallel.clear();
        for step in &mut self.steps {
            if let Step::Parallel(group) = step {
                group.iter_mut().for_each(Sequence::reset);
            }
        }
    }

    /// Marks the sequence done without running the remaining steps.
    pub fn skip(&mut self) {
        self.index = self.steps.len();
        self.waited = 0.0;
        self.parallel.clear();
    }

    /// Runs steps with `dt`. Returns the unused part of `dt` when the
    /// sequence finished, or `None` while it is still running.
    fn advance(&mut self, mut dt: f64) -> Option<f64> {
        let mut wrapped = false;
        while self.index < self.steps.len() {
            match &mut self.steps[self.index] {
                Step::Call(f) => f(),
                Step::Until(cond) => {
                    if !cond() {
                        return None;
                    }
                }
                Step::Run(f) => {
                    if !f(dt) {
                        return None;
                    }
                    dt = 0.0;
                }
                Step::Wait(seconds) => {
                    self.waited += dt;
                    if self.waited < *seconds {
                        return None;
                    }
                    dt = self.waited - *seconds;
                    self.waited = 0.0;
                }
                Step::Parallel(group) => {
                    let left = Self::advance_group(group, &mut self.parallel, dt)?;
                    dt = left;
                }
            }
            self.index += 1;
            if self.index >= self.steps.len() && self.looping {
                self.index = 0;
                // A loop goes round at most once per update, and not at
                // all when it would spin without time passing.
                if dt <= 0.0 || wrapped {
                    return None;
                }
                wrapped = true;
            }
        }
        Some(dt)
    }

    fn advance_group(group: &mut [Sequence], state: &mut Vec<Option<f64>>, dt: f64) -> Option<f64> {
        if state.len() != group.len() {
            *state = vec![None; group.len()];
        }
        for (child, slot) in group.iter_mut().zip(state.iter_mut()) {
            match slot {
                // Finished in an earlier update: no time is left over now.
                Some(left) => *left = f64::INFINITY,
                None => *slot = child.advance(dt),
            }
        }
        if state.iter().all(Option::is_some) {
            let left = state.iter().flatten().fold(f64::INFINITY, |a, &b| a.min(b));
            state.clear();
            return Some(if left.is_finite() { left } else { 0.0 });
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    fn log() -> Arc<Mutex<Vec<&'static str>>> {
        Arc::new(Mutex::new(Vec::new()))
    }

    fn mark(
        log: &Arc<Mutex<Vec<&'static str>>>,
        name: &'static str,
    ) -> impl FnMut() + Send + use<> {
        let log = Arc::clone(log);
        move || {
            if let Ok(mut l) = log.lock() {
                l.push(name);
            }
        }
    }

    fn seen(log: &Arc<Mutex<Vec<&'static str>>>) -> Vec<&'static str> {
        log.lock().map(|l| l.clone()).unwrap_or_default()
    }

    #[test]
    fn waits_then_calls() {
        let l = log();
        let mut s = Sequence::new()
            .call(mark(&l, "a"))
            .wait(1.0)
            .call(mark(&l, "b"));
        assert!(!s.update(0.5));
        assert_eq!(seen(&l), ["a"]);
        assert!(s.update(0.5));
        assert_eq!(seen(&l), ["a", "b"]);
        assert!(s.done());
    }

    #[test]
    fn leftover_time_carries_between_waits() {
        let l = log();
        let mut s = Sequence::new()
            .wait(1.0)
            .call(mark(&l, "a"))
            .wait(1.0)
            .call(mark(&l, "b"));
        assert!(s.update(2.5));
        assert_eq!(seen(&l), ["a", "b"]);
    }

    #[test]
    fn wait_until_and_run() {
        let flag = Arc::new(Mutex::new(false));
        let f = Arc::clone(&flag);
        let mut total = 0.0;
        let mut s = Sequence::new()
            .wait_until(move || f.lock().map(|v| *v).unwrap_or(true))
            .run(move |dt| {
                total += dt;
                total >= 1.0
            });
        assert!(!s.update(5.0));
        if let Ok(mut v) = flag.lock() {
            *v = true;
        }
        assert!(!s.update(0.5));
        assert!(s.update(0.5));
    }

    #[test]
    fn parallel_waits_for_all() {
        let l = log();
        let group = vec![
            Sequence::new().wait(1.0).call(mark(&l, "short")),
            Sequence::new().wait(3.0).call(mark(&l, "long")),
        ];
        let mut s = Sequence::new().parallel(group).call(mark(&l, "after"));
        assert!(!s.update(1.5));
        assert_eq!(seen(&l), ["short"]);
        assert!(!s.update(1.0));
        assert!(s.update(0.5));
        assert_eq!(seen(&l), ["short", "long", "after"]);
    }

    #[test]
    fn parallel_carries_leftover_from_slowest() {
        let l = log();
        let group = vec![Sequence::new().wait(1.0), Sequence::new().wait(2.0)];
        let mut s = Sequence::new()
            .parallel(group)
            .wait(1.0)
            .call(mark(&l, "x"));
        // 3.0 elapsed: group ends at 2.0, one second remains for the wait.
        assert!(s.update(3.0));
        assert_eq!(seen(&l), ["x"]);
    }

    #[test]
    fn loop_reset_and_skip() {
        let l = log();
        let mut s = Sequence::new().wait(1.0).call(mark(&l, "t")).looped();
        assert!(!s.update(2.5));
        assert_eq!(seen(&l).len(), 2);
        assert!(!s.update(0.0));
        s.skip();
        assert!(s.done());
        s.reset();
        assert!(!s.done());
        let mut empty = Sequence::new();
        assert!(empty.update(0.0));
    }
}
