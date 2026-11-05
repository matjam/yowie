use std::f32::consts::FRAC_PI_2;

use yowie_core::{Color, vec2};

use super::*;

const DT: f32 = 1.0 / 60.0;

fn run(system: &mut System, seconds: f32) {
    for _ in 0..(seconds / DT).round() as usize {
        system.update(DT);
    }
}

fn base() -> Emitter {
    Emitter {
        lifetime: Range::constant(1.0),
        size: Range::constant(4.0),
        ..Emitter::default()
    }
}

#[test]
fn continuous_emission_births_at_the_rate() {
    for rate in [10.0, 60.0, 250.0] {
        let mut s = System::new(Emitter {
            rate,
            lifetime: Range::constant(100.0),
            ..base()
        });
        run(&mut s, 2.0);
        let want = (rate * 2.0) as i64;
        assert!(
            (s.alive() as i64 - want).abs() <= 1,
            "rate {rate}: {} alive, want about {want}",
            s.alive()
        );
    }
}

#[test]
fn bursts_are_born_on_time_and_capped() {
    let mut s = System::new(Emitter {
        burst: 30,
        bursts: vec![
            Burst {
                time: 0.5,
                count: 20,
            },
            Burst {
                time: 0.25,
                count: 10,
            },
        ],
        lifetime: Range::constant(100.0),
        max: 55,
        ..base()
    });
    assert_eq!(s.alive(), 30, "the start burst is born at once");
    run(&mut s, 0.3);
    assert_eq!(s.alive(), 40, "the 0.25 s burst, not the 0.5 s one");
    run(&mut s, 0.4);
    assert_eq!(s.alive(), 55, "max stops births");
    assert!(!s.emitting());
}

#[test]
fn particles_live_their_lifetime_then_die() {
    let mut s = System::new(Emitter {
        burst: 200,
        lifetime: Range::new(0.4, 0.9),
        ..base()
    });
    assert!(s.particles().iter().all(|p| (0.4..=0.9).contains(&p.life)));
    run(&mut s, 0.35);
    assert_eq!(s.alive(), 200, "nobody dies before the shortest lifetime");
    run(&mut s, 0.65);
    assert_eq!(s.alive(), 0, "everybody is gone after the longest");
    assert!(s.finished());
}

#[test]
fn duration_and_stop_end_emission() {
    let mut s = System::new(Emitter {
        rate: 100.0,
        duration: Some(0.5),
        ..base()
    });
    run(&mut s, 0.4);
    let mid = s.alive();
    assert!(s.emitting() && mid > 30);
    run(&mut s, 2.0);
    assert!(s.finished(), "stops by duration and the last particle dies");
    s.start();
    s.stop();
    run(&mut s, 0.1);
    assert_eq!(s.alive(), 0);
}

#[test]
fn same_seed_same_particles() {
    let make = |seed| {
        let mut s = System::new(Emitter {
            rate: 80.0,
            speed: Range::new(10.0, 50.0),
            spread: 1.0,
            seed,
            ..base()
        });
        run(&mut s, 1.0);
        s.particles().iter().map(|p| p.position).collect::<Vec<_>>()
    };
    assert_eq!(make(7), make(7));
    assert_ne!(make(7), make(8));
}

#[test]
fn shapes_place_births_inside_their_bounds() {
    let birth = |shape: Shape| {
        let mut s = System::new(Emitter {
            burst: 500,
            shape,
            ..base()
        });
        s.set_position(vec2(0.0, 0.0));
        s.particles().to_vec()
    };
    let circle = birth(Shape::Circle {
        radius: 10.0,
        edge: false,
    });
    assert!(circle.iter().all(|p| p.position.length() <= 10.001));
    assert!(circle.iter().any(|p| p.position.length() < 5.0));
    let ring = birth(Shape::Circle {
        radius: 10.0,
        edge: true,
    });
    assert!(
        ring.iter()
            .all(|p| (p.position.length() - 10.0).abs() < 1e-3)
    );
    let rect = birth(Shape::Rect {
        w: 20.0,
        h: 6.0,
        edge: false,
    });
    assert!(
        rect.iter()
            .all(|p| p.position.x.abs() <= 10.0 && p.position.y.abs() <= 3.0)
    );
    let border = birth(Shape::Rect {
        w: 20.0,
        h: 6.0,
        edge: true,
    });
    assert!(border.iter().all(|p| {
        let (x, y) = (p.position.x.abs(), p.position.y.abs());
        ((x - 10.0).abs() < 1e-3 && y <= 3.001) || ((y - 3.0).abs() < 1e-3 && x <= 10.001)
    }));
    let line = birth(Shape::Line {
        to: vec2(30.0, -40.0),
    });
    assert!(
        line.iter()
            .all(|p| (p.position.x * -40.0 - p.position.y * 30.0).abs() < 1e-2)
    );
}

