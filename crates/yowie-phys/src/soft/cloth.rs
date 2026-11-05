//! Cloth: a grid of particles joined by stretch, shear and bend constraints.

use yowie_core::Vec3;

use super::collide::Colliders3;
use super::constraints::Link;
use super::mesh::SurfaceMesh;
use super::solver::{Sim3, SoftSettings, Wind};
use crate::error::{PhysError, Result};
use crate::ids::CollisionLayers;

/// Describes a rectangular cloth for [`Cloth::new`].
///
/// Particle `(col, row)` has index `row * cols + col` and starts at
/// `origin + u * (col * spacing) + v * (row * spacing)`. The front face
/// normal is `u x v`.
#[derive(Debug, Clone, PartialEq)]
pub struct ClothDesc {
    /// Particles along `u`; at least 2.
    pub cols: usize,
    /// Particles along `v`; at least 2.
    pub rows: usize,
    /// Distance between neighbouring particles at rest, in world units.
    pub spacing: f32,
    /// Total mass shared equally between the particles.
    pub mass: f32,
    /// World position of particle 0.
    pub origin: Vec3,
    /// Direction of increasing column; normalised on creation.
    pub u: Vec3,
    /// Direction of increasing row; normalised on creation. The default
    /// hangs the cloth downward from `origin`.
    pub v: Vec3,
    /// Indices of particles that never move on their own.
    pub pinned: Vec<usize>,
    /// Softness of the edges between neighbours; 0 is inextensible.
    pub stretch_compliance: f32,
    /// Softness of the diagonals, which resist shearing.
    pub shear_compliance: f32,
    /// Softness of the links that skip a particle, which resist folding.
    pub bend_compliance: f32,
    /// Fraction of velocity lost per second.
    pub damping: f32,
    /// Collision radius of each particle.
    pub radius: f32,
    /// Friction coefficient against colliders.
    pub friction: f32,
    /// Wind acting on the cloth.
    pub wind: Wind,
    /// Which colliders the cloth touches.
    pub layers: CollisionLayers,
}

impl Default for ClothDesc {
    fn default() -> Self {
        Self {
            cols: 10,
            rows: 10,
            spacing: 0.1,
            mass: 1.0,
            origin: Vec3::ZERO,
            u: Vec3::X,
            v: -Vec3::Y,
            pinned: Vec::new(),
            stretch_compliance: 0.0,
            shear_compliance: 1.0e-6,
            bend_compliance: 1.0e-3,
            damping: 0.05,
            radius: 0.02,
            friction: 0.5,
            wind: Wind::default(),
            layers: CollisionLayers::ALL,
        }
    }
}

/// A simulated cloth sheet.
#[derive(Debug, Clone)]
pub struct Cloth {
    sim: Sim3,
    cols: usize,
    rows: usize,
    mass_inv: f32,
    stretch_links: usize,
}

impl Cloth {
    /// Builds a cloth from `desc`.
    ///
    /// # Errors
    ///
    /// Returns [`PhysError::InvalidParameter`] for a grid smaller than 2x2,
    /// a non-positive spacing or mass, a zero direction vector or a pinned
    /// index out of range.
    pub fn new(desc: &ClothDesc) -> Result<Self> {
        if desc.cols < 2 || desc.rows < 2 {
            return Err(PhysError::InvalidParameter(
                "cloth needs at least 2x2 particles",
            ));
        }
        if !(desc.spacing > 0.0 && desc.mass > 0.0) {
            return Err(PhysError::InvalidParameter(
                "spacing and mass must be positive",
            ));
        }
        let (Some(u), Some(v)) = (desc.u.try_normalize(), desc.v.try_normalize()) else {
            return Err(PhysError::InvalidParameter(
                "cloth directions must be non-zero",
            ));
        };
        let n = desc.cols * desc.rows;
        if desc.pinned.iter().any(|&i| i >= n) {
            return Err(PhysError::InvalidParameter("pinned index out of range"));
        }
        let mass_inv = n as f32 / desc.mass;
        let mut pos = Vec::with_capacity(n);
        for r in 0..desc.rows {
            for c in 0..desc.cols {
                pos.push(
                    desc.origin + u * (c as f32 * desc.spacing) + v * (r as f32 * desc.spacing),
                );
            }
        }
        let mut inv = vec![mass_inv; n];
        for &i in &desc.pinned {
            inv[i] = 0.0;
        }
        let mut sim = Sim3::new(pos, inv);
        let at = |c: usize, r: usize| (r * desc.cols + c) as u32;
        for r in 0..desc.rows {
            for c in 0..desc.cols {
                if c + 1 < desc.cols {
                    add_link(&mut sim, at(c, r), at(c + 1, r), desc.stretch_compliance);
                }
                if r + 1 < desc.rows {
                    add_link(&mut sim, at(c, r), at(c, r + 1), desc.stretch_compliance);
                }
            }
        }
        let stretch_links = sim.links.len();
        for r in 0..desc.rows {
            for c in 0..desc.cols {
                if c + 1 < desc.cols && r + 1 < desc.rows {
                    add_link(&mut sim, at(c, r), at(c + 1, r + 1), desc.shear_compliance);
                    add_link(&mut sim, at(c + 1, r), at(c, r + 1), desc.shear_compliance);
                }
                if c + 2 < desc.cols {
                    add_link(&mut sim, at(c, r), at(c + 2, r), desc.bend_compliance);
                }
                if r + 2 < desc.rows {
                    add_link(&mut sim, at(c, r), at(c, r + 2), desc.bend_compliance);
                }
            }
        }
        for r in 0..desc.rows - 1 {
            for c in 0..desc.cols - 1 {
                let (a, b, d, e) = (at(c, r), at(c + 1, r), at(c, r + 1), at(c + 1, r + 1));
                sim.tris.push([a, b, d]);
                sim.tris.push([b, e, d]);
            }
        }
        sim.wind = desc.wind;
        sim.radius = desc.radius;
        sim.friction = desc.friction;
        sim.damping = desc.damping;
        sim.layers = desc.layers;
        Ok(Self {
            sim,
            cols: desc.cols,
            rows: desc.rows,
            mass_inv,
            stretch_links,
        })
    }

