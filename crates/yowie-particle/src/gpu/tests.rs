//! GPU particle tests: they render headless and read the pool back, and
//! skip with a message when there is no adapter or no compute support.

use yowie_core::{Color, uvec2, vec2};
use yowie_gfx::{Graphics, GraphicsSettings};

use super::*;
use crate::curve::Range;
use crate::{Backend, Particles, System};

const DT: f32 = 1.0 / 60.0;

fn headless() -> Option<Graphics> {
    if !yowie_gfx::adapter_available() {
        println!("skipping GPU test: no GPU adapter");
        return None;
    }
    let gfx = Graphics::new_headless(uvec2(128, 128), GraphicsSettings::default()).ok()?;
    if !gpu_supported(&gfx) {
        println!("skipping GPU test: no compute support");
        return None;
    }
    Some(gfx)
}

/// Updates and renders `frames` frames, one update step per frame.
fn run(gfx: &mut Graphics, sys: &mut GpuSystem, frames: usize) {
    for _ in 0..frames {
        sys.update(DT);
        sys.draw(gfx);
        if let Err(e) = gfx.end_frame() {
            panic!("frame failed: {e}");
        }
    }
}

fn steady(rate: f32) -> Emitter {
    Emitter {
        rate,
        lifetime: Range::constant(1.0),
        size: Range::constant(4.0),
        speed: Range::constant(0.0),
        max: 20_000,
        ..Emitter::default()
    }
}

