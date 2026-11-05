//! When particles are born: the rate, bursts and duration of an emitter,
//! shared by the CPU [`System`](crate::System) and the GPU system.

use crate::emitter::{Burst, Emitter};

/// Tracks an emitter's clock and says how many particles are due each
/// step. It does not create particles; callers do, with the birth code in
/// `spawn.rs`.
#[derive(Clone, Debug, Default)]
pub(crate) struct Schedule {
    running: bool,
    time: f32,
    carry: f64,
    pending: Vec<Burst>,
}

impl Schedule {
    /// Starts emitting from time zero with the emitter's bursts queued.
    pub(crate) fn start(&mut self, e: &Emitter) {
        self.running = true;
        self.time = 0.0;
        self.carry = 0.0;
        self.pending = e.bursts.clone();
        self.pending.sort_by(|a, b| b.time.total_cmp(&a.time));
    }

    /// Ends emission over time.
    pub(crate) fn stop(&mut self) {
        self.running = false;
    }

    /// Drops the fraction of a birth carried to the next step, used when
    /// births were refused at the particle limit.
    pub(crate) fn drop_carry(&mut self) {
        self.carry = 0.0;
    }

    /// Whether births still happen over time: running with a rate or a
    /// burst left.
    pub(crate) fn emitting(&self, e: &Emitter) -> bool {
        self.running && (e.rate > 0.0 || !self.pending.is_empty())
    }

    /// Advances the clock by `dt` and returns the births due: the bursts
    /// whose time has come, then the continuous rate with fractions
    /// carried over.
    pub(crate) fn advance(&mut self, e: &Emitter, dt: f32) -> usize {
        if !self.running {
            return 0;
        }
        self.time += dt;
        let mut due = 0usize;
        while self.pending.last().is_some_and(|b| b.time <= self.time) {
            if let Some(b) = self.pending.pop() {
                due += b.count as usize;
            }
        }
        let ending = e.duration.is_some_and(|d| self.time >= d);
        if !ending && e.rate > 0.0 {
            self.carry += f64::from(e.rate) * f64::from(dt);
            let n = self.carry as usize;
            self.carry -= n as f64;
            due += n;
        }
        if ending {
            self.running = false;
        }
        due
    }
}
