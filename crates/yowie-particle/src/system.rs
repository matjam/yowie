//! The CPU simulation of one emitter.

use yowie_core::Vec2;
use yowie_gfx::{SpriteSheet, Texture};
use yowie_rng::Rng;

use crate::emitter::{Burst, Emitter};
use crate::spawn::{Particle, birth};

/// Runs one [`Emitter`] on the CPU: births particles, moves them and
/// draws them.
///
/// Create it with [`System::new`], call [`update`](Self::update) once per
/// update step and [`draw`](Self::draw) once per frame:
///
/// ```
/// use yowie_core::vec2;
/// use yowie_particle::{System, presets};
///
/// let mut sparks = System::new(presets::sparks());
/// sparks.set_position(vec2(100.0, 80.0));
/// sparks.update(1.0 / 60.0);
/// assert!(sparks.alive() > 0);
/// ```
///
/// The simulation is separate from drawing: [`Particle`]s are plain data
/// reachable through [`particles`](Self::particles), and the birth code
/// takes only the emitter and a random stream, so a GPU-instanced system
/// can share it.
#[derive(Clone, Debug)]
pub struct System {
    emitter: Emitter,
    position: Vec2,
    pub(crate) live: Vec<Particle>,
    rng: Rng,
    carry: f64,
    running: bool,
    time: f32,
    pending: Vec<Burst>,
    pub(crate) texture: Option<Texture>,
    pub(crate) sheet: Option<SpriteSheet>,
}

impl System {
    /// Makes a system for `emitter` and starts it: the emitter's `burst`
    /// is born at its position and its `prewarm` runs.
    pub fn new(emitter: Emitter) -> Self {
        Self::at(emitter, Vec2::ZERO)
    }

    /// Like [`new`](Self::new) with the system at `position`, so the
    /// starting burst and the prewarm are born there. A world-space
    /// system whose position is set after `new` would have already
    /// created those particles at the origin.
    pub fn at(emitter: Emitter, position: Vec2) -> Self {
        let mut s = Self {
            live: Vec::with_capacity(emitter.max.min(1 << 16)),
            rng: Rng::new(emitter.seed),
            emitter,
            position,
            carry: 0.0,
            running: false,
            time: 0.0,
            pending: Vec::new(),
            texture: None,
            sheet: None,
        };
        s.start();
        s
    }

    /// Draws particles with `texture`. Without one they are solid
    /// squares. Particle sizes of 0 in the emitter take the texture's
    /// width.
    pub fn set_texture(&mut self, texture: Texture) {
        self.texture = Some(texture);
        self.sheet = None;
    }

    /// Draws particles with the frames of `sheet`, chosen over each
    /// particle's life by the emitter's `frames` and `frame_over_life`.
    pub fn set_sheet(&mut self, sheet: SpriteSheet) {
        self.sheet = Some(sheet);
        self.texture = None;
    }

    /// The emitter the system runs.
    pub fn emitter(&self) -> &Emitter {
        &self.emitter
    }

    /// Replaces the emitter without disturbing live particles: later births
    /// use the new settings, and every particle is drawn and moved by
    /// them. The random stream and the position are kept.
    pub fn set_emitter(&mut self, emitter: Emitter) {
        self.emitter = emitter;
    }

    /// Moves the system. In a world-space system live particles stay where
    /// they are; otherwise they move with it.
    pub fn set_position(&mut self, position: Vec2) {
        self.position = position;
    }

    /// Where the system is.
    pub fn position(&self) -> Vec2 {
        self.position
    }

    /// Begins emitting from time zero: births the emitter's `burst` and
    /// runs its `prewarm`. [`new`](Self::new) calls it; call it again to
    /// restart a stopped system.
    pub fn start(&mut self) {
        self.running = true;
        self.time = 0.0;
        self.carry = 0.0;
        self.pending = self.emitter.bursts.clone();
        self.pending.sort_by(|a, b| b.time.total_cmp(&a.time));
        self.burst(self.emitter.burst as usize);
        let step = 1.0 / 60.0;
        let mut t = 0.0;
        while t < self.emitter.prewarm {
            self.update(step);
            t += step;
        }
    }

    /// Ends emission over time. Live particles die out on their own;
    /// [`finished`](Self::finished) says when they have.
    pub fn stop(&mut self) {
        self.running = false;
    }

