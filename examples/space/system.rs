//! The fictional star system of the space example: a star, seven planets
//! with moons, a comet and a belt of rocks. Nothing here is our solar
//! system: masses and distances are game units and `G` is 1.

use yowie::Color;
use yowie::orbit::{BodyId, DVec3, Elements, ShipId, State, System};
use yowie::rng::Rng;

/// Mass of the star. With `G = 1` the inner planet takes two minutes to go
/// round and the outer one an hour.
pub const STAR_MASS: f64 = 175.0;

struct Planet {
    name: &'static str,
    a: f64,
    e: f64,
    incl: f64,
    node: f64,
    arg: f64,
    mass: f64,
    radius: f32,
    color: (u8, u8, u8),
    /// Semi-major axis and inclination of each moon.
    moons: &'static [(f64, f64)],
}

const PLANETS: [Planet; 7] = [
    Planet {
        name: "Ash",
        a: 40.0,
        e: 0.15,
        incl: 0.02,
        node: 0.5,
        arg: 1.0,
        mass: 0.3,
        radius: 0.45,
        color: (190, 150, 120),
        moons: &[],
    },
    Planet {
        name: "Ferro",
        a: 70.0,
        e: 0.05,
        incl: 0.03,
        node: 2.0,
        arg: 0.4,
        mass: 0.8,
        radius: 0.8,
        color: (200, 110, 80),
        moons: &[(2.6, 0.1)],
    },
    Planet {
        name: "Vessa",
        a: 110.0,
        e: 0.03,
        incl: 0.01,
        node: 4.0,
        arg: 2.2,
        mass: 1.2,
        radius: 1.0,
        color: (80, 150, 210),
        moons: &[(5.0, 0.05), (8.0, 0.25)],
    },
    Planet {
        name: "Grond",
        a: 170.0,
        e: 0.08,
        incl: 0.04,
        node: 1.2,
        arg: 3.1,
        mass: 4.0,
        radius: 1.7,
        color: (200, 170, 120),
        moons: &[(4.5, 0.0), (7.0, 0.1), (11.0, 0.3)],
    },
    Planet {
        name: "Pell",
        a: 265.0,
        e: 0.12,
        incl: 0.15,
        node: 3.5,
        arg: 0.9,
        mass: 0.05,
        radius: 0.3,
        color: (160, 160, 170),
        moons: &[],
    },
    Planet {
        name: "Nimbus",
        a: 380.0,
        e: 0.05,
        incl: 0.02,
        node: 5.5,
        arg: 1.7,
        mass: 8.0,
        radius: 2.3,
        color: (150, 190, 230),
        moons: &[(5.5, 0.02), (8.5, 0.05), (13.0, 0.1), (21.0, 0.4)],
    },
    Planet {
        name: "Thule",
        a: 600.0,
        e: 0.2,
        incl: 0.3,
        node: 0.3,
        arg: 2.5,
        mass: 0.2,
        radius: 0.45,
        color: (200, 220, 240),
        moons: &[],
    },
];

const NUMERALS: [&str; 4] = ["I", "II", "III", "IV"];

/// How to draw and follow one body of the system.
pub struct Look {
    /// Shown on its label.
    pub name: String,
    /// Sphere radius in game units.
    pub radius: f32,
    /// Surface colour.
    pub color: Color,
    /// Glow strength; 0 reflects light only.
    pub glow: f32,
    /// The body in the [`System`].
    pub id: BodyId,
    /// Index of the body this one orbits; `None` for the star.
    pub parent: Option<usize>,
    /// Its orbit at time 0 around the parent, with the parent's `mu`.
    pub orbit: Option<(Elements, f64)>,
    /// Whether Tab can focus it.
    pub focusable: bool,
}

/// A massless rock of the belt: it follows its orbit and pulls on nothing.
pub struct Rock {
    /// Orbit around the star at time 0.
    pub elements: Elements,
}

/// The whole simulation.
pub struct Cosmos {
    /// Bodies on Kepler rails, and the ship that flies among them.
    pub sys: System,
    /// What to draw for each body; index 0 is the star.
    pub bodies: Vec<Look>,
    /// The belt.
    pub rocks: Vec<Rock>,
    /// The player's ship.
    pub ship: ShipId,
}

fn rgb(c: (u8, u8, u8)) -> Color {
    Color::rgb8(c.0, c.1, c.2)
}

