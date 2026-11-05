//! Drawing the lab: solid meshes for every body, then the physics debug
//! lines over them.

use std::collections::HashMap;

use yowie::gfx::scene::{DirectionalLight, Material, Mesh, MeshData, Scene, Sky};
use yowie::gfx::{Graphics, Result};
use yowie::phys::DebugDraw;
use yowie::{Color, Mat4, SceneDebugDraw, Vec3};

use crate::lab::{self, HERO_HALF, HERO_RADIUS, Kind, Lab};
use crate::meshes::{self, Terrain};

/// The GPU meshes the lab draws with.
pub struct Meshes {
    terrain: Mesh,
    cube: Mesh,
    sphere: Mesh,
    capsule: Mesh,
    octahedron: Mesh,
    wedge: Mesh,
    hero: Mesh,
    /// One capsule per ragdoll part, sized from the part's bone.
    parts: HashMap<&'static str, Mesh>,
}

impl Meshes {
    pub fn new(gfx: &Graphics, terrain: &Terrain, lab: &Lab) -> Result<Self> {
        let mut parts = HashMap::new();
        if let Some(doll) = &lab.ragdoll {
            for (name, bone) in &doll.bones {
                let half = (bone.length / 2.0 - bone.radius).max(0.0);
                let data = MeshData::capsule(bone.radius, half, 6, 16);
                parts.insert(*name, Mesh::new(gfx, &data)?);
            }
        }
        Ok(Meshes {
            terrain: Mesh::new(gfx, &terrain.mesh)?,
            cube: Mesh::new(gfx, &MeshData::cube(1.0))?,
            sphere: Mesh::new(gfx, &MeshData::sphere(1.0, 12, 18))?,
            capsule: Mesh::new(gfx, &MeshData::capsule(0.3, 0.45, 6, 16))?,
            octahedron: Mesh::new(gfx, &meshes::octahedron_mesh(0.7))?,
            wedge: Mesh::new(gfx, &meshes::wedge_mesh(0.6, 0.35, 0.5))?,
            hero: Mesh::new(gfx, &MeshData::capsule(HERO_RADIUS, HERO_HALF, 6, 16))?,
            parts,
        })
    }

    /// Records the whole frame into `s`.
    pub fn draw(&self, s: &mut Scene<'_>, lab: &mut Lab, alpha: f32, debug: bool) {
        s.set_sun(
            DirectionalLight::new(Vec3::new(-0.5, -1.0, -0.3), Color::rgb(1.0, 0.96, 0.9), 3.2)
                .with_shadows(60.0),
        );
        s.set_sky(Sky {
            zenith: Color::rgb8(80, 110, 170),
            horizon: Color::rgb8(170, 185, 205),
            ground: Color::rgb8(70, 62, 55),
            ..Sky::default()
        });
        s.draw(
            &self.terrain,
            &Material::color(Color::rgb8(110, 140, 90), 0.0, 0.9),
            Mat4::IDENTITY,
        );
        self.draw_stairs(s);
        self.draw_debris(s, lab, alpha);
        self.draw_chain(s, lab, alpha);
        self.draw_ragdoll(s, lab, alpha);
        if let Some(t) = lab.world.interpolated_transform(lab.hero.body, alpha) {
            let color = if lab.hero.grounded {
                Color::rgb8(80, 220, 120)
            } else {
                Color::rgb8(230, 90, 90)
            };
            s.draw(&self.hero, &Material::color(color, 0.0, 0.5), t);
        }
        if debug {
            lab.world
                .debug_draw(&mut NoTerrainEdges(SceneDebugDraw::new(s)));
        }
    }

    fn draw_stairs(&self, s: &mut Scene<'_>) {
        let stone = Material::color(Color::rgb8(150, 150, 160), 0.0, 0.9);
        let boxes = std::iter::once(lab::SLAB).chain((0..lab::STEP_COUNT).map(lab::step_box));
        for (centre, half) in boxes {
            let m =
                Mat4::from_scale_rotation_translation(half * 2.0, yowie::Quat::IDENTITY, centre);
            s.draw(&self.cube, &stone, m);
        }
    }

