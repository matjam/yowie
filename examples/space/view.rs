//! Drawing the star system: bodies, orbit rings, the predicted path, the
//! ship, name labels and the ship window.

use yowie::gfx::scene::{Camera3D, DirectionalLight, Material, Mesh, PointLight, Scene, Sky};
use yowie::gfx::{Font, TextStyle};
use yowie::orbit::DVec3;
use yowie::{Color, Context, Mat4, Quat, Rect, Vec3, vec2};

use crate::path::Around;
use crate::system::Cosmos;

/// Meshes the scene draws.
pub struct Meshes {
    pub sphere: Mesh,
    pub hull: Mesh,
}

/// Everything about one frame's view that the drawing needs.
pub struct Viewpoint {
    /// System position the scene is centred on: the floating origin.
    pub origin: DVec3,
    /// Camera distance from the origin.
    pub dist: f32,
    /// Ship thrust acceleration, for the exhaust.
    pub thrust: Vec3,
}

fn rel(p: DVec3, origin: DVec3) -> Vec3 {
    (p - origin).as_vec3()
}

fn scaled(c: Color, k: f32) -> Color {
    Color::rgb(c.r * k, c.g * k, c.b * k)
}

/// The camera: orbits the origin with +Z up, since the system's reference
/// plane is X-Y.
pub fn camera(yaw: f32, pitch: f32, dist: f32) -> Camera3D {
    let (sp, cp) = pitch.sin_cos();
    let (sy, cy) = yaw.sin_cos();
    Camera3D {
        position: Vec3::new(dist * cp * cy, dist * cp * sy, dist * sp),
        up: Vec3::Z,
        ..Camera3D::perspective(Vec3::ZERO, Vec3::ZERO, 0.8)
            .with_depth_range((dist * 0.01).max(0.02), 20_000.0)
    }
}

/// Records the whole system into `s`.
pub fn record(s: &mut Scene<'_>, c: &Cosmos, m: &Meshes, v: &Viewpoint, path: &[Vec3]) {
    let black = Color::rgb8(2, 2, 6);
    s.set_sky(Sky {
        up: Vec3::Z,
        vacuum: 1.0,
        stars: 1.0,
        sun_size: 0.0,
        zenith: black,
        horizon: black,
        ground: black,
        ..Sky::default()
    });
    s.set_ambient(Color::rgb8(14, 15, 24));
    // At this scale the star is a directional light travelling from it
    // towards the origin, weaker the farther out the origin is.
    let star = rel(c.body_pos(0), v.origin);
    let warm = Color::rgb(1.0, 0.93, 0.8);
    if star.length() > 1.0 {
        let strength = (110.0 * 110.0 / star.length_squared()).clamp(0.25, 3.0);
        s.set_sun(DirectionalLight::new(-star, warm, 3.0 * strength));
    } else {
        // Centred on the star, light has no single direction: the star
        // lights the system as a point.
        s.add_point_light(PointLight::new(star, warm, 9000.0, 5000.0));
    }
    for (i, b) in c.bodies.iter().enumerate() {
        let r = b.radius.max(v.dist * 0.003);
        let material = if b.glow > 0.0 {
            Material::glowing(b.color, b.glow)
        } else {
            Material::color(b.color, 0.0, 0.9)
        };
        let at = Mat4::from_translation(rel(c.body_pos(i), v.origin));
        s.draw(&m.sphere, &material, at * Mat4::from_scale(Vec3::splat(r)));
    }
    let rock_size = v.dist * 0.0012;
    let rocks: Vec<Mat4> = c
        .rocks
        .iter()
        .map(|r| {
            let at = Mat4::from_translation(rel(c.rock_pos(r), v.origin));
            at * Mat4::from_scale(Vec3::splat(rock_size.max(0.08)))
        })
        .collect();
    s.draw_instanced(
        &m.sphere,
        &Material::color(Color::rgb(0.55, 0.53, 0.5), 0.0, 1.0),
        &rocks,
    );
    rings(s, c, v);
    for w in path.windows(2) {
        s.line_on_top(w[0], w[1], Color::rgb8(120, 220, 255));
    }
    ship(s, c, m, v);
}

