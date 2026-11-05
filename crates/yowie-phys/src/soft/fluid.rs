//! 2D position-based fluids.

use yowie_core::{Rect, Vec2};

use super::collide::{Colliders2, friction_slide2};
use super::fluid_grid::{Neighbours, poly6, rest_density, spiky_grad};
use super::parallel::fill_with;
use crate::error::{PhysError, Result};
use crate::ids::CollisionLayers;

/// Describes a body of liquid for [`Fluid2D::new`]. Lengths are world
/// units; Y points up.
#[derive(Debug, Clone, PartialEq)]
pub struct FluidDesc2D {
    /// The tank that keeps particles in. `None` leaves the fluid held only
    /// by the colliders it meets.
    pub bounds: Option<Rect>,
    /// Distance between particles at rest. It sets the density the fluid
    /// settles at, so changing it changes the fluid.
    pub spacing: f32,
    /// How far a particle feels its neighbours; 0 means 2.5 times the
    /// spacing, about twenty neighbours.
    pub radius: f32,
    /// Solves per `step`; at least 1. Position-based fluids are meant to
    /// run at 1; raise it for fast, thin fluids that must not cross walls.
    pub substeps: u32,
    /// Density solver iterations per solve; at least 1.
    pub iterations: u32,
    /// Rate per second at which a particle's velocity moves toward its
    /// neighbours': 0 churns freely, 15 is water, 60 is syrup.
    pub viscosity: f32,
    /// Strength of the close-range push that rounds off drops. The push
    /// scales with the square of the particle radius.
    pub surface_tension: f32,
    /// Softens the density solve so sparse particles are not thrown about.
    pub relaxation: f32,
    /// Fraction of velocity lost per second.
    pub damping: f32,
    /// Coulomb friction against the tank and colliders.
    pub friction: f32,
    /// Which colliders the fluid touches.
    pub layers: CollisionLayers,
}

impl Default for FluidDesc2D {
    fn default() -> Self {
        Self {
            bounds: None,
            spacing: 0.05,
            radius: 0.0,
            substeps: 1,
            iterations: 4,
            viscosity: 15.0,
            surface_tension: 0.02,
            relaxation: 0.05,
            damping: 0.1,
            friction: 0.1,
            layers: CollisionLayers::ALL,
        }
    }
}

/// A body of liquid in the plane, simulated as particles.
///
/// Fill it with [`fill`](Self::fill) or [`add`](Self::add), call
/// [`step`](Self::step) every frame, and draw [`positions`](Self::positions).
#[derive(Debug, Clone)]
pub struct Fluid2D {
    desc: FluidDesc2D,
    radius: f32,
    rest: f32,
    pos: Vec<Vec2>,
    prev: Vec<Vec2>,
    vel: Vec<Vec2>,
    next: Vec<Vec2>,
    delta: Vec<Vec2>,
    dens: Vec<f32>,
    lambda: Vec<f32>,
    nbr: Neighbours,
}

impl Fluid2D {
    /// Creates an empty fluid.
    ///
    /// # Errors
    ///
    /// Returns [`PhysError::InvalidParameter`] for a non-positive spacing.
    pub fn new(desc: &FluidDesc2D) -> Result<Self> {
        if !(desc.spacing.is_finite() && desc.spacing > 0.0) {
            return Err(PhysError::InvalidParameter(
                "fluid spacing must be positive",
            ));
        }
        let radius = if desc.radius > 0.0 {
            desc.radius
        } else {
            2.5 * desc.spacing
        };
        Ok(Self {
            desc: desc.clone(),
            radius,
            rest: rest_density(radius, desc.spacing),
            pos: Vec::new(),
            prev: Vec::new(),
            vel: Vec::new(),
            next: Vec::new(),
            delta: Vec::new(),
            dens: Vec::new(),
            lambda: Vec::new(),
            nbr: Neighbours::default(),
        })
    }

    /// Adds one particle at rest.
    pub fn add(&mut self, p: Vec2) {
        self.pos.push(p);
        self.prev.push(p);
        self.vel.push(Vec2::ZERO);
        self.next.push(p);
        self.delta.push(Vec2::ZERO);
        self.dens.push(0.0);
        self.lambda.push(0.0);
    }

    /// Fills `area` with particles on a square lattice at the fluid's
    /// spacing.
    pub fn fill(&mut self, area: Rect) {
        let s = self.desc.spacing;
        let (min, max) = (area.min(), area.max());
        let mut y = min.y + s / 2.0;
        while y < max.y {
            let mut x = min.x + s / 2.0;
            while x < max.x {
                self.add(Vec2::new(x, y));
                x += s;
            }
            y += s;
        }
    }

    /// Removes every particle.
    pub fn clear(&mut self) {
        *self = Self {
            desc: self.desc.clone(),
            radius: self.radius,
            rest: self.rest,
            pos: Vec::new(),
            prev: Vec::new(),
            vel: Vec::new(),
            next: Vec::new(),
            delta: Vec::new(),
            dens: Vec::new(),
            lambda: Vec::new(),
            nbr: Neighbours::default(),
        };
    }

    /// Number of particles.
    pub fn count(&self) -> usize {
        self.pos.len()
    }

    /// Particle positions in world units.
    pub fn positions(&self) -> &[Vec2] {
        &self.pos
    }

    /// Particle velocities, in the same order as positions.
    pub fn velocities(&self) -> &[Vec2] {
        &self.vel
    }

    /// Density around particle `i` as measured by the last step, or 0 for
    /// an invalid index. Compare with [`rest_density`](Self::rest_density).
    pub fn density(&self, i: usize) -> f32 {
        self.dens.get(i).copied().unwrap_or(0.0)
    }