    fn draw_debris(&self, s: &mut Scene<'_>, lab: &Lab, alpha: f32) {
        for d in &lab.debris {
            let Some(t) = lab.world.interpolated_transform(d.body, alpha) else {
                continue;
            };
            let mesh = match d.kind {
                Kind::Capsule => &self.capsule,
                Kind::Octahedron => &self.octahedron,
                Kind::Wedge => &self.wedge,
                Kind::Box => &self.cube,
                Kind::Sphere => &self.sphere,
            };
            // The unit cube and sphere meshes are scaled to the collider.
            let scale = match d.kind {
                Kind::Box => Vec3::splat(0.8),
                Kind::Sphere => Vec3::splat(0.4),
                _ => Vec3::ONE,
            };
            let m = t.to_matrix() * Mat4::from_scale(scale);
            s.draw(mesh, &Material::color(d.color, 0.0, 0.6), m);
        }
    }

    fn draw_chain(&self, s: &mut Scene<'_>, lab: &Lab, alpha: f32) {
        let steel = Material::color(Color::rgb8(200, 200, 210), 1.0, 0.3);
        for id in &lab.links {
            if let Some(t) = lab.world.interpolated_transform(*id, alpha) {
                let m = t.to_matrix() * Mat4::from_scale(Vec3::new(1.0, 0.24, 0.24));
                s.draw(&self.cube, &steel, m);
            }
        }
        if let Some(t) = lab.world.interpolated_transform(lab.ball, alpha) {
            let red = Material::color(Color::rgb8(220, 60, 60), 0.0, 0.4);
            s.draw(
                &self.sphere,
                &red,
                t.to_matrix() * Mat4::from_scale(Vec3::splat(0.6)),
            );
        }
        if let Some(t) = lab.world.interpolated_transform(lab.wheel, alpha) {
            let wood = Material::color(Color::rgb8(180, 120, 60), 0.0, 0.7);
            for half in [Vec3::new(1.6, 0.12, 0.5), Vec3::new(0.12, 1.6, 0.5)] {
                s.draw(
                    &self.cube,
                    &wood,
                    t.to_matrix() * Mat4::from_scale(half * 2.0),
                );
            }
        }
    }

    fn draw_ragdoll(&self, s: &mut Scene<'_>, lab: &Lab, alpha: f32) {
        let Some(doll) = &lab.ragdoll else { return };
        let skin = Material::color(Color::rgb8(230, 180, 140), 0.0, 0.8);
        for (name, id) in &doll.parts {
            if let (Some(mesh), Some(t)) = (
                self.parts.get(name),
                lab.world.interpolated_transform(*id, alpha),
            ) {
                s.draw(mesh, &skin, t);
            }
        }
    }
}

/// Passes debug lines on except the terrain mesh's own triangle edges,
/// which would cover the ground in lines: an edge of the terrain joins two
/// grid vertices that lie exactly on the height function.
struct NoTerrainEdges<'s, 'a>(SceneDebugDraw<'s, 'a>);

impl NoTerrainEdges<'_, '_> {
    fn on_terrain(p: Vec3) -> bool {
        let on_grid = |v: f32| (v - v.round()).abs() < 1.0e-3 && v.abs() <= meshes::TERRAIN_HALF;
        on_grid(p.x) && on_grid(p.z) && (p.y - meshes::terrain_height(p.x, p.z)).abs() < 1.0e-3
    }
}

impl DebugDraw for NoTerrainEdges<'_, '_> {
    fn line(&mut self, a: Vec3, b: Vec3, color: Color) {
        if !(Self::on_terrain(a) && Self::on_terrain(b)) {
            self.0.line(a, b, color);
        }
    }

    fn point(&mut self, p: Vec3, color: Color) {
        self.0.point(p, color);
    }
}
