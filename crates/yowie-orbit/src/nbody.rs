use serde::{Deserialize, Serialize};
use yowie_core::glam::DVec3;

use crate::G;

/// A point mass.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Body {
    /// Position.
    pub pos: DVec3,
    /// Velocity.
    pub vel: DVec3,
    /// Mass. A body with zero mass feels gravity but exerts none.
    pub mass: f64,
}

/// Returns the gravitational acceleration at `p` from `bodies`.
///
/// `softening` is a distance added in quadrature to every separation, which
/// keeps close encounters finite: the pull of a body at distance `d` is
/// `g m d / (d² + softening²)^(3/2)`.
pub fn field_at(bodies: &[Body], g: f64, softening: f64, p: DVec3) -> DVec3 {
    let eps2 = softening * softening;
    let mut a = DVec3::ZERO;
    for b in bodies {
        if b.mass == 0.0 {
            continue;
        }
        let d = b.pos - p;
        let r2 = d.length_squared() + eps2;
        if r2 == 0.0 {
            continue;
        }
        a += d * (g * b.mass / (r2 * r2.sqrt()));
    }
    a
}

/// Advances `(pos, vel)` by `dt` with the classic fourth-order Runge-Kutta
/// method. `accel(pos, vel)` returns the acceleration there. Use it for
/// ships and probes; for many interacting bodies use [`Simulation`].
pub fn rk4(
    pos: DVec3,
    vel: DVec3,
    dt: f64,
    mut accel: impl FnMut(DVec3, DVec3) -> DVec3,
) -> (DVec3, DVec3) {
    let k1v = accel(pos, vel);
    let k1p = vel;
    let k2p = vel + k1v * (dt / 2.0);
    let k2v = accel(pos + k1p * (dt / 2.0), k2p);
    let k3p = vel + k2v * (dt / 2.0);
    let k3v = accel(pos + k2p * (dt / 2.0), k3p);
    let k4p = vel + k3v * dt;
    let k4v = accel(pos + k3p * dt, k4p);
    (
        pos + (k1p + k2p * 2.0 + k3p * 2.0 + k4p) * (dt / 6.0),
        vel + (k1v + k2v * 2.0 + k3v * 2.0 + k4v) * (dt / 6.0),
    )
}

#[derive(Clone, Debug)]
struct AccCache {
    acc: Vec<DVec3>,
    from: Vec<(DVec3, f64)>,
    g: f64,
    softening: f64,
}

/// An N-body simulation integrated with the leapfrog (kick-drift-kick)
/// method.
///
/// Leapfrog is second order and symplectic: energy error stays bounded over
/// very long runs instead of drifting, and it conserves linear and angular
/// momentum to rounding error. Choose steps small enough to resolve the
/// fastest orbit, about 1/200 of its period or less.
#[derive(Clone, Debug)]
pub struct Simulation {
    /// The bodies. Change them freely between steps.
    pub bodies: Vec<Body>,
    /// The gravitational constant. Defaults to [`G`].
    pub g: f64,
    /// Softening distance; see [`field_at`]. Zero disables softening.
    pub softening: f64,
    /// Time elapsed, advanced by [`Simulation::step`].
    pub time: f64,
    cache: Option<AccCache>,
}

impl Default for Simulation {
    fn default() -> Self {
        Self::new(Vec::new())
    }
}

impl Simulation {
    /// Creates a simulation of `bodies` with the real gravitational
    /// constant and no softening.
    pub fn new(bodies: Vec<Body>) -> Self {
        Self {
            bodies,
            g: G,
            softening: 0.0,
            time: 0.0,
            cache: None,
        }
    }

    /// Computes every body's acceleration into `out`, which is resized to
    /// match.
    pub fn accelerations(&self, out: &mut Vec<DVec3>) {
        out.clear();
        out.extend(self.bodies.iter().enumerate().map(|(i, b)| {
            let eps2 = self.softening * self.softening;
            let mut a = DVec3::ZERO;
            for (j, o) in self.bodies.iter().enumerate() {
                if i == j || o.mass == 0.0 {
                    continue;
                }
                let d = o.pos - b.pos;
                let r2 = d.length_squared() + eps2;
                if r2 > 0.0 {
                    a += d * (self.g * o.mass / (r2 * r2.sqrt()));
                }
            }
            a
        }));
    }

