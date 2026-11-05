use super::{AU, Body, Orbit, deg};

/// A heliocentric orbit from JPL's table of mean elements at J2000: the
/// semi-major axis in AU, eccentricity, inclination, mean longitude,
/// longitude of perihelion and longitude of the ascending node, the angles
/// in degrees.
const fn planet(a_au: f64, e: f64, incl: f64, mean_lon: f64, peri_lon: f64, node: f64) -> Orbit {
    Orbit {
        primary: "Sun",
        semi_major_axis: a_au * AU,
        eccentricity: e,
        inclination: deg(incl.abs()),
        node: deg(node),
        arg_periapsis: deg(peri_lon - node),
        mean_anomaly: deg(mean_lon - peri_lon),
    }
}

/// Mercury.
pub const MERCURY: Body = Body {
    name: "Mercury",
    mass: 3.3011e23,
    radius: 2.4397e6,
    orbit: Some(planet(
        0.387_099_27,
        0.205_635_93,
        7.004_979_02,
        252.250_323_50,
        77.457_796_28,
        48.330_765_93,
    )),
};

/// Venus.
pub const VENUS: Body = Body {
    name: "Venus",
    mass: 4.8675e24,
    radius: 6.0518e6,
    orbit: Some(planet(
        0.723_335_66,
        0.006_776_72,
        3.394_676_05,
        181.979_099_50,
        131.602_467_18,
        76.679_842_55,
    )),
};

/// Earth. The orbit is that of the Earth-Moon barycentre.
pub const EARTH: Body = Body {
    name: "Earth",
    mass: 5.9722e24,
    radius: 6.371e6,
    orbit: Some(planet(
        1.000_002_61,
        0.016_711_23,
        0.000_015_31,
        100.464_571_66,
        102.937_681_93,
        0.0,
    )),
};

/// Mars.
pub const MARS: Body = Body {
    name: "Mars",
    mass: 6.4171e23,
    radius: 3.3895e6,
    orbit: Some(planet(
        1.523_710_34,
        0.093_394_10,
        1.849_691_42,
        -4.553_432_05,
        -23.943_629_59,
        49.559_538_91,
    )),
};

/// Jupiter.
pub const JUPITER: Body = Body {
    name: "Jupiter",
    mass: 1.8982e27,
    radius: 6.9911e7,
    orbit: Some(planet(
        5.202_887_00,
        0.048_386_24,
        1.304_396_95,
        34.396_440_51,
        14.728_479_83,
        100.473_909_09,
    )),
};

/// Saturn.
pub const SATURN: Body = Body {
    name: "Saturn",
    mass: 5.6834e26,
    radius: 5.8232e7,
    orbit: Some(planet(
        9.536_675_94,
        0.053_861_79,
        2.485_991_87,
        49.954_244_23,
        92.598_878_31,
        113.662_424_48,
    )),
};

/// Uranus.
pub const URANUS: Body = Body {
    name: "Uranus",
    mass: 8.6810e25,
    radius: 2.5362e7,
    orbit: Some(planet(
        19.189_164_64,
        0.047_257_44,
        0.772_637_83,
        313.238_104_51,
        170.954_276_30,
        74.016_925_03,
    )),
};

/// Neptune.
pub const NEPTUNE: Body = Body {
    name: "Neptune",
    mass: 1.02409e26,
    radius: 2.4622e7,
    orbit: Some(planet(
        30.069_922_76,
        0.008_590_48,
        1.770_043_47,
        -55.120_029_69,
        44.964_762_27,
        131.784_225_74,
    )),
};

/// Pluto, a dwarf planet. The orbit is from JPL's table for 1800 to 2050.
pub const PLUTO: Body = Body {
    name: "Pluto",
    mass: 1.303e22,
    radius: 1.1883e6,
    orbit: Some(planet(
        39.481_686_77,
        0.248_807_66,
        17.141_75,
        238.928_81,
        224.066_76,
        110.303_47,
    )),
};
