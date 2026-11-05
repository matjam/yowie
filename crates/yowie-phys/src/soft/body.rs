//! Volumetric soft bodies built from tetrahedra.

use std::collections::HashMap;

use yowie_core::Vec3;

use super::collide::Colliders3;
use super::constraints::{Link, Pressure, ShapeMatch, Tet, enclosed_volume, tet_volume};
use super::mesh::SurfaceMesh;
use super::solver::{Sim3, SoftSettings, Wind};
use crate::error::{PhysError, Result};
use crate::ids::CollisionLayers;

/// Material and collision settings for a [`SoftBody`].
#[derive(Debug, Clone, PartialEq)]
pub struct SoftBodyDesc {
    /// Total mass shared between the particles by the volume around each.
    pub mass: f32,
    /// Softness of the edges; 0 is rigid.
    pub edge_compliance: f32,
    /// Softness of each tetrahedron's volume; 0 keeps every cell's volume.
    pub volume_compliance: f32,
    /// Softness of the whole-body volume constraint; `None` turns it off.
    /// Use a small value, such as `1e-4`, so the body does not deflate.
    pub pressure_compliance: Option<f32>,
    /// Target enclosed volume as a multiple of the rest volume. Above 1
    /// inflates the body.
    pub pressure_scale: f32,
    /// Shape-matching stiffness in `[0, 1]`; 0 turns it off. Values near 1
    /// make the body spring back to its rest shape like rubber.
    pub shape_matching: f32,
    /// Fraction of velocity lost per second.
    pub damping: f32,
    /// Collision radius of each particle.
    pub radius: f32,
    /// Friction coefficient against colliders.
    pub friction: f32,
    /// Wind acting on the surface.
    pub wind: Wind,
    /// Which colliders the body touches.
    pub layers: CollisionLayers,
}

impl Default for SoftBodyDesc {
    fn default() -> Self {
        Self {
            mass: 1.0,
            edge_compliance: 1.0e-5,
            volume_compliance: 0.0,
            pressure_compliance: Some(1.0e-4),
            pressure_scale: 1.0,
            shape_matching: 0.0,
            damping: 0.2,
            radius: 0.02,
            friction: 0.5,
            wind: Wind::default(),
            layers: CollisionLayers::ALL,
        }
    }
}

/// A closed volume of particles that squashes and springs back.
#[derive(Debug, Clone)]
pub struct SoftBody {
    sim: Sim3,
    surface: Vec<[u32; 3]>,
    rest_volume: f32,
    mass_inv: Vec<f32>,
}

impl SoftBody {
    /// Builds a box-shaped body centred on `centre`, filled with a lattice
    /// of `cells[i]` cells along each axis, each cell split into six
    /// tetrahedra.
    ///
    /// # Errors
    ///
    /// Returns [`PhysError::InvalidParameter`] for a zero cell count or a
    /// non-positive size, and anything [`from_tets`](Self::from_tets)
    /// rejects.
    pub fn lattice(
        centre: Vec3,
        size: Vec3,
        cells: [usize; 3],
        desc: &SoftBodyDesc,
    ) -> Result<Self> {
        if cells.contains(&0) || size.min_element() <= 0.0 {
            return Err(PhysError::InvalidParameter(
                "lattice needs cells and a positive size",
            ));
        }
        let [nx, ny, nz] = cells;
        let node = |x: usize, y: usize, z: usize| ((z * (ny + 1) + y) * (nx + 1) + x) as u32;
        let mut verts = Vec::with_capacity((nx + 1) * (ny + 1) * (nz + 1));
        for z in 0..=nz {
            for y in 0..=ny {
                for x in 0..=nx {
                    let t = Vec3::new(
                        x as f32 / nx as f32,
                        y as f32 / ny as f32,
                        z as f32 / nz as f32,
                    );
                    verts.push(centre + (t - Vec3::splat(0.5)) * size);
                }
            }
        }
        const PERMS: [[usize; 3]; 6] = [
            [0, 1, 2],
            [0, 2, 1],
            [1, 0, 2],
            [1, 2, 0],
            [2, 0, 1],
            [2, 1, 0],
        ];
        let mut tets = Vec::with_capacity(nx * ny * nz * 6);
        for z in 0..nz {
            for y in 0..ny {
                for x in 0..nx {
                    for perm in PERMS {
                        let mut c = [x, y, z];
                        let v0 = node(c[0], c[1], c[2]);
                        c[perm[0]] += 1;
                        let v1 = node(c[0], c[1], c[2]);
                        c[perm[1]] += 1;
                        let v2 = node(c[0], c[1], c[2]);
                        c[perm[2]] += 1;
                        tets.push([v0, v1, v2, node(c[0], c[1], c[2])]);
                    }
                }
            }
        }
        Self::from_tets(verts, &tets, desc)
    }