/// Each body's orbit as a ring around where its primary is now. Moons show
/// only when close enough for the rings to read.
fn rings(s: &mut Scene<'_>, c: &Cosmos, v: &Viewpoint) {
    for b in &c.bodies {
        let (Some((mut el, mu)), Some(parent)) = (b.orbit, b.parent) else {
            continue;
        };
        let moon = parent != 0;
        if moon && v.dist > 200.0 {
            continue;
        }
        let n = if moon { 48 } else { 160 };
        let centre = c.body_pos(parent);
        let color = scaled(b.color, 0.55);
        let point = |el: &mut yowie::orbit::Elements, i: usize| {
            el.true_anomaly = std::f64::consts::TAU * (i % n) as f64 / n as f64;
            rel(el.state(mu).pos + centre, v.origin)
        };
        let mut prev = point(&mut el, 0);
        for i in 1..=n {
            let next = point(&mut el, i);
            s.line(prev, next, color);
            prev = next;
        }
    }
}

fn ship(s: &mut Scene<'_>, c: &Cosmos, m: &Meshes, v: &Viewpoint) {
    let state = c.ship_state();
    let at = rel(state.pos, v.origin);
    let heading = state.vel.as_vec3().normalize_or(Vec3::X);
    let size = (v.dist * 0.008).max(0.08);
    let turn = Quat::from_rotation_arc(Vec3::Y, heading);
    let body = Material {
        emissive: Color::rgb8(255, 255, 255),
        emissive_strength: 0.6,
        ..Material::color(Color::rgb8(235, 235, 245), 0.6, 0.3)
    };
    let place = Mat4::from_scale_rotation_translation(Vec3::splat(size), turn, at);
    s.draw(&m.hull, &body, place);
    if v.thrust.length() > 0.0 {
        let tail = at - v.thrust.normalize() * size * 4.0;
        s.line_on_top(at, tail, Color::rgb8(255, 170, 60));
    }
}

/// Names of the focusable bodies and the ship, projected through `cam`.
pub fn labels(ctx: &mut Context, font: &Font, cam: &Camera3D, c: &Cosmos, v: &Viewpoint) {
    let view = ctx.gfx.view_size();
    let style = TextStyle::new(14.0).color(Color::rgba(0.86, 0.88, 0.92, 0.75));
    let put = |ctx: &mut Context, name: &str, pos: DVec3| {
        let p = rel(pos, v.origin);
        if let Some(at) = cam.world_to_screen(p, view) {
            ctx.gfx.draw_text(font, name, at + vec2(8.0, -6.0), &style);
        }
    };
    for (i, b) in c.bodies.iter().enumerate() {
        let away = rel(c.body_pos(i), v.origin).length();
        let near = away < 60.0 && v.dist < 60.0;
        if (b.focusable && away < v.dist * 6.0 + 20.0) || near {
            put(ctx, &b.name, c.body_pos(i));
        }
    }
    if v.dist < 300.0 {
        put(ctx, "Ship", c.ship_state().pos);
    }
}

/// Readouts of the ship's orbit and the sliders.
pub fn window(
    ctx: &mut Context,
    rect: &mut Rect,
    c: &Cosmos,
    around: Option<&Around>,
    focus: &str,
    thrust: &mut f32,
    warp: &mut f32,
) {
    let state = c.ship_state();
    let mut lines = vec![
        format!("focus: {focus} (Tab cycles)"),
        format!(
            "speed {:.3} u/s, time {:.0} s",
            state.vel.length(),
            c.sys.time()
        ),
    ];
    if let Some(a) = around {
        let name = c
            .bodies
            .iter()
            .find(|b| b.id == a.primary)
            .map_or("?", |b| b.name.as_str());
        lines.push(format!(
            "orbiting {name}: a {:.1}, e {:.2}",
            a.elements.semi_major_axis, a.elements.eccentricity
        ));
        match a.elements.period(a.mu) {
            Some(p) => lines.push(format!(
                "periapsis {:.1}, apoapsis {:.1}, period {p:.0} s",
                a.elements.periapsis(),
                a.elements.apoapsis().unwrap_or(0.0)
            )),
            None => lines.push("escape trajectory".to_owned()),
        }
    }
    ctx.ui.draw(&mut ctx.gfx, &ctx.input, |f| {
        f.window("Ship", rect, |f| {
            for l in &lines {
                f.label(l);
            }
            f.slider("Thrust", thrust, 0.0..=0.05);
            f.slider("Time warp", warp, 1.0..=200.0);
            f.label_dim("W/S prograde and retrograde, A/D sideways,");
            f.label_dim("Q/E out of the orbital plane. Drag orbits,");
            f.label_dim("wheel zooms. Fictional system, G = 1.");
        });
    });
}
