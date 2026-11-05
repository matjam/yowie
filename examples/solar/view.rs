//! Drawing helpers for the solar example: orbit rings, name labels, the
//! schedule window and the footer.

use crate::Drawn;
use crate::common;
use crate::systems::{Asteroid, Orbit, Solar};
use yowie::ecs::{Entity, GlobalTransform, Parent};
use yowie::gfx::Font;
use yowie::gfx::scene::Camera3D;
use yowie::{Context, Rect, Vec3, vec2};

/// Line segments that trace each orbit.
pub struct Rings {
    /// Orbits around the sun.
    pub planets: Vec<(Vec3, Vec3)>,
    /// Orbits around planets.
    pub moons: Vec<(Vec3, Vec3)>,
}

const SEGMENTS: usize = 72;

/// Traces every orbit in the plane of the entity it circles.
pub fn rings(sim: &Solar) -> Rings {
    let mut out = Rings {
        planets: Vec::new(),
        moons: Vec::new(),
    };
    let w = &sim.world;
    for (orbit, parent) in w.query::<(&Orbit, &Parent)>().without::<&Asteroid>().iter() {
        let Ok(g) = w.get::<&GlobalTransform>(parent.0) else {
            continue;
        };
        let at = |i: usize| {
            let a = i as f32 / SEGMENTS as f32 * std::f32::consts::TAU;
            g.0.transform_point3(Vec3::new(
                orbit.radius * a.cos(),
                0.0,
                orbit.radius * a.sin(),
            ))
        };
        let list = if parent.0 == sim.sun {
            &mut out.planets
        } else {
            &mut out.moons
        };
        list.extend((0..SEGMENTS).map(|i| (at(i), at(i + 1))));
    }
    out
}

/// Writes each body's name beside it; the selected one stands out.
pub fn labels(
    ctx: &mut Context,
    font: &Font,
    cam: &Camera3D,
    sim: &Solar,
    bodies: &[Drawn],
    selected: Entity,
) {
    let view = ctx.gfx.view_size();
    for d in bodies {
        let parent = sim.world.get::<&Parent>(d.entity).ok().map(|p| p.0);
        let moon = parent.is_some_and(|p| p != sim.sun);
        // Moons crowd their planet, so a moon is named only while it or its
        // planet is selected.
        if moon && d.entity != selected && parent != Some(selected) {
            continue;
        }
        let at = d.matrix.w_axis.truncate();
        let Some(p) = cam.world_to_screen(at, view) else {
            continue;
        };
        let (size, color) = match (d.entity == selected, moon) {
            (true, _) => (16.0, common::ACCENT),
            (false, true) => (11.0, common::DIM),
            (false, false) => (14.0, common::TEXT),
        };
        let name = sim.name(d.entity);
        // Planet names sit below the body and moon names above and beside
        // their moon, so a planet's name and its moons' never meet.
        let offset = if moon {
            vec2(8.0, -20.0)
        } else {
            vec2(-12.0, 14.0 + (d.body.radius - 0.5).max(0.0) * 20.0)
        };
        common::label(ctx, font, &name, p + offset, size, color);
    }
}

/// The window that lists each system's run time and the CPU scopes.
pub fn schedule_window(
    ctx: &mut Context,
    window: &mut Rect,
    sim: &Solar,
    scopes: [f32; 3],
    timings: bool,
) {
    let stats = sim.schedule.stats();
    ctx.ui.draw(&mut ctx.gfx, &ctx.input, |f| {
        f.window("Schedule", window, |f| {
            f.heading("systems");
            f.grid(2, |f| {
                for s in &stats {
                    f.label(&s.name);
                    if timings {
                        f.label(&format!("{:.0} us", s.last.as_secs_f64() * 1e6));
                    } else {
                        f.label(&format!("{} runs", s.runs));
                    }
                }
            });
            f.separator();
            f.heading("scopes, CPU ms");
            f.grid(2, |f| {
                for (name, ms) in ["systems", "minimap", "scene"].iter().zip(scopes) {
                    f.label(name);
                    f.label(&if timings {
                        format!("{ms:.3}")
                    } else {
                        "-".to_owned()
                    });
                }
            });
        });
    });
}

/// The status panel along the bottom.
pub fn footer(ctx: &mut Context, font: &Font, sim: &Solar, selected: Entity) {
    let view = ctx.gfx.view_size();
    let area = Rect::new(12.0, view.y - 70.0, 600.0, 58.0);
    common::panel(ctx, area);
    let mut line = format!("selected: {}", sim.name(selected));
    if let Ok(b) = sim.world.get::<&crate::systems::Body>(selected) {
        line += &format!(", radius {:.2}", b.radius);
    }
    if let Ok(p) = sim.world.get::<&Parent>(selected) {
        line += &format!(", circles {}", sim.name(p.0));
    }
    let info = format!("{} entities; click a body to select it", sim.world.len());
    common::label(
        ctx,
        font,
        &info,
        vec2(22.0, area.y + 8.0),
        16.0,
        common::TEXT,
    );
    common::label(
        ctx,
        font,
        &line,
        vec2(22.0, area.y + 32.0),
        15.0,
        common::DIM,
    );
}
