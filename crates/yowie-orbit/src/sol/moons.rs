use super::{Body, Orbit, deg};

/// An orbit around `primary` given by semi-major axis (kilometres),
/// eccentricity and inclination (degrees). The other angles are 0.
const fn moon_orbit(primary: &'static str, a_km: f64, e: f64, incl: f64) -> Orbit {
    Orbit {
        primary,
        semi_major_axis: a_km * 1.0e3,
        eccentricity: e,
        inclination: deg(incl),
        node: 0.0,
        arg_periapsis: 0.0,
        mean_anomaly: 0.0,
    }
}

const fn moon(
    name: &'static str,
    primary: &'static str,
    mass: f64,
    radius_km: f64,
    orbit: (f64, f64, f64),
) -> Body {
    Body {
        name,
        mass,
        radius: radius_km * 1.0e3,
        orbit: Some(moon_orbit(primary, orbit.0, orbit.1, orbit.2)),
    }
}

/// The Moon. A mean orbit referred to the ecliptic, with its angles at
/// J2000.
pub const MOON: Body = Body {
    name: "Moon",
    mass: 7.342e22,
    radius: 1.7374e6,
    orbit: Some(Orbit {
        primary: "Earth",
        semi_major_axis: 384_399.0e3,
        eccentricity: 0.0549,
        inclination: deg(5.145),
        node: deg(125.08),
        arg_periapsis: deg(318.15),
        mean_anomaly: deg(135.27),
    }),
};

/// Phobos, a moon of Mars.
pub const PHOBOS: Body = moon("Phobos", "Mars", 1.0659e16, 11.1, (9_376.0, 0.0151, 1.08));
/// Deimos, a moon of Mars.
pub const DEIMOS: Body = moon("Deimos", "Mars", 1.4762e15, 6.2, (23_463.2, 0.00033, 1.79));

/// Io, a moon of Jupiter.
pub const IO: Body = moon(
    "Io",
    "Jupiter",
    8.931_938e22,
    1821.6,
    (421_700.0, 0.0041, 0.050),
);
/// Europa, a moon of Jupiter.
pub const EUROPA: Body = moon(
    "Europa",
    "Jupiter",
    4.799_844e22,
    1560.8,
    (671_100.0, 0.0094, 0.471),
);
/// Ganymede, a moon of Jupiter and the largest moon in the solar system.
pub const GANYMEDE: Body = moon(
    "Ganymede",
    "Jupiter",
    1.4819e23,
    2634.1,
    (1_070_400.0, 0.0013, 0.204),
);
/// Callisto, a moon of Jupiter.
pub const CALLISTO: Body = moon(
    "Callisto",
    "Jupiter",
    1.075_938e23,
    2410.3,
    (1_882_700.0, 0.0074, 0.205),
);

/// Mimas, a moon of Saturn.
pub const MIMAS: Body = moon(
    "Mimas",
    "Saturn",
    3.7493e19,
    198.2,
    (185_539.0, 0.0196, 1.574),
);
/// Enceladus, a moon of Saturn.
pub const ENCELADUS: Body = moon(
    "Enceladus",
    "Saturn",
    1.080_22e20,
    252.1,
    (237_948.0, 0.0047, 0.009),
);
/// Tethys, a moon of Saturn.
pub const TETHYS: Body = moon(
    "Tethys",
    "Saturn",
    6.174_49e20,
    531.1,
    (294_619.0, 0.0001, 1.12),
);
/// Dione, a moon of Saturn.
pub const DIONE: Body = moon(
    "Dione",
    "Saturn",
    1.095_452e21,
    561.4,
    (377_396.0, 0.0022, 0.019),
);
/// Rhea, a moon of Saturn.
pub const RHEA: Body = moon(
    "Rhea",
    "Saturn",
    2.306_518e21,
    763.8,
    (527_108.0, 0.001_258, 0.345),
);
/// Titan, a moon of Saturn with a thick atmosphere.
pub const TITAN: Body = moon(
    "Titan",
    "Saturn",
    1.3452e23,
    2574.7,
    (1_221_870.0, 0.0288, 0.348),
);
/// Iapetus, a moon of Saturn.
pub const IAPETUS: Body = moon(
    "Iapetus",
    "Saturn",
    1.805_635e21,
    734.5,
    (3_560_820.0, 0.028_612, 15.47),
);

/// Miranda, a moon of Uranus.
pub const MIRANDA: Body = moon(
    "Miranda",
    "Uranus",
    6.4e19,
    235.8,
    (129_390.0, 0.0013, 4.338),
);
/// Ariel, a moon of Uranus.
pub const ARIEL: Body = moon(
    "Ariel",
    "Uranus",
    1.251e21,
    578.9,
    (191_020.0, 0.0012, 0.041),
);
/// Umbriel, a moon of Uranus.
pub const UMBRIEL: Body = moon(
    "Umbriel",
    "Uranus",
    1.275e21,
    584.7,
    (265_986.0, 0.0039, 0.128),
);
/// Titania, a moon of Uranus and its largest.
pub const TITANIA: Body = moon(
    "Titania",
    "Uranus",
    3.527e21,
    788.4,
    (435_910.0, 0.0011, 0.079),
);
/// Oberon, a moon of Uranus.
pub const OBERON: Body = moon(
    "Oberon",
    "Uranus",
    3.014e21,
    761.4,
    (583_520.0, 0.0014, 0.068),
);

/// Triton, the large retrograde moon of Neptune.
pub const TRITON: Body = moon(
    "Triton",
    "Neptune",
    2.139e22,
    1353.4,
    (354_759.0, 0.000_016, 156.885),
);

/// Charon, the large moon of Pluto.
pub const CHARON: Body = moon(
    "Charon",
    "Pluto",
    1.586e21,
    606.0,
    (19_591.0, 0.0002, 0.001),
);
