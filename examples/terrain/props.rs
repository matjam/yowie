//! Things standing on the ground, built in code: pines, rocks at three
//! levels of detail, the tree picture for billboards, campfires and the
//! searchlight tower.

use std::f32::consts::TAU;

use yowie::gfx::Graphics;
use yowie::gfx::image::{Rgba, RgbaImage};
use yowie::gfx::scene::{
    Lod, Material, Mesh, MeshData, Model, PointLight, Scene, SpotLight, Vertex3D,
};
use yowie::rng::Noise;
use yowie::{Color, GameResult, Mat4, Quat, Vec3, vec2, vec3};

/// A cone of `radius` at its base (y = 0) narrowing to a point `height`
/// above it, with `segments` around and a closed base.
fn cone(radius: f32, height: f32, segments: u32) -> MeshData {
    let mut data = MeshData::default();
    let slope = radius / height;
    for s in 0..segments {
        let (a0, a1) = (
            s as f32 / segments as f32 * TAU,
            (s + 1) as f32 / segments as f32 * TAU,
        );
        let mid = (a0 + a1) * 0.5;
        let normal = |a: f32| vec3(a.sin(), slope, a.cos()).normalize();
        let base = data.vertices.len() as u32;
        let rim = |a: f32| vec3(a.sin() * radius, 0.0, a.cos() * radius);
        data.vertices
            .push(Vertex3D::new(rim(a0), normal(a0), vec2(a0 / TAU, 1.0)));
        data.vertices
            .push(Vertex3D::new(rim(a1), normal(a1), vec2(a1 / TAU, 1.0)));
        data.vertices.push(Vertex3D::new(
            Vec3::Y * height,
            normal(mid),
            vec2(mid / TAU, 0.0),
        ));
        data.indices.extend([base, base + 1, base + 2]);
        let cap = data.vertices.len() as u32;
        data.vertices
            .push(Vertex3D::new(rim(a0), Vec3::NEG_Y, vec2(0.0, 0.0)));
        data.vertices
            .push(Vertex3D::new(Vec3::ZERO, Vec3::NEG_Y, vec2(0.5, 0.5)));
        data.vertices
            .push(Vertex3D::new(rim(a1), Vec3::NEG_Y, vec2(1.0, 0.0)));
        data.indices.extend([cap, cap + 1, cap + 2]);
    }
    data.compute_tangents();
    data
}

/// A pine: a trunk under three stacked cones of needles. About 6 units
/// tall with its base at the origin.
pub fn pine(gfx: &Graphics) -> GameResult<Model> {
    let trunk = Mesh::new(gfx, &MeshData::cylinder(0.22, 2.2, 10))?;
    let bark = Material::color(Color::rgb8(92, 64, 44), 0.0, 0.95);
    let needles = Material::color(Color::rgb8(46, 92, 52), 0.0, 0.85);
    let mut parts = vec![(trunk, bark, Mat4::from_translation(Vec3::Y * 1.1))];
    for (radius, height, y) in [(1.7, 2.8, 1.2), (1.35, 2.4, 2.5), (0.95, 2.1, 3.7)] {
        let mesh = Mesh::new(gfx, &cone(radius, height, 14))?;
        parts.push((mesh, needles.clone(), Mat4::from_translation(Vec3::Y * y)));
    }
    Ok(Model::from_parts(&parts)?)
}

/// A lumpy unit rock with `rings` bands: the same lumps at every detail.
fn rock(rings: u32) -> MeshData {
    let noise = Noise::new(77);
    let mut data = MeshData::sphere(1.0, rings, rings * 2);
    for v in &mut data.vertices {
        let p = v.position;
        let lump = noise.fbm(p.x * 1.3 + p.y * 0.7, p.z * 1.3 - p.y * 0.9, 3, 2.0, 0.5);
        v.position = p * Vec3::new(1.3, 0.75, 1.0) * (1.0 + lump * 0.55);
    }
    data.compute_normals();
    data.compute_tangents();
    data
}

/// Rocks at three levels of detail: the finest until a rock covers less
/// than 4% of the view's height, the middle down to 1.2%, the coarsest
/// down to 0.25%, then nothing.
pub fn rocks(gfx: &Graphics) -> GameResult<Lod<Mesh>> {
    Ok(Lod::new(Mesh::new(gfx, &rock(16))?)
        .then(Mesh::new(gfx, &rock(8))?, 0.04)
        .then(Mesh::new(gfx, &rock(4))?, 0.012)
        .cull_below(0.0025))
}

/// A broad-leaved tree on a transparent background for billboards, its
/// trunk at the bottom middle.
pub fn tree_image() -> RgbaImage {
    let noise = Noise::new(5);
    let (w, h) = (64u32, 96u32);
    let blobs = [
        (32.0, 36.0, 22.0),
        (20.0, 46.0, 15.0),
        (44.0, 46.0, 15.0),
        (32.0, 22.0, 15.0),
    ];
    RgbaImage::from_fn(w, h, |x, y| {
        let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
        let n = noise.fbm(fx / 9.0, fy / 9.0, 3, 2.0, 0.5);
        let inside = blobs
            .iter()
            .map(|(cx, cy, r)| 1.0 - ((fx - cx).hypot(fy - cy) / r))
            .fold(f32::MIN, f32::max);
        if inside + n * 0.35 > 0.0 {
            let shade = (0.75 + n * 0.8 - (fy - 20.0) / 160.0).clamp(0.4, 1.1);
            let c = |v: f32| (v * shade).clamp(0.0, 255.0) as u8;
            return Rgba([c(78.0), c(128.0), c(50.0), 255]);
        }
        if (fx - 32.0).abs() < 3.5 - (fy - 60.0).max(0.0) * 0.02 && fy > 50.0 {
            return Rgba([86, 62, 42, 255]);
        }
        Rgba([0, 0, 0, 0])
    })
}