    /// The density the solver holds the fluid at.
    pub fn rest_density(&self) -> f32 {
        self.rest
    }

    /// Distance between particles at rest.
    pub fn spacing(&self) -> f32 {
        self.desc.spacing
    }

    /// Advances the fluid by `dt` seconds under `gravity`, colliding with
    /// the static and kinematic colliders of `world`. Pass `&()` for none.
    pub fn step(&mut self, dt: f32, gravity: Vec2, world: &dyn Colliders2) {
        if !(dt.is_finite() && dt > 0.0) || self.pos.is_empty() {
            return;
        }
        let substeps = self.desc.substeps.max(1);
        let h = dt / substeps as f32;
        for _ in 0..substeps {
            self.substep(h, gravity, world);
        }
    }

    fn substep(&mut self, h: f32, gravity: Vec2, world: &dyn Colliders2) {
        let keep = (1.0 - self.desc.damping * h).max(0.0);
        for i in 0..self.pos.len() {
            self.vel[i] = (self.vel[i] + gravity * h) * keep;
            self.prev[i] = self.pos[i];
            self.pos[i] += self.vel[i] * h;
        }
        self.nbr.build(&self.pos, self.radius);
        for _ in 0..self.desc.iterations.max(1) {
            self.densities();
            self.lambdas();
            self.deltas();
            self.apply(world);
        }
        let inv_h = 1.0 / h;
        for i in 0..self.pos.len() {
            self.vel[i] = (self.pos[i] - self.prev[i]) * inv_h;
        }
        self.viscosity(h);
    }

    fn densities(&mut self) {
        let (pos, nbr, r) = (&self.pos, &self.nbr, self.radius);
        fill_with(&mut self.dens, |i| {
            poly6(0.0, r)
                + nbr
                    .of(i)
                    .iter()
                    .map(|&j| poly6((pos[i] - pos[j as usize]).length(), r))
                    .sum::<f32>()
        });
    }

    fn lambdas(&mut self) {
        let (pos, nbr, dens) = (&self.pos, &self.nbr, &self.dens);
        let (r, rest) = (self.radius, self.rest);
        let eps = self.desc.relaxation / (r * r);
        fill_with(&mut self.lambda, |i| {
            let c = (dens[i] / rest - 1.0).max(0.0);
            let (mut sum_sq, mut total) = (0.0, Vec2::ZERO);
            for &j in nbr.of(i) {
                let d = pos[i] - pos[j as usize];
                let len = d.length();
                let g = spiky_grad(len, r);
                if g != 0.0 {
                    let grad = d / len * (g / rest);
                    total += grad;
                    sum_sq += grad.length_squared();
                }
            }
            -c / (sum_sq + total.length_squared() + eps)
        });
    }

    fn deltas(&mut self) {
        let (pos, nbr, lambda) = (&self.pos, &self.nbr, &self.lambda);
        let (r, rest, tension) = (self.radius, self.rest, self.desc.surface_tension);
        let wq = poly6(0.2 * r, r);
        fill_with(&mut self.delta, |i| {
            let mut d = Vec2::ZERO;
            for &j in nbr.of(i) {
                let diff = pos[i] - pos[j as usize];
                let len = diff.length();
                let g = spiky_grad(len, r);
                if g == 0.0 {
                    continue;
                }
                let corr = if wq > 0.0 && tension > 0.0 {
                    let q = poly6(len, r) / wq;
                    -tension * r * r * q * q * q * q
                } else {
                    0.0
                };
                d += diff * ((lambda[i] + lambda[j as usize] + corr) * g / (len * rest));
            }
            d
        });
    }

    fn apply(&mut self, world: &dyn Colliders2) {
        let (pos, prev, delta) = (&self.pos, &self.prev, &self.delta);
        let (bounds, mu, layers) = (self.desc.bounds, self.desc.friction, self.desc.layers);
        let radius = self.desc.spacing / 2.0;
        fill_with(&mut self.next, |i| {
            let mut p = pos[i] + delta[i];
            if let Some(b) = bounds {
                let (lo, hi) = (b.min(), b.max());
                for (axis, n) in [(0, Vec2::X), (1, Vec2::Y)] {
                    if p[axis] < lo[axis] + radius {
                        p[axis] = lo[axis] + radius;
                        friction_slide2(&mut p, prev[i], n, radius, mu);
                    } else if p[axis] > hi[axis] - radius {
                        p[axis] = hi[axis] - radius;
                        friction_slide2(&mut p, prev[i], -n, radius, mu);
                    }
                }
            }
            if let Some(c) = world.contact(p, radius, layers) {
                p += c.normal * c.depth;
                friction_slide2(&mut p, prev[i], c.normal, c.depth, (c.friction * mu).sqrt());
            }
            p
        });
        std::mem::swap(&mut self.pos, &mut self.next);
    }

    /// Pulls each particle's velocity toward its neighbours', which makes
    /// a thick liquid move as one.
    fn viscosity(&mut self, h: f32) {
        if self.desc.viscosity <= 0.0 {
            return;
        }
        let rate = (self.desc.viscosity * h).min(1.0);
        let (pos, vel, nbr) = (&self.pos, &self.vel, &self.nbr);
        let (r, rest) = (self.radius, self.rest);
        fill_with(&mut self.delta, |i| {
            let mut sum = Vec2::ZERO;
            for &j in nbr.of(i) {
                let w = poly6((pos[i] - pos[j as usize]).length(), r);
                sum += (vel[j as usize] - vel[i]) * w;
            }
            sum * (rate / rest)
        });
        for (v, d) in self.vel.iter_mut().zip(&self.delta) {
            *v += *d;
        }
    }
}