#[test]
fn cone_directions_stay_inside_the_opening() {
    let mut s = System::new(Emitter {
        burst: 500,
        shape: Shape::Cone {
            angle: 0.8,
            base_radius: 3.0,
        },
        direction: -FRAC_PI_2,
        speed: Range::constant(10.0),
        ..base()
    });
    assert!(s.particles().iter().all(|p| {
        let a = p.velocity.y.atan2(p.velocity.x);
        (a - (-FRAC_PI_2)).abs() <= 0.4 + 1e-4 && p.position.length() <= 3.001
    }));
    s.clear();
    assert_eq!(s.alive(), 0);
}

#[test]
fn gravity_drag_and_attractors_move_particles() {
    let one = |e: Emitter| {
        let mut s = System::new(Emitter {
            burst: 1,
            lifetime: Range::constant(10.0),
            ..e
        });
        run(&mut s, 1.0);
        s.particles()[0]
    };
    let falling = one(Emitter {
        gravity: vec2(0.0, 100.0),
        ..base()
    });
    assert!((falling.velocity.y - 100.0).abs() < 2.0);
    assert!(falling.position.y > 40.0);
    let fast = one(Emitter {
        speed: Range::constant(100.0),
        ..base()
    });
    let dragged = one(Emitter {
        speed: Range::constant(100.0),
        drag: 2.0,
        ..base()
    });
    assert!(dragged.position.x < fast.position.x * 0.7);
    let pulled = one(Emitter {
        attractors: vec![Attractor {
            position: vec2(0.0, 50.0),
            strength: 200.0,
            radius: 100.0,
        }],
        ..base()
    });
    assert!(
        pulled.position.y > 5.0,
        "pulled towards +y: {:?}",
        pulled.position
    );
    let pushed = one(Emitter {
        attractors: vec![Attractor {
            position: vec2(0.0, 50.0),
            strength: -200.0,
            radius: 100.0,
        }],
        ..base()
    });
    assert!(pushed.position.y < -5.0);
    // Outside the radius an attractor does nothing.
    let far = one(Emitter {
        attractors: vec![Attractor {
            position: vec2(0.0, 500.0),
            strength: 200.0,
            radius: 100.0,
        }],
        ..base()
    });
    assert_eq!(far.position, vec2(0.0, 0.0));
}

#[test]
fn world_space_particles_are_born_at_the_system_position() {
    let born = |world_space| {
        let e = Emitter {
            burst: 1,
            world_space,
            lifetime: Range::constant(5.0),
            ..base()
        };
        System::at(e, vec2(100.0, 50.0)).particles()[0].position
    };
    assert_eq!(born(true), vec2(100.0, 50.0));
    // A local particle is stored relative to the system.
    assert_eq!(born(false), vec2(0.0, 0.0));
}

#[test]
fn json_round_trips_and_applies_defaults() {
    let e = Emitter {
        rate: 12.0,
        shape: Shape::Cone {
            angle: 0.5,
            base_radius: 2.0,
        },
        color_over_life: vec![
            ColorKey {
                t: 0.0,
                color: Color::rgb(1.0, 0.0, 0.0),
            },
            ColorKey {
                t: 1.0,
                color: Color::rgba(0.0, 0.0, 1.0, 0.0),
            },
        ],
        size_over_life: Curve::linear(1.0, 0.0),
        attractors: vec![Attractor {
            position: vec2(1.0, 2.0),
            strength: 3.0,
            radius: 4.0,
        }],
        bursts: vec![Burst {
            time: 1.0,
            count: 5,
        }],
        blend: Blend::Additive,
        texture: Some("soft".into()),
        ..Emitter::default()
    };
    let json = e.to_json().expect("serialises");
    assert_eq!(Emitter::from_json(&json).expect("parses"), e);
    let minimal = Emitter::from_json(r#"{"rate": 5}"#).expect("parses");
    assert_eq!(
        minimal,
        Emitter {
            rate: 5.0,
            ..Emitter::default()
        }
    );
    assert!(Emitter::from_json(r#"{"blend": "plaid"}"#).is_err());
    assert!(Emitter::from_json("[").is_err());
}

#[test]
fn presets_run_and_sparks_burst() {
    let mut fire = System::new(presets::fire());
    run(&mut fire, 1.0);
    assert!(fire.alive() > 50);
    let sparks = System::new(presets::sparks());
    assert_eq!(sparks.alive(), 40);
    let mut rain = presets::rain();
    rain.prewarm = 1.0;
    assert!(System::new(rain).alive() > 100);
    assert_eq!(System::new(presets::confetti()).alive(), 150);
    let img = presets::soft_circle(16);
    assert!(img.get_pixel(8, 8).0[3] > 240);
    assert_eq!(img.get_pixel(0, 0).0[3], 0);
}