    /// Kills every live particle.
    pub fn clear(&mut self) {
        self.live.clear();
    }

    /// Births `n` particles now, whether or not the system is emitting, up
    /// to the emitter's `max`.
    pub fn burst(&mut self, n: usize) {
        for _ in 0..n {
            if !self.emit() {
                break;
            }
        }
    }

    /// The number of live particles.
    pub fn alive(&self) -> usize {
        self.live.len()
    }

    /// Whether the system still births particles over time: it is running
    /// and has a rate or a burst left.
    pub fn emitting(&self) -> bool {
        self.running && (self.emitter.rate > 0.0 || !self.pending.is_empty())
    }

    /// Whether the system has stopped emitting and every particle has
    /// died, so a one-shot effect can be dropped.
    pub fn finished(&self) -> bool {
        !self.emitting() && self.live.is_empty()
    }

    /// The live particles, oldest first.
    pub fn particles(&self) -> &[Particle] {
        &self.live
    }

    /// Advances the simulation by `dt` seconds. Particles age, move and
    /// die; new ones are born at the emitter's rate, with fractions of a
    /// birth carried to the next step. A step of zero or less does
    /// nothing.
    pub fn update(&mut self, dt: f32) {
        if dt <= 0.0 {
            return;
        }
        self.step_particles(dt);
        if !self.running {
            return;
        }
        self.time += dt;
        while self.pending.last().is_some_and(|b| b.time <= self.time) {
            if let Some(b) = self.pending.pop() {
                self.burst(b.count as usize);
            }
        }
        let ending = self.emitter.duration.is_some_and(|d| self.time >= d);
        if !ending && self.emitter.rate > 0.0 {
            self.carry += f64::from(self.emitter.rate) * f64::from(dt);
            let n = self.carry as usize;
            self.carry -= n as f64;
            for _ in 0..n {
                if !self.emit() {
                    self.carry = 0.0;
                    break;
                }
            }
        }
        if ending {
            self.running = false;
        }
    }

    fn step_particles(&mut self, dt: f32) {
        let e = &self.emitter;
        self.live.retain_mut(|p| {
            p.age += dt;
            if p.age >= p.life {
                return false;
            }
            let mut accel = e.gravity;
            for a in &e.attractors {
                let d = a.position - p.position;
                let dist = d.length();
                if a.radius > 0.0 && dist < a.radius && dist > 1e-4 {
                    accel += d / dist * a.strength * (1.0 - dist / a.radius);
                }
            }
            if e.radial_accel != 0.0 || e.tangential_accel != 0.0 {
                let r = (p.position - p.origin).normalize_or_zero();
                accel += r * e.radial_accel + r.perp() * e.tangential_accel;
            }
            p.velocity += accel * dt;
            if e.drag > 0.0 {
                p.velocity *= (1.0 - e.drag * dt).max(0.0);
            }
            let boost = e.speed_over_life.at(p.t());
            p.position += p.velocity * boost * dt;
            p.rotation += p.spin * dt;
            true
        });
    }

    /// The width a particle takes when the emitter's size is 0.
    pub(crate) fn default_width(&self) -> f32 {
        let w = match (&self.sheet, &self.texture) {
            (Some(s), _) => s.frame_size().x as f32,
            (None, Some(t)) => t.width() as f32,
            _ => 0.0,
        };
        if w > 0.0 { w } else { 8.0 }
    }

    /// Height over width for a particle.
    pub(crate) fn aspect(&self) -> f32 {
        if self.emitter.aspect > 0.0 {
            return self.emitter.aspect;
        }
        match (&self.sheet, &self.texture) {
            (Some(s), _) => s.frame_size().y as f32 / (s.frame_size().x as f32).max(1.0),
            (None, Some(t)) => t.height() as f32 / (t.width() as f32).max(1.0),
            _ => 1.0,
        }
    }

    fn emit(&mut self) -> bool {
        if self.live.len() >= self.emitter.max {
            return false;
        }
        let origin = if self.emitter.world_space {
            self.position
        } else {
            Vec2::ZERO
        };
        let width = self.default_width();
        let p = birth(&self.emitter, &mut self.rng, origin, width);
        self.live.push(p);
        true
    }
}