    /// The acceleration a test mass would feel at `p`.
    pub fn field_at(&self, p: DVec3) -> DVec3 {
        field_at(&self.bodies, self.g, self.softening, p)
    }

    fn cache_is_current(&self, c: &AccCache) -> bool {
        c.from.len() == self.bodies.len()
            && c.g.to_bits() == self.g.to_bits()
            && c.softening.to_bits() == self.softening.to_bits()
            && self.bodies.iter().zip(&c.from).all(|(b, &(pos, mass))| {
                b.mass.to_bits() == mass.to_bits()
                    && b.pos.x.to_bits() == pos.x.to_bits()
                    && b.pos.y.to_bits() == pos.y.to_bits()
                    && b.pos.z.to_bits() == pos.z.to_bits()
            })
    }

    /// Advances the simulation by `dt`. The acceleration computed at the end
    /// of one step is reused at the start of the next, so a run costs one
    /// force evaluation per step unless the bodies were changed between
    /// steps.
    pub fn step(&mut self, dt: f64) {
        let mut cache = self.cache.take().unwrap_or(AccCache {
            acc: Vec::new(),
            from: Vec::new(),
            g: 0.0,
            softening: 0.0,
        });
        if !self.cache_is_current(&cache) {
            self.accelerations(&mut cache.acc);
        }
        for (b, a) in self.bodies.iter_mut().zip(&cache.acc) {
            b.vel += *a * (dt / 2.0);
            b.pos += b.vel * dt;
        }
        self.accelerations(&mut cache.acc);
        for (b, a) in self.bodies.iter_mut().zip(&cache.acc) {
            b.vel += *a * (dt / 2.0);
        }
        self.time += dt;
        cache.from.clear();
        cache
            .from
            .extend(self.bodies.iter().map(|b| (b.pos, b.mass)));
        cache.g = self.g;
        cache.softening = self.softening;
        self.cache = Some(cache);
    }

    /// Total energy: kinetic plus gravitational potential. Conserved when
    /// softening is zero.
    pub fn energy(&self) -> f64 {
        let mut e = 0.0;
        for (i, b) in self.bodies.iter().enumerate() {
            e += 0.5 * b.mass * b.vel.length_squared();
            for o in &self.bodies[i + 1..] {
                let d = (o.pos - b.pos).length();
                if d > 0.0 {
                    e -= self.g * b.mass * o.mass / d;
                }
            }
        }
        e
    }

    /// Total angular momentum about the origin.
    pub fn angular_momentum(&self) -> DVec3 {
        self.bodies
            .iter()
            .map(|b| b.pos.cross(b.vel) * b.mass)
            .sum()
    }

    /// Total linear momentum.
    pub fn momentum(&self) -> DVec3 {
        self.bodies.iter().map(|b| b.vel * b.mass).sum()
    }

    /// The centre of mass, or the origin when all masses are zero.
    pub fn barycenter(&self) -> DVec3 {
        let mass: f64 = self.bodies.iter().map(|b| b.mass).sum();
        if mass == 0.0 {
            return DVec3::ZERO;
        }
        self.bodies.iter().map(|b| b.pos * b.mass).sum::<DVec3>() / mass
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Elements, sol};
    use std::f64::consts::TAU;

    /// The Sun and Earth on a slightly eccentric orbit, in the barycentric
    /// frame, plus the period of the relative orbit.
    fn sun_earth() -> (Simulation, f64) {
        let (ms, me) = (sol::SUN.mass, sol::EARTH.mass);
        let mu = G * (ms + me);
        let el = Elements {
            semi_major_axis: sol::AU,
            eccentricity: 0.0167,
            true_anomaly: 1.0,
            ..Elements::default()
        };
        let rel = el.state(mu);
        let total = ms + me;
        let sim = Simulation::new(vec![
            Body {
                pos: -rel.pos * (me / total),
                vel: -rel.vel * (me / total),
                mass: ms,
            },
            Body {
                pos: rel.pos * (ms / total),
                vel: rel.vel * (ms / total),
                mass: me,
            },
        ]);
        (sim, el.period(mu).unwrap_or(0.0))
    }

