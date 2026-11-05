//! Real-world constants for games set in our solar system.
//!
//! The rest of the crate is unit-agnostic. This module fixes the units to
//! metres, kilograms and seconds, and gives the Sun, the planets, Pluto and
//! the major moons with their masses, mean radii and orbits.
//!
//! Planet orbits are the JPL mean Keplerian elements for 1800 to 2050 at
//! the J2000 epoch (2000-01-01 12:00 TT), referred to the ecliptic and
//! equinox of J2000. The ecliptic is this crate's XY plane. Earth's orbit is
//! that of the Earth-Moon barycentre. They are accurate to arcminutes over a
//! few centuries, and they treat each orbit as a fixed ellipse: no
//! precession, no perturbations.
//!
//! The Moon's orbit is a mean orbit referred to the ecliptic. The other
//! moons are given by semi-major axis, eccentricity and inclination to the
//! parent's equator or Laplace plane only; their other angles are 0, since
//! where a moon is at the epoch is not meaningful for a game. Rotate them
//! with [`Elements::node`](crate::Elements::node) and the other angles if
//! you want them spread out. Moon orbits are not rotated into the ecliptic.
//!
//! Load the whole system into a [`System`](crate::System) with [`load`].

mod moons;
mod planets;

use std::f64::consts::PI;

use crate::kepler::{true_anomaly_from_mean, wrap_angle};
use crate::{BodyId, Elements, State, System};

pub use moons::*;
pub use planets::*;

/// The astronomical unit in metres.
pub const AU: f64 = 1.495_978_707e11;
/// A day of 86,400 seconds.
pub const DAY: f64 = 86_400.0;
/// A Julian year of 365.25 days, in seconds.
pub const YEAR: f64 = 365.25 * DAY;
/// The speed of light in metres per second.
pub const C: f64 = 299_792_458.0;
/// The standard gravitational parameter of the Sun, in m³/s².
pub const MU_SUN: f64 = 1.327_124_400_18e20;
/// The standard gravitational parameter of the Earth, in m³/s².
pub const MU_EARTH: f64 = 3.986_004_418e14;
/// The standard gravitational parameter of the Moon, in m³/s².
pub const MU_MOON: f64 = 4.904_869_5e12;
/// The standard gravitational parameter of Jupiter, in m³/s².
pub const MU_JUPITER: f64 = 1.266_865_34e17;
/// The mean Earth-Moon distance in metres.
pub const MOON_DISTANCE: f64 = 384_399.0e3;

/// Degrees to radians, usable in constants.
pub(crate) const fn deg(d: f64) -> f64 {
    d * (PI / 180.0)
}

/// A real orbit around a primary body.
///
/// Angles are radians. The mean anomaly is at the J2000 epoch.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Orbit {
    /// The name of the body it orbits, as in [`Body::name`].
    pub primary: &'static str,
    /// Semi-major axis in metres.
    pub semi_major_axis: f64,
    /// Eccentricity.
    pub eccentricity: f64,
    /// Inclination.
    pub inclination: f64,
    /// Longitude of the ascending node.
    pub node: f64,
    /// Argument of periapsis.
    pub arg_periapsis: f64,
    /// Mean anomaly at J2000.
    pub mean_anomaly: f64,
}

impl Orbit {
    /// The orbit as [`Elements`] at the J2000 epoch, with the mean anomaly
    /// converted to a true anomaly.
    pub fn elements(&self) -> Elements {
        Elements {
            semi_major_axis: self.semi_major_axis,
            eccentricity: self.eccentricity,
            inclination: self.inclination,
            node: wrap_angle(self.node),
            arg_periapsis: wrap_angle(self.arg_periapsis),
            true_anomaly: true_anomaly_from_mean(self.mean_anomaly, self.eccentricity),
        }
    }
}

/// A star, planet or moon.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Body {
    /// The English name.
    pub name: &'static str,
    /// Mass in kilograms.
    pub mass: f64,
    /// Mean radius in metres.
    pub radius: f64,
    /// The orbit around its primary, or `None` for the Sun.
    pub orbit: Option<Orbit>,
}

impl Body {
    /// The gravitational parameter `G * mass` in m³/s².
    pub fn mu(&self) -> f64 {
        crate::G * self.mass
    }
}

/// The Sun.
pub const SUN: Body = Body {
    name: "Sun",
    mass: 1.988_41e30,
    radius: 6.957e8,
    orbit: None,
};

/// Looks a body up by name, ignoring case.
pub fn by_name(name: &str) -> Option<&'static Body> {
    ALL.iter().find(|b| b.name.eq_ignore_ascii_case(name))
}