/// The meshes and materials a campfire draws with.
pub struct FireParts {
    log: Mesh,
    stone: Mesh,
    flame: Mesh,
    wood: Material,
    rock: Material,
    hot: Material,
    core: Material,
}

impl FireParts {
    /// Builds the fire's meshes.
    pub fn new(gfx: &Graphics) -> GameResult<Self> {
        Ok(Self {
            log: Mesh::new(gfx, &MeshData::cylinder(0.12, 1.3, 8))?,
            stone: Mesh::new(gfx, &rock(4))?,
            flame: Mesh::new(gfx, &cone(0.32, 1.0, 10))?,
            wood: Material::color(Color::rgb8(70, 48, 32), 0.0, 0.95),
            rock: Material::color(Color::rgb8(105, 100, 95), 0.0, 0.9),
            hot: Material::glowing(Color::rgb8(255, 120, 30), 6.0),
            core: Material::glowing(Color::rgb8(255, 210, 120), 9.0),
        })
    }
}

pub fn draw_fire(s: &mut Scene, parts: &FireParts, at: Vec3, t: f32, seed: f32) {
    let flicker = |rate: f32, phase: f32| 0.5 + 0.5 * (t * rate + seed * 7.3 + phase).sin();
    for i in 0..8 {
        let a = i as f32 / 8.0 * TAU;
        let place = Mat4::from_scale_rotation_translation(
            Vec3::splat(0.22),
            Quat::from_rotation_y(a * 1.7),
            at + vec3(a.cos() * 0.75, 0.05, a.sin() * 0.75),
        );
        s.draw(&parts.stone, &parts.rock, place);
    }
    for i in 0..3 {
        let a = i as f32 / 3.0 * TAU + seed;
        let tilt = Quat::from_rotation_y(a) * Quat::from_rotation_x(1.25);
        let place = Mat4::from_rotation_translation(tilt, at + Vec3::Y * 0.2);
        s.draw(&parts.log, &parts.wood, place);
    }
    for i in 0..3 {
        let a = i as f32 / 3.0 * TAU + seed * 2.0;
        let lean = vec3(a.cos(), 0.0, a.sin()) * 0.12;
        let tall = 0.75 + 0.45 * flicker(9.0 + i as f32 * 2.3, i as f32);
        let place = Mat4::from_scale_rotation_translation(
            vec3(1.0, tall, 1.0),
            Quat::from_rotation_y(t * 1.5 + a),
            at + lean + Vec3::Y * 0.15,
        );
        s.draw(&parts.flame, &parts.hot, place);
    }
    let core = 0.5 + 0.3 * flicker(13.0, 1.0);
    let place = Mat4::from_scale_rotation_translation(
        vec3(0.6, core, 0.6),
        Quat::from_rotation_y(-t * 2.0),
        at + Vec3::Y * 0.12,
    );
    s.draw(&parts.flame, &parts.core, place);
    let glow = 0.75 + 0.25 * flicker(11.0, 0.0) * flicker(17.0, 2.0);
    let wander = vec3((t * 7.0 + seed).sin(), 0.0, (t * 5.0 + seed).cos()) * 0.08;
    s.add_point_light(PointLight::new(
        at + Vec3::Y * 0.9 + wander,
        Color::rgb(1.0, 0.55, 0.22),
        14.0 * glow,
        14.0,
    ));
}

/// The searchlight tower's meshes and materials.
pub struct Tower {
    leg: Mesh,
    cabin: Mesh,
    lamp: Mesh,
    steel: Material,
    lens: Material,
}

impl Tower {
    /// Builds the tower's meshes.
    pub fn new(gfx: &Graphics) -> GameResult<Self> {
        Ok(Self {
            leg: Mesh::new(gfx, &MeshData::cylinder(0.12, 9.0, 8))?,
            cabin: Mesh::new(gfx, &MeshData::cube(1.0))?,
            lamp: Mesh::new(gfx, &MeshData::cylinder(0.45, 0.7, 16))?,
            steel: Material::color(Color::rgb8(80, 84, 90), 0.8, 0.45),
            lens: Material::glowing(Color::rgb8(255, 246, 220), 12.0),
        })
    }

    /// Draws the tower standing at `base` with its lamp sweeping round
    /// over `t` seconds, and adds the lamp's shadowed spot light.
    pub fn draw(&self, s: &mut Scene, base: Vec3, t: f32) {
        for (dx, dz) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
            let foot = base + vec3(dx * 1.1, 4.5, dz * 1.1);
            s.draw(&self.leg, &self.steel, foot);
        }
        let top = base + Vec3::Y * 9.4;
        let cabin = Mat4::from_scale_rotation_translation(vec3(3.0, 0.8, 3.0), Quat::IDENTITY, top);
        s.draw(&self.cabin, &self.steel, cabin);
        let yaw = t * 0.45;
        let dir = vec3(yaw.cos() * 0.85, -0.42, yaw.sin() * 0.85).normalize();
        let lamp_at = top + Vec3::Y * 0.9;
        let turn = Quat::from_rotation_arc(Vec3::Y, dir);
        s.draw(
            &self.lamp,
            &self.lens,
            Mat4::from_rotation_translation(turn, lamp_at),
        );
        s.add_spot_light(
            SpotLight::new(
                lamp_at + dir * 0.5,
                dir,
                Color::rgb(1.0, 0.96, 0.85),
                40.0,
                140.0,
            )
            .with_cone(6f32.to_radians(), 11f32.to_radians())
            .with_shadows(),
        );
    }
}