#[test]
fn live_count_matches_the_spawn_rate() {
    let Some(mut gfx) = headless() else {
        return;
    };
    // 600 per second living one second settles near 600 alive.
    let mut sys = GpuSystem::new(&gfx, steady(600.0)).unwrap();
    run(&mut gfx, &mut sys, 120);
    let alive = sys.read_alive() as f32;
    assert!((alive - 600.0).abs() < 600.0 * 0.06, "alive {alive}");
    // The asynchronous count follows within a few frames of the GPU
    // finishing them.
    let mut seen = 0.0;
    for _ in 0..200 {
        run(&mut gfx, &mut sys, 1);
        seen = sys.alive() as f32;
        if (seen - 600.0).abs() < 600.0 * 0.1 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert!((seen - 600.0).abs() < 600.0 * 0.1, "async count {seen}");
}

#[test]
fn the_cpu_system_agrees_on_the_count() {
    let Some(mut gfx) = headless() else {
        return;
    };
    for rate in [120.0, 900.0] {
        let mut gpu = GpuSystem::new(&gfx, steady(rate)).unwrap();
        let mut cpu = System::new(steady(rate));
        for _ in 0..90 {
            cpu.update(DT);
        }
        run(&mut gfx, &mut gpu, 90);
        let (g, c) = (gpu.read_alive() as f32, cpu.alive() as f32);
        assert!(
            (g - c).abs() <= c * 0.06 + 2.0,
            "rate {rate}: gpu {g}, cpu {c}"
        );
    }
}

#[test]
fn particles_fall_under_gravity() {
    let Some(mut gfx) = headless() else {
        return;
    };
    let mut sys = GpuSystem::new(
        &gfx,
        Emitter {
            gravity: vec2(0.0, 100.0),
            lifetime: Range::constant(5.0),
            ..steady(0.0)
        },
    )
    .unwrap();
    sys.burst(50);
    run(&mut gfx, &mut sys, 61);
    let parts = sys.snapshot();
    assert_eq!(parts.len(), 50);
    // After about a second: v = g t = 100, y = g t^2 / 2 = 50.
    for p in &parts {
        assert!((p.velocity.y - 100.0).abs() < 4.0, "vy {}", p.velocity.y);
        assert!((p.position.y - 50.0).abs() < 3.0, "y {}", p.position.y);
        assert!(p.position.x.abs() < 1e-3);
    }
}

#[test]
fn the_pool_caps_and_recycles_slots() {
    let Some(mut gfx) = headless() else {
        return;
    };
    let mut sys = GpuSystem::new(
        &gfx,
        Emitter {
            max: 100,
            lifetime: Range::constant(0.5),
            ..steady(0.0)
        },
    )
    .unwrap();
    sys.burst(300);
    run(&mut gfx, &mut sys, 2);
    assert_eq!(sys.read_alive(), 100, "births stop at the capacity");
    run(&mut gfx, &mut sys, 40);
    assert_eq!(sys.read_alive(), 0, "everything died");
    sys.burst(60);
    run(&mut gfx, &mut sys, 2);
    assert_eq!(sys.read_alive(), 60, "freed slots are reused");
}

#[test]
fn a_gpu_system_draws_where_its_particles_are() {
    let Some(mut gfx) = headless() else {
        return;
    };
    let mut sys = GpuSystem::at(
        &gfx,
        Emitter {
            color: Color::RED,
            size: Range::constant(20.0),
            lifetime: Range::constant(5.0),
            burst: 1,
            ..steady(0.0)
        },
        vec2(64.0, 64.0),
    )
    .unwrap();
    gfx.begin_frame();
    sys.update(DT);
    sys.draw(&mut gfx);
    gfx.request_capture();
    let _ = gfx.end_frame();
    let img = match gfx.take_capture() {
        Some(Ok(i)) => i,
        _ => panic!("no capture"),
    };
    let centre = img.get_pixel(64, 64).0;
    assert!(
        centre[0] > 200 && centre[1] < 40,
        "red at the centre: {centre:?}"
    );
    assert_eq!(img.get_pixel(10, 10).0[0], 0, "nothing far away");
}

/// Measures 100,000 live additive particles at 960 by 540: run with
/// `cargo test -p yowie-particle gpu_throughput -- --ignored --nocapture`.
#[test]
#[ignore = "a measurement, not a check"]
fn gpu_throughput() {
    if !yowie_gfx::adapter_available() {
        return;
    }
    let Ok(mut gfx) = Graphics::new_headless(uvec2(960, 540), GraphicsSettings::default()) else {
        return;
    };
    let emitter = Emitter {
        rate: 50_000.0,
        lifetime: Range::constant(2.0),
        speed: Range::new(20.0, 120.0),
        spread: std::f32::consts::TAU,
        gravity: vec2(0.0, 60.0),
        drag: 0.3,
        size: Range::constant(6.0),
        color: Color::rgba(1.0, 0.6, 0.2, 0.5),
        blend: crate::Blend::Additive,
        max: 120_000,
        ..Emitter::default()
    };
    let Ok(mut sys) = GpuSystem::at(&gfx, emitter, vec2(480.0, 270.0)) else {
        return;
    };
    run(&mut gfx, &mut sys, 150);
    let live = sys.read_alive();
    let frames = 300;
    let start = std::time::Instant::now();
    for _ in 0..frames {
        sys.update(DT);
        sys.draw(&mut gfx);
        let _ = gfx.end_frame();
    }
    let _ = gfx.device().poll(wgpu::PollType::wait_indefinitely());
    let ms = start.elapsed().as_secs_f64() * 1000.0 / f64::from(frames);
    println!("{live} live particles: {ms:.2} ms per frame (update, simulate and draw)");
}

#[test]
fn particles_draw_in_a_3d_scene_and_hide_behind_geometry() {
    use yowie_core::{Mat4, Vec3};
    use yowie_gfx::scene::{Camera3D, Material, Mesh, MeshData};
    let Some(mut gfx) = headless() else {
        return;
    };
    let Ok(wall_mesh) = Mesh::new(&gfx, &MeshData::cube(8.0)) else {
        panic!("wall mesh");
    };
    let mut sys = GpuSystem::new(
        &gfx,
        Emitter {
            color: Color::RED,
            size: Range::constant(2.0),
            lifetime: Range::constant(5.0),
            burst: 1,
            blend: crate::Blend::Additive,
            ..steady(0.0)
        },
    )
    .unwrap();
    let camera = Camera3D::perspective(Vec3::new(0.0, 0.0, 6.0), Vec3::ZERO, 1.0);
    let render = |gfx: &mut Graphics, sys: &mut GpuSystem, wall: bool| {
        gfx.begin_frame();
        sys.update(DT);
        gfx.scene(&camera, |s| {
            if wall {
                // A wall between the camera and the particle plane.
                let place = Mat4::from_translation(Vec3::new(0.0, 0.0, 3.0))
                    * Mat4::from_scale(Vec3::new(1.0, 1.0, 0.1));
                s.draw(&wall_mesh, &Material::color(Color::BLUE, 0.0, 0.5), place);
            }
            sys.draw_3d(s);
        });
        gfx.request_capture();
        let _ = gfx.end_frame();
        match gfx.take_capture() {
            Some(Ok(i)) => i,
            _ => panic!("no capture"),
        }
    };
    let open = render(&mut gfx, &mut sys, false);
    let c = open.get_pixel(64, 64).0;
    assert!(
        c[0] > 120 && c[0] > c[2] * 2,
        "red particle at the centre: {c:?}"
    );
    let hidden = render(&mut gfx, &mut sys, true);
    let h = hidden.get_pixel(64, 64).0;
    assert!(h[2] > h[0], "the wall hides the particle: {h:?}");
}

#[test]
fn switching_backends_keeps_the_effect() {
    let Some(mut gfx) = headless() else {
        return;
    };
    let mut p = Particles::at(&gfx, steady(300.0), vec2(30.0, 40.0), Backend::Cpu);
    assert_eq!(p.backend(), Backend::Cpu);
    for _ in 0..30 {
        p.update(DT);
    }
    p.switch_to(&gfx, Backend::Gpu);
    assert_eq!(p.backend(), Backend::Gpu);
    assert_eq!(p.position(), vec2(30.0, 40.0));
    assert_eq!(p.emitter().rate, 300.0);
    for _ in 0..90 {
        p.update(DT);
        p.draw(&mut gfx);
        let _ = gfx.end_frame();
    }
    let Some(gpu) = p.gpu_mut() else {
        panic!("the GPU backend is running");
    };
    let alive = gpu.read_alive() as f32;
    assert!((alive - 300.0).abs() < 30.0, "alive {alive}");
}