    /// Particles across and down.
    pub fn size(&self) -> (usize, usize) {
        (self.cols, self.rows)
    }

    /// World positions of the particles, row by row.
    pub fn positions(&self) -> &[Vec3] {
        &self.sim.pos
    }

    /// Velocities of the particles, in the same order as positions.
    pub fn velocities(&self) -> &[Vec3] {
        &self.sim.vel
    }

    /// Whether particle `i` is pinned.
    pub fn is_pinned(&self, i: usize) -> bool {
        self.sim.inv_mass.get(i).is_some_and(|w| *w == 0.0)
    }

    /// Pins particle `i` where it is. Out-of-range indices are ignored.
    pub fn pin(&mut self, i: usize) {
        if let Some(w) = self.sim.inv_mass.get_mut(i) {
            *w = 0.0;
        }
    }

    /// Releases particle `i`. Out-of-range indices are ignored.
    pub fn free(&mut self, i: usize) {
        if let Some(w) = self.sim.inv_mass.get_mut(i) {
            *w = self.mass_inv;
        }
    }

    /// Moves a pinned particle to `p`, for example to wave a flag. The rest
    /// of the cloth follows on the next step.
    pub fn move_pinned(&mut self, i: usize, p: Vec3) {
        if self.is_pinned(i) {
            self.sim.pos[i] = p;
            self.sim.prev[i] = p;
        }
    }

    /// Moves every particle by `delta` without disturbing the cloth.
    pub fn translate(&mut self, delta: Vec3) {
        for (p, q) in self.sim.pos.iter_mut().zip(&mut self.sim.prev) {
            *p += delta;
            *q += delta;
        }
    }

    /// Sets the wind.
    pub fn set_wind(&mut self, wind: Wind) {
        self.sim.wind = wind;
    }

    /// The largest relative change in length among the edges between
    /// neighbouring particles: 0 at rest, 0.1 for an edge 10% too long.
    pub fn max_strain(&self) -> f32 {
        self.sim.links[..self.stretch_links]
            .iter()
            .map(|l| {
                let d = self.sim.pos[l.a as usize] - self.sim.pos[l.b as usize];
                ((d.length() - l.rest) / l.rest).abs()
            })
            .fold(0.0, f32::max)
    }

    /// Advances the cloth by `dt` seconds, colliding with the static and
    /// kinematic colliders of `world`. Pass `&()` for no colliders.
    pub fn step(&mut self, dt: f32, settings: &SoftSettings, world: &dyn Colliders3) {
        self.sim.step(dt, settings, world);
    }

    /// Creates a mesh for rendering, with one vertex per particle.
    pub fn mesh(&self) -> SurfaceMesh {
        let indices = self.sim.tris.iter().flatten().copied().collect();
        let mut mesh = SurfaceMesh::new(indices, self.sim.pos.len());
        mesh.update(&self.sim.pos);
        mesh
    }

    /// Refreshes a mesh made by [`mesh`](Self::mesh) with the current
    /// positions and normals.
    pub fn update_mesh(&self, mesh: &mut SurfaceMesh) {
        mesh.update(&self.sim.pos);
    }
}

fn add_link(sim: &mut Sim3, a: u32, b: u32, compliance: f32) {
    let rest = (sim.pos[a as usize] - sim.pos[b as usize]).length();
    sim.links.push(Link {
        a,
        b,
        rest,
        compliance,
    });
}