/// Adds the Sun, the planets, Pluto and the moons to `system`, with the Sun
/// fixed at the origin, and returns each body with its id. The system's
/// gravitational constant should be [`crate::G`]. Orbits are as at J2000, at
/// the system's time 0.
pub fn load(system: &mut System) -> Vec<(&'static Body, BodyId)> {
    let mut ids: Vec<(&'static Body, BodyId)> = Vec::with_capacity(ALL.len());
    for body in &ALL {
        let id = match body.orbit {
            None => Some(system.add_fixed(body.mass, State::default())),
            Some(orbit) => {
                ids.iter()
                    .find(|(b, _)| b.name == orbit.primary)
                    .and_then(|&(_, primary)| {
                        system
                            .add_orbiting(body.mass, primary, orbit.elements())
                            .ok()
                    })
            }
        };
        if let Some(id) = id {
            ids.push((body, id));
        }
    }
    ids
}

/// Every body, ordered so that each primary comes before its moons.
pub static ALL: [Body; 31] = [
    SUN, MERCURY, VENUS, EARTH, MARS, JUPITER, SATURN, URANUS, NEPTUNE, PLUTO, MOON, PHOBOS,
    DEIMOS, IO, EUROPA, GANYMEDE, CALLISTO, MIMAS, ENCELADUS, TETHYS, DIONE, RHEA, TITAN, IAPETUS,
    MIRANDA, ARIEL, UMBRIEL, TITANIA, OBERON, TRITON, CHARON,
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::TAU;

    #[test]
    fn every_primary_precedes_its_moons_and_exists() {
        for (i, b) in ALL.iter().enumerate() {
            if let Some(o) = b.orbit {
                let at = ALL.iter().position(|p| p.name == o.primary);
                assert!(at.is_some_and(|p| p < i), "{} orbits {}", b.name, o.primary);
                assert!((0.0..1.0).contains(&o.eccentricity), "{}", b.name);
                assert!(
                    o.semi_major_axis > 0.0 && b.mass > 0.0 && b.radius > 0.0,
                    "{}",
                    b.name
                );
            }
        }
        assert_eq!(by_name("earth").map(|b| b.name), Some("Earth"));
        assert!(by_name("Vulcan").is_none());
    }

    #[test]
    fn constants_agree_with_each_other() {
        assert!((SUN.mu() / MU_SUN - 1.0).abs() < 1e-3);
        assert!((EARTH.mu() / MU_EARTH - 1.0).abs() < 1e-3);
        assert!((MOON.mu() / MU_MOON - 1.0).abs() < 1e-3);
        assert!((JUPITER.mu() / MU_JUPITER - 1.0).abs() < 1e-3);
        assert_eq!(YEAR, 31_557_600.0);
        assert!((C / AU - 1.0 / 499.004_784).abs() < 1e-9);
    }

    #[test]
    fn planet_periods_match_known_years() {
        // Sidereal periods in Julian years.
        let cases = [
            ("Mercury", 0.240_846),
            ("Venus", 0.615_198),
            ("Earth", 1.000_017),
            ("Mars", 1.880_816),
            ("Jupiter", 11.862_6),
            ("Saturn", 29.447_5),
            ("Uranus", 84.016_8),
            ("Neptune", 164.791_3),
            ("Pluto", 247.94),
        ];
        for (name, years) in cases {
            let b = by_name(name).expect("known planet");
            let orbit = b.orbit.expect("planets orbit the Sun");
            let t = orbit.elements().period(MU_SUN).expect("closed orbit") / YEAR;
            assert!((t / years - 1.0).abs() < 2e-3, "{name}: {t} vs {years}");
        }
    }

    #[test]
    fn moon_periods_match_known_values() {
        // Sidereal periods in days.
        let cases = [
            ("Moon", 27.3217),
            ("Io", 1.7691),
            ("Europa", 3.5512),
            ("Ganymede", 7.1546),
            ("Callisto", 16.689),
            ("Titan", 15.945),
            ("Rhea", 4.5175),
            ("Enceladus", 1.3702),
            ("Titania", 8.7059),
            ("Triton", 5.8769),
            ("Charon", 6.3872),
        ];
        for (name, days) in cases {
            let b = by_name(name).expect("known moon");
            let orbit = b.orbit.expect("moons have orbits");
            let parent = by_name(orbit.primary).expect("primary exists");
            let mu = parent.mu() + b.mu();
            let t = TAU * (orbit.semi_major_axis.powi(3) / mu).sqrt() / DAY;
            assert!((t / days - 1.0).abs() < 5e-3, "{name}: {t} vs {days}");
        }
    }

    #[test]
    fn loading_builds_a_working_system() {
        let mut sys = System::default();
        let ids = load(&mut sys);
        assert_eq!(ids.len(), ALL.len());
        let find = |name: &str| ids.iter().find(|(b, _)| b.name == name).map(|&(_, id)| id);
        let (earth, moon, jupiter) = (find("Earth"), find("Moon"), find("Jupiter"));
        let at = |id: Option<BodyId>| id.and_then(|i| sys.body_state(i)).unwrap_or_default();
        let d_earth = at(earth).pos.length();
        assert!((0.97 * AU..1.03 * AU).contains(&d_earth), "{d_earth}");
        let d_moon = (at(moon).pos - at(earth).pos).length();
        assert!((3.5e8..4.1e8).contains(&d_moon), "{d_moon}");
        let d_jupiter = at(jupiter).pos.length();
        assert!((4.9 * AU..5.5 * AU).contains(&d_jupiter), "{d_jupiter}");
        // Earth is back after a year, to within the Moon's wobble of the
        // barycentre it is not modelled as (Earth is the barycentre here).
        let start = at(earth).pos;
        let period = TAU * (EARTH.orbit.map_or(1.0, |o| o.semi_major_axis).powi(3) / MU_SUN).sqrt();
        sys.step(period);
        let end = sys
            .body_state(earth.unwrap_or(BodyId(0)))
            .unwrap_or_default()
            .pos;
        assert!((end - start).length() < 1e-6 * AU);
    }
}