    #[test]
    fn two_body_conserves_energy_and_angular_momentum() {
        let (mut sim, period) = sun_earth();
        let (e0, l0, p0) = (sim.energy(), sim.angular_momentum(), sim.momentum());
        let dt = period / 2000.0;
        let mut worst = 0.0f64;
        for _ in 0..(2000 * 20) {
            sim.step(dt);
            worst = worst.max(((sim.energy() - e0) / e0).abs());
        }
        assert!(worst < 1e-6, "energy drift {worst}");
        assert!((sim.angular_momentum() - l0).length() < 1e-11 * l0.length());
        assert!((sim.momentum() - p0).length() < 1e-9 * p0.length().max(sol::EARTH.mass * 3.0e4));
        assert!((sim.time - 20.0 * period).abs() < 1e-3 * dt);
    }

    #[test]
    fn earth_returns_after_one_period() {
        let (mut sim, period) = sun_earth();
        let start = sim.bodies[1].pos - sim.bodies[0].pos;
        let steps = 4000;
        for _ in 0..steps {
            sim.step(period / f64::from(steps));
        }
        let end = sim.bodies[1].pos - sim.bodies[0].pos;
        assert!((end - start).length() < 1e-5 * start.length());
    }

    #[test]
    fn changing_bodies_invalidates_the_cached_acceleration() {
        let (mut a, _) = sun_earth();
        let (mut b, _) = sun_earth();
        a.step(1000.0);
        b.step(1000.0);
        a.bodies[1].pos.x += 1.0e6;
        b.bodies[1].pos.x += 1.0e6;
        b.cache = None;
        a.step(1000.0);
        b.step(1000.0);
        assert_eq!(a.bodies, b.bodies);
    }

    #[test]
    fn softening_limits_close_encounters() {
        let mut sim = Simulation::new(vec![
            Body {
                pos: DVec3::ZERO,
                vel: DVec3::ZERO,
                mass: 1.0,
            },
            Body {
                pos: DVec3::new(1.0e-9, 0.0, 0.0),
                vel: DVec3::ZERO,
                mass: 1.0,
            },
        ]);
        sim.g = 1.0;
        sim.softening = 0.1;
        let mut acc = Vec::new();
        sim.accelerations(&mut acc);
        assert!(acc[0].length() < 1e-6);
        sim.softening = 0.0;
        sim.accelerations(&mut acc);
        assert!(acc[0].length() > 1e17);
    }

    #[test]
    fn field_and_barycenter() {
        let sim = Simulation {
            g: 1.0,
            ..Simulation::new(vec![
                Body {
                    pos: DVec3::new(-1.0, 0.0, 0.0),
                    vel: DVec3::ZERO,
                    mass: 1.0,
                },
                Body {
                    pos: DVec3::new(3.0, 0.0, 0.0),
                    vel: DVec3::ZERO,
                    mass: 3.0,
                },
                Body {
                    pos: DVec3::new(50.0, 0.0, 0.0),
                    vel: DVec3::ZERO,
                    mass: 0.0,
                },
            ])
        };
        assert_eq!(sim.barycenter(), DVec3::new(2.0, 0.0, 0.0));
        // At the origin: pulled left by m=1 at distance 1, right by m=3 at 3.
        let f = sim.field_at(DVec3::ZERO);
        assert!((f.x - (-1.0 + 3.0 / 9.0)).abs() < 1e-12);
        assert_eq!(Simulation::default().barycenter(), DVec3::ZERO);
    }

    #[test]
    fn rk4_integrates_a_circular_orbit() {
        let mu = 1.0;
        let accel = |p: DVec3, _v: DVec3| -p * (mu / p.length().powi(3));
        let (mut p, mut v) = (DVec3::X, DVec3::Y);
        let n = 2000;
        for _ in 0..n {
            (p, v) = rk4(p, v, TAU / f64::from(n), accel);
        }
        assert!((p - DVec3::X).length() < 1e-9);
        assert!((v - DVec3::Y).length() < 1e-9);
    }
}
