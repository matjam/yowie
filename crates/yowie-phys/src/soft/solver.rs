//! The XPBD step shared by cloth and volumetric soft bodies.

use yowie_core::Vec3;

use super::collide::{Colliders3, Ground, friction_slide3};
use super::constraints::{
    Link, Pressure, ShapeMatch, Tet, project_link, project_pressure, project_tet,
};
use crate::ids::CollisionLayers;

/// Tuning for the soft solver, shared by [`Cloth`](super::Cloth) and
/// [`SoftBody`](super::SoftBody).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SoftSettings {
    /// Acceleration applied to every particle, in units per second squared.
    pub gravity: Vec3,
    /// Substeps per `step` call; at least 1. More substeps give stiffer,
    /// more stable results at a proportional cost.
    pub substeps: u32,
    /// Constraint iterations per substep; at least 1.
    pub iterations: u32,
    /// An optional floor plane that every particle rests on.
    pub ground: Option<Ground>,
}

impl Default for SoftSettings {
    fn default() -> Self {
        Self {
            gravity: Vec3::new(0.0, -9.81, 0.0),
            substeps: 4,
            iterations: 4,
            ground: None,
        }
    }
}

/// Wind blowing through a particle mesh.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Wind {
    /// Air velocity in units per second.
    pub velocity: Vec3,
    /// Force per unit area per unit relative speed along the surface
    /// normal. Zero disables wind.
    pub drag: f32,
}

/// Particles, constraints and collision settings of one soft object.
#[derive(Debug, Clone)]
pub(crate) struct Sim3 {
    pub pos: Vec<Vec3>,
    pub prev: Vec<Vec3>,
    pub vel: Vec<Vec3>,
    pub inv_mass: Vec<f32>,
    pub links: Vec<Link>,
    pub tets: Vec<Tet>,
    pub pressure: Option<Pressure>,
    pub matching: Option<ShapeMatch>,
    pub tris: Vec<[u32; 3]>,
    pub wind: Wind,
    pub radius: f32,
    pub friction: f32,
    pub damping: f32,
    pub layers: CollisionLayers,
    lambda_links: Vec<f32>,
    lambda_tets: Vec<f32>,
    grad: Vec<Vec3>,
}

impl Sim3 {
    pub fn new(pos: Vec<Vec3>, inv_mass: Vec<f32>) -> Self {
        let n = pos.len();
        Self {
            prev: pos.clone(),
            vel: vec![Vec3::ZERO; n],
            pos,
            inv_mass,
            links: Vec::new(),
            tets: Vec::new(),
            pressure: None,
            matching: None,
            tris: Vec::new(),
            wind: Wind::default(),
            radius: 0.02,
            friction: 0.5,
            damping: 0.1,
            layers: CollisionLayers::ALL,
            lambda_links: Vec::new(),
            lambda_tets: Vec::new(),
            grad: vec![Vec3::ZERO; n],
        }
    }

    /// Advances the simulation by `dt` seconds.
    pub fn step(&mut self, dt: f32, s: &SoftSettings, world: &dyn Colliders3) {
        if !(dt.is_finite() && dt > 0.0) || self.pos.is_empty() {
            return;
        }
        let substeps = s.substeps.max(1);
        let h = dt / substeps as f32;
        self.lambda_links.resize(self.links.len(), 0.0);
        self.lambda_tets.resize(self.tets.len(), 0.0);
        for _ in 0..substeps {
            self.integrate(h, s);
            self.lambda_links.fill(0.0);
            self.lambda_tets.fill(0.0);
            let mut lambda_p = 0.0;
            for _ in 0..s.iterations.max(1) {
                self.project(h, &mut lambda_p);
            }
            if let Some(m) = &self.matching {
                m.apply(&mut self.pos, &self.inv_mass);
            }
            self.collide(s, world);
            let inv_h = 1.0 / h;
            for ((v, p), q) in self.vel.iter_mut().zip(&self.pos).zip(&self.prev) {
                *v = (*p - *q) * inv_h;
            }
        }
    }

    fn integrate(&mut self, h: f32, s: &SoftSettings) {
        if self.wind.drag > 0.0 {
            self.apply_wind(h);
        }
        let keep = (1.0 - self.damping * h).max(0.0);
        for i in 0..self.pos.len() {
            self.prev[i] = self.pos[i];
            if self.inv_mass[i] == 0.0 {
                continue;
            }
            self.vel[i] = (self.vel[i] + s.gravity * h) * keep;
            self.pos[i] += self.vel[i] * h;
        }
    }

    fn apply_wind(&mut self, h: f32) {
        for t in &self.tris {
            let [a, b, c] = t.map(|i| i as usize);
            let n = (self.pos[b] - self.pos[a]).cross(self.pos[c] - self.pos[a]);
            let len = n.length();
            if len < 1.0e-12 {
                continue;
            }
            let (area, nn) = (len * 0.5, n / len);
            let v = (self.vel[a] + self.vel[b] + self.vel[c]) / 3.0;
            let force = nn * (-self.wind.drag * area * nn.dot(v - self.wind.velocity));
            for i in [a, b, c] {
                self.vel[i] += force * (self.inv_mass[i] * h / 3.0);
            }
        }
    }

    fn project(&mut self, h: f32, lambda_p: &mut f32) {
        let h2 = h * h;
        for (l, lam) in self.links.iter().zip(self.lambda_links.iter_mut()) {
            project_link(&mut self.pos, &self.inv_mass, l, lam, l.compliance / h2);
        }
        for (t, lam) in self.tets.iter().zip(self.lambda_tets.iter_mut()) {
            project_tet(&mut self.pos, &self.inv_mass, t, lam, t.compliance / h2);
        }
        if let Some(p) = &self.pressure {
            project_pressure(
                &mut self.pos,
                &self.inv_mass,
                p,
                &mut self.grad,
                lambda_p,
                p.compliance / h2,
            );
        }
    }

    fn collide(&mut self, s: &SoftSettings, world: &dyn Colliders3) {
        let (radius, layers, friction) = (self.radius, self.layers, self.friction);
        let inv = &self.inv_mass;
        let prev = &self.prev;
        let work = |i: usize, p: &mut Vec3| {
            if inv[i] == 0.0 {
                return;
            }
            if let Some(g) = s.ground {
                let depth = g.y + radius - p.y;
                if depth > 0.0 {
                    p.y += depth;
                    friction_slide3(p, prev[i], Vec3::Y, depth, g.friction);
                }
            }
            if let Some(c) = world.contact(*p, radius, layers) {
                *p += c.normal * c.depth;
                friction_slide3(
                    p,
                    prev[i],
                    c.normal,
                    c.depth,
                    (c.friction * friction).sqrt(),
                );
            }
        };
        #[cfg(feature = "rayon")]
        {
            use rayon::prelude::*;
            if self.pos.len() >= 512 {
                self.pos
                    .par_iter_mut()
                    .enumerate()
                    .for_each(|(i, p)| work(i, p));
                return;
            }
        }
        for (i, p) in self.pos.iter_mut().enumerate() {
            work(i, p);
        }
    }
}