impl Cosmos {
    /// Builds the system with the ship in a low orbit around `Vessa`.
    pub fn new() -> Self {
        let mut rng = Rng::new(11);
        let mut sys = System::new(1.0);
        let mut bodies = Vec::new();
        let star = sys.add_fixed(STAR_MASS, State::default());
        bodies.push(Look {
            name: "Kaal".into(),
            radius: 4.0,
            color: Color::rgb8(255, 214, 130),
            glow: 5.0,
            id: star,
            parent: None,
            orbit: None,
            focusable: true,
        });
        let add = |sys: &mut System,
                   bodies: &mut Vec<Look>,
                   mass: f64,
                   parent: usize,
                   el: Elements,
                   mut look: Look| {
            let mu = sys.g * sys.body_mass(bodies[parent].id).unwrap_or(0.0);
            if let Ok(id) = sys.add_orbiting(mass, bodies[parent].id, el) {
                look.id = id;
                look.parent = Some(parent);
                look.orbit = Some((el, mu));
                bodies.push(look);
            }
        };
        let blank = |name: String, radius: f32, color: Color, focusable: bool| Look {
            name,
            radius,
            color,
            glow: 0.0,
            id: star,
            parent: None,
            orbit: None,
            focusable,
        };
        for p in &PLANETS {
            let el = Elements {
                semi_major_axis: p.a,
                eccentricity: p.e,
                inclination: p.incl,
                node: p.node,
                arg_periapsis: p.arg,
                true_anomaly: rng.float64() * std::f64::consts::TAU,
            };
            let look = blank(p.name.into(), p.radius, rgb(p.color), true);
            add(&mut sys, &mut bodies, p.mass, 0, el, look);
            let planet = bodies.len() - 1;
            for (i, (a, incl)) in p.moons.iter().enumerate() {
                let el = Elements {
                    semi_major_axis: *a,
                    inclination: *incl,
                    node: rng.float64() * 6.0,
                    true_anomaly: rng.float64() * std::f64::consts::TAU,
                    ..Elements::default()
                };
                let name = format!("{} {}", p.name, NUMERALS[i]);
                let look = blank(
                    name,
                    0.12 + 0.05 * i as f32,
                    Color::rgb8(190, 190, 200),
                    false,
                );
                add(&mut sys, &mut bodies, p.mass * 0.008, planet, el, look);
            }
        }
        let comet = Elements {
            semi_major_axis: 330.0,
            eccentricity: 0.93,
            inclination: 0.55,
            node: 4.2,
            arg_periapsis: 1.1,
            true_anomaly: 2.4,
        };
        let mut look = blank("Comet Iri".into(), 0.1, Color::rgb8(210, 240, 255), true);
        look.glow = 3.0;
        add(&mut sys, &mut bodies, 0.0, 0, comet, look);

        let rocks = (0..300)
            .map(|_| Rock {
                elements: Elements {
                    semi_major_axis: f64::from(rng.between(225.0, 305.0)),
                    eccentricity: f64::from(rng.between(0.0, 0.12)),
                    inclination: f64::from(rng.between(-0.08, 0.08)),
                    node: rng.float64() * 6.0,
                    arg_periapsis: rng.float64() * 6.0,
                    true_anomaly: rng.float64() * std::f64::consts::TAU,
                },
            })
            .collect();

        // The ship starts in a low circular orbit around Vessa, inside
        // its moons.
        let vessa = bodies.iter().position(|b| b.name == "Vessa").unwrap_or(1);
        let v = sys.body_state(bodies[vessa].id).unwrap_or_default();
        let mu = sys.g * sys.body_mass(bodies[vessa].id).unwrap_or(1.0);
        let rel = Elements {
            semi_major_axis: 2.4,
            true_anomaly: std::f64::consts::PI,
            ..Elements::default()
        }
        .state(mu);
        let ship = sys.add_ship(State {
            pos: v.pos + rel.pos,
            vel: v.vel + rel.vel,
        });
        Cosmos {
            sys,
            bodies,
            rocks,
            ship,
        }
    }

    /// The ship's state.
    pub fn ship_state(&self) -> State {
        self.sys
            .ship(self.ship)
            .map(|s| s.state)
            .unwrap_or_default()
    }

    /// The position of body `i`.
    pub fn body_pos(&self, i: usize) -> DVec3 {
        self.sys
            .body_state(self.bodies[i].id)
            .map_or(DVec3::ZERO, |s| s.pos)
    }

    /// Where a belt rock is now.
    pub fn rock_pos(&self, r: &Rock) -> DVec3 {
        let mu = self.sys.g * STAR_MASS;
        r.elements.at_time(mu, self.sys.time()).state(mu).pos
    }
}