    /// Builds a body from a tetrahedral mesh. Tetrahedra of either winding
    /// are accepted.
    ///
    /// # Errors
    ///
    /// Returns [`PhysError::InvalidParameter`] for an empty mesh, an index
    /// out of range, a degenerate tetrahedron or a non-positive mass.
    pub fn from_tets(vertices: Vec<Vec3>, tets: &[[u32; 4]], desc: &SoftBodyDesc) -> Result<Self> {
        if vertices.is_empty() || tets.is_empty() || desc.mass <= 0.0 {
            return Err(PhysError::InvalidParameter(
                "soft body needs vertices, tets and mass",
            ));
        }
        let n = vertices.len();
        let mut cells = Vec::with_capacity(tets.len());
        let mut share = vec![0.0_f32; n];
        for t in tets {
            if t.iter().any(|&i| i as usize >= n) {
                return Err(PhysError::InvalidParameter(
                    "tetrahedron index out of range",
                ));
            }
            let mut p = *t;
            let mut v = tet_volume(
                vertices[p[0] as usize],
                vertices[p[1] as usize],
                vertices[p[2] as usize],
                vertices[p[3] as usize],
            );
            if v < 0.0 {
                p.swap(2, 3);
                v = -v;
            }
            if v < 1.0e-12 {
                return Err(PhysError::InvalidParameter("degenerate tetrahedron"));
            }
            for i in p {
                share[i as usize] += v / 4.0;
            }
            cells.push(Tet {
                p,
                rest: v,
                compliance: desc.volume_compliance,
            });
        }
        let total: f32 = share.iter().sum();
        let mass_inv: Vec<f32> = share
            .iter()
            .map(|s| {
                if *s > 0.0 {
                    total / (desc.mass * s)
                } else {
                    0.0
                }
            })
            .collect();

        let mut sim = Sim3::new(vertices, mass_inv.clone());
        let mut seen = HashMap::new();
        for t in &cells {
            for (i, a) in t.p.iter().enumerate() {
                for b in &t.p[i + 1..] {
                    let key = (*a.min(b), *a.max(b));
                    seen.entry(key).or_insert_with(|| {
                        let rest = (sim.pos[*a as usize] - sim.pos[*b as usize]).length();
                        sim.links.push(Link {
                            a: key.0,
                            b: key.1,
                            rest,
                            compliance: desc.edge_compliance,
                        });
                    });
                }
            }
        }
        let surface = surface_triangles(&sim.pos, &cells);
        sim.tets = cells;
        let rest_volume = enclosed_volume(&sim.pos, &surface);
        if let Some(c) = desc.pressure_compliance {
            sim.pressure = Some(Pressure {
                tris: surface.clone(),
                rest: rest_volume,
                scale: desc.pressure_scale,
                compliance: c,
            });
        }
        if desc.shape_matching > 0.0 {
            sim.matching = Some(ShapeMatch::new(
                &sim.pos,
                &sim.inv_mass,
                desc.shape_matching,
            ));
        }
        sim.tris = surface.clone();
        sim.wind = desc.wind;
        sim.radius = desc.radius;
        sim.friction = desc.friction;
        sim.damping = desc.damping;
        sim.layers = desc.layers;
        Ok(Self {
            sim,
            surface,
            rest_volume,
            mass_inv,
        })
    }

