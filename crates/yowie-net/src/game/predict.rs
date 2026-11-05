//! Client-side prediction with server reconciliation.

use std::collections::VecDeque;

/// Most unacknowledged inputs kept; older ones are dropped.
const MAX_PENDING: usize = 4096;

/// Applies the local player's inputs immediately and corrects the result
/// when the authoritative server state arrives.
///
/// Each input gets a sequence number (returned by [`Predictor::apply`]) to
/// send to the server with the input. The server echoes the sequence of the
/// last input it processed beside its state; pass both to
/// [`Predictor::reconcile`], which adopts the server state and replays the
/// inputs the server has not yet seen.
///
/// `step` must be deterministic and identical on client and server.
pub struct Predictor<S, I, F> {
    state: S,
    step: F,
    pending: VecDeque<(u32, I)>,
    next_seq: u32,
}

impl<S, I, F: Fn(&S, &I) -> S> Predictor<S, I, F> {
    /// Creates a predictor starting at `state`.
    pub fn new(state: S, step: F) -> Self {
        Self {
            state,
            step,
            pending: VecDeque::new(),
            next_seq: 1,
        }
    }

    /// Applies `input` to the predicted state and remembers it until the
    /// server acknowledges it. Returns the input's sequence number, which
    /// starts at 1.
    pub fn apply(&mut self, input: I) -> u32 {
        let seq = self.next_seq;
        self.next_seq = self.next_seq.wrapping_add(1);
        self.state = (self.step)(&self.state, &input);
        self.pending.push_back((seq, input));
        if self.pending.len() > MAX_PENDING {
            self.pending.pop_front();
        }
        seq
    }

    /// Adopts the server's `state`, computed after it processed every input
    /// up to and including `ack`, then replays the inputs after `ack`.
    pub fn reconcile(&mut self, ack: u32, state: S) {
        while self.pending.front().is_some_and(|(s, _)| *s <= ack) {
            self.pending.pop_front();
        }
        self.state = state;
        for (_, input) in &self.pending {
            self.state = (self.step)(&self.state, input);
        }
    }

    /// The predicted state.
    pub fn state(&self) -> &S {
        &self.state
    }

    /// Inputs applied locally and not yet acknowledged.
    pub fn pending(&self) -> usize {
        self.pending.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reconcile_replays_unacknowledged_inputs() {
        // The server applies a speed of 2 per input; the client predicted 1.
        let mut p = Predictor::new(0i32, |s, i: &i32| s + i);
        for _ in 0..5 {
            p.apply(1);
        }
        assert_eq!(*p.state(), 5);
        // The server has processed inputs 1..=3 and ended at 6 (it used 2 each).
        p.reconcile(3, 6);
        assert_eq!(p.pending(), 2);
        assert_eq!(*p.state(), 8);
        p.reconcile(5, 10);
        assert_eq!(p.pending(), 0);
        assert_eq!(*p.state(), 10);
    }

    #[test]
    fn matching_server_state_leaves_prediction_unchanged() {
        let mut p = Predictor::new(0i32, |s, i: &i32| s + i);
        let seqs: Vec<u32> = (1..=4).map(|n| p.apply(n)).collect();
        assert_eq!(seqs, [1, 2, 3, 4]);
        p.reconcile(2, 3);
        assert_eq!(*p.state(), 10);
    }
}