    /// World positions of the particles.
    pub fn positions(&self) -> &[Vec3] {
        &self.sim.pos
    }

    /// Velocities of the particles.
    pub fn velocities(&self) -> &[Vec3] {
        &self.sim.vel
    }

    /// Volume enclosed by the surface now.
    pub fn volume(&self) -> f32 {
        enclosed_volume(&self.sim.pos, &self.surface)
    }

    /// Volume enclosed by the surface at rest.
    pub fn rest_volume(&self) -> f32 {
        self.rest_volume
    }

    /// Outward-facing surface triangles as particle indices.
    pub fn surface(&self) -> &[[u32; 3]] {
        &self.surface
    }

    /// Pins particle `i` where it is. Out-of-range indices are ignored.
    pub fn pin(&mut self, i: usize) {
        if let Some(w) = self.sim.inv_mass.get_mut(i) {
            *w = 0.0;
        }
    }

    /// Releases particle `i`. Out-of-range indices are ignored.
    pub fn free(&mut self, i: usize) {
        if let (Some(w), Some(m)) = (self.sim.inv_mass.get_mut(i), self.mass_inv.get(i)) {
            *w = *m;
        }
    }

    /// Moves every particle by `delta` without disturbing the body.
    pub fn translate(&mut self, delta: Vec3) {
        for (p, q) in self.sim.pos.iter_mut().zip(&mut self.sim.prev) {
            *p += delta;
            *q += delta;
        }
    }

    /// Adds `dv` to the velocity of every free particle.
    pub fn add_velocity(&mut self, dv: Vec3) {
        for (v, w) in self.sim.vel.iter_mut().zip(&self.sim.inv_mass) {
            if *w > 0.0 {
                *v += dv;
            }
        }
    }

    /// Sets the wind.
    pub fn set_wind(&mut self, wind: Wind) {
        self.sim.wind = wind;
    }

    /// Advances the body by `dt` seconds, colliding with the static and
    /// kinematic colliders of `world`. Pass `&()` for no colliders.
    pub fn step(&mut self, dt: f32, settings: &SoftSettings, world: &dyn Colliders3) {
        self.sim.step(dt, settings, world);
    }

    /// Creates a mesh of the outer surface for rendering. Its vertices are
    /// all the particles, so interior ones go unused.
    pub fn mesh(&self) -> SurfaceMesh {
        let indices = self.surface.iter().flatten().copied().collect();
        let mut mesh = SurfaceMesh::new(indices, self.sim.pos.len());
        mesh.update(&self.sim.pos);
        mesh
    }

    /// Refreshes a mesh made by [`mesh`](Self::mesh).
    pub fn update_mesh(&self, mesh: &mut SurfaceMesh) {
        mesh.update(&self.sim.pos);
    }
}

/// Finds the faces that belong to one tetrahedron only and orients them
/// away from the tetrahedron's fourth vertex.
fn surface_triangles(pos: &[Vec3], tets: &[Tet]) -> Vec<[u32; 3]> {
    let mut faces: HashMap<[u32; 3], (u32, [u32; 3])> = HashMap::new();
    for t in tets {
        let p = t.p;
        for (face, opp) in [
            ([p[0], p[2], p[1]], p[3]),
            ([p[0], p[1], p[3]], p[2]),
            ([p[0], p[3], p[2]], p[1]),
            ([p[1], p[2], p[3]], p[0]),
        ] {
            let mut key = face;
            key.sort_unstable();
            let [a, b, c] = face.map(|i| pos[i as usize]);
            let outward = (b - a).cross(c - a).dot(pos[opp as usize] - a) < 0.0;
            let oriented = if outward {
                face
            } else {
                [face[0], face[2], face[1]]
            };
            let e = faces.entry(key).or_insert((0, oriented));
            e.0 += 1;
        }
    }
    let mut out: Vec<_> = faces
        .into_values()
        .filter(|(n, _)| *n == 1)
        .map(|(_, f)| f)
        .collect();
    out.sort_unstable();
    out
}
