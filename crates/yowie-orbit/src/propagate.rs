use std::f64::consts::PI;

use crate::State;

/// Advances a state exactly along its two-body orbit around a primary with
/// gravitational parameter `mu`, by `dt` (which may be negative).
///
/// Uses universal variables, so one routine serves ellipses, parabolas and
/// hyperbolas, including orbits that are nearly parabolic. The state is
/// relative to the primary. For a circular orbit, `propagate` over one
/// period returns the starting state.
pub fn propagate(s: State, mu: f64, dt: f64) -> State {
    if dt == 0.0 {
        return s;
    }
    let r0 = s.pos.length();
    let v0 = s.vel.length();
    let rdotv = s.pos.dot(s.vel);
    let sqmu = mu.sqrt();
    // Reciprocal of the semi-major axis: positive for ellipses, zero for
    // parabolas, negative for hyperbolas.
    let alpha = -v0 * v0 / mu + 2.0 / r0;
    let mut chi = if alpha > 1e-12 {
        sqmu * dt * alpha
    } else if alpha.abs() <= 1e-12 {
        let h = s.pos.cross(s.vel);
        let p = h.dot(h) / mu;
        let sv = 0.5 * (1.0 / (3.0 * (mu / (p * p * p)).sqrt() * dt)).atan();
        let wv = sv.tan().cbrt().atan();
        p.sqrt() * 2.0 / (2.0 * wv).tan()
    } else {
        let a = 1.0 / alpha;
        let sgn = dt.signum();
        let num = -2.0 * mu * alpha * dt;
        let den = rdotv + sgn * (-mu * a).sqrt() * (1.0 - r0 * alpha);
        sgn * (-a).sqrt() * (num / den).ln()
    };
    if !chi.is_finite() {
        chi = sqmu * dt / r0;
    }
    let radius = |chi: f64| {
        let psi = chi * chi * alpha;
        let (c2, c3) = stumpff(psi);
        let r = chi * chi * c2 + rdotv / sqmu * chi * (1.0 - psi * c3) + r0 * (1.0 - psi * c2);
        (psi, c2, c3, r)
    };
    for _ in 0..100 {
        let (psi, c2, c3, r) = radius(chi);
        let next = chi
            + (sqmu * dt
                - chi.powi(3) * c3
                - rdotv / sqmu * chi * chi * c2
                - r0 * chi * (1.0 - psi * c3))
                / r;
        let done = (next - chi).abs() < 1e-10 * chi.abs().max(1.0);
        chi = next;
        if done {
            break;
        }
    }
    let (psi, c2, c3, r) = radius(chi);
    let f = 1.0 - chi * chi / r0 * c2;
    let g = dt - chi.powi(3) / sqmu * c3;
    let gdot = 1.0 - chi * chi / r * c2;
    let fdot = sqmu / (r * r0) * chi * (psi * c3 - 1.0);
    State {
        pos: s.pos * f + s.vel * g,
        vel: s.pos * fdot + s.vel * gdot,
    }
}

/// The Stumpff functions `C2(psi)` and `C3(psi)`.
fn stumpff(psi: f64) -> (f64, f64) {
    if psi > 1e-6 {
        let sq = psi.sqrt();
        ((1.0 - sq.cos()) / psi, (sq - sq.sin()) / (sq * sq * sq))
    } else if psi < -1e-6 {
        let sq = (-psi).sqrt();
        ((1.0 - sq.cosh()) / psi, (sq.sinh() - sq) / (sq * sq * sq))
    } else {
        (0.5, 1.0 / 6.0)
    }
}

/// The specific orbital energy (energy per unit mass) of `s` around a
/// primary with gravitational parameter `mu`. Negative for closed orbits.
pub fn energy(s: State, mu: f64) -> f64 {
    s.vel.dot(s.vel) / 2.0 - mu / s.pos.length()
}

/// The speed of a circular orbit of radius `r`.
pub fn circular_velocity(mu: f64, r: f64) -> f64 {
    (mu / r).sqrt()
}

/// The speed needed to escape from radius `r` on a parabola.
pub fn escape_velocity(mu: f64, r: f64) -> f64 {
    (2.0 * mu / r).sqrt()
}

/// The speed at distance `r` on an orbit with semi-major axis `a` (the
/// vis-viva equation). `a` is negative for hyperbolas.
pub fn vis_viva(mu: f64, r: f64, a: f64) -> f64 {
    (mu * (2.0 / r - 1.0 / a)).sqrt()
}

/// A Hohmann transfer between two coplanar circular orbits.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hohmann {
    /// Speed change at the start to enter the transfer ellipse.
    pub dv1: f64,
    /// Speed change at the far end to circularise.
    pub dv2: f64,
    /// Time spent on the transfer ellipse: half its period.
    pub coast: f64,
}

/// Computes the Hohmann transfer between circular orbits of radii `r1`
/// and `r2` around a primary with gravitational parameter `mu`. The speed
/// changes are magnitudes, whether the transfer goes up or down.
pub fn hohmann(mu: f64, r1: f64, r2: f64) -> Hohmann {
    let at = (r1 + r2) / 2.0;
    Hohmann {
        dv1: (vis_viva(mu, r1, at) - circular_velocity(mu, r1)).abs(),
        dv2: (circular_velocity(mu, r2) - vis_viva(mu, r2, at)).abs(),
        coast: PI * (at.powi(3) / mu).sqrt(),
    }
}

/// The radius of the sphere of influence of a body of mass `m` orbiting a
/// primary of mass `big_m` at distance `a`: `a (m / M)^(2/5)`. Inside it,
/// the body's gravity is the better centre for a two-body approximation.
pub fn sphere_of_influence(a: f64, m: f64, big_m: f64) -> f64 {
    a * (m / big_m).powf(0.4)
}

/// The radius of the Hill sphere of a body of mass `m` orbiting a primary
/// of mass `big_m` on an orbit of semi-major axis `a` and eccentricity `e`:
/// `a (1 - e) (m / 3M)^(1/3)`. Moons can only orbit stably well inside it.
pub fn hill_radius(a: f64, e: f64, m: f64, big_m: f64) -> f64 {
    a * (1.0 - e) * (m / (3.0 * big_m)).cbrt()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Elements, sol};

    const MU: f64 = 3.986004418e14;

    fn rel(a: f64, b: f64) -> f64 {
        (a - b).abs() / a.abs().max(b.abs()).max(1e-300)
    }

    fn elliptic() -> State {
        Elements {
            semi_major_axis: 1.2e7,
            eccentricity: 0.5,
            inclination: 0.6,
            node: 0.4,
            arg_periapsis: 1.1,
            true_anomaly: 0.9,
        }
        .state(MU)
    }

    fn hyperbolic() -> State {
        Elements {
            semi_major_axis: -3.0e7,
            eccentricity: 1.4,
            inclination: 0.3,
            node: 2.0,
            arg_periapsis: 0.5,
            true_anomaly: 0.2,
        }
        .state(MU)
    }

    #[test]
    fn a_full_period_returns_to_the_start() {
        let el = Elements {
            semi_major_axis: 1.2e7,
            eccentricity: 0.5,
            inclination: 0.6,
            node: 0.4,
            arg_periapsis: 1.1,
            true_anomaly: 0.9,
        };
        let period = el.period(MU).unwrap();
        let s = el.state(MU);
        let end = propagate(s, MU, period);
        assert!((end.pos - s.pos).length() < 1e-6 * s.pos.length());
        assert!((end.vel - s.vel).length() < 1e-9 * s.vel.length());
    }

    #[test]
    fn energy_and_angular_momentum_are_conserved() {
        for s in [elliptic(), hyperbolic()] {
            let e0 = energy(s, MU);
            let h0 = s.pos.cross(s.vel);
            for dt in [100.0, 1.0e3, 2.5e4, -7.0e3] {
                let t = propagate(s, MU, dt);
                assert!(rel(energy(t, MU), e0) < 1e-9, "energy dt {dt}");
                assert!(
                    (t.pos.cross(t.vel) - h0).length() < 1e-9 * h0.length(),
                    "h dt {dt}"
                );
            }
        }
    }

    #[test]
    fn forward_then_back_is_identity() {
        for s in [elliptic(), hyperbolic()] {
            let there = propagate(s, MU, 9000.0);
            let back = propagate(there, MU, -9000.0);
            assert!((back.pos - s.pos).length() < 1e-6 * s.pos.length());
            assert!((back.vel - s.vel).length() < 1e-9 * s.vel.length());
        }
    }

    #[test]
    fn splitting_a_step_gives_the_same_result() {
        let s = hyperbolic();
        let one = propagate(s, MU, 6000.0);
        let two = propagate(propagate(s, MU, 2500.0), MU, 3500.0);
        assert!((one.pos - two.pos).length() < 1e-6 * one.pos.length());
    }

    #[test]
    fn parabola_has_zero_energy_and_escapes() {
        let r = 7.0e6;
        let s = State {
            pos: glam_vec(r, 0.0, 0.0),
            vel: glam_vec(0.0, escape_velocity(MU, r), 0.0),
        };
        for dt in [500.0, 5000.0, 50000.0] {
            let t = propagate(s, MU, dt);
            assert!(energy(t, MU).abs() < 1e-6 * MU / r, "dt {dt}");
            assert!(t.pos.length() > r);
        }
    }

    fn glam_vec(x: f64, y: f64, z: f64) -> crate::DVec3 {
        crate::DVec3::new(x, y, z)
    }

    #[test]
    fn closed_form_formulas() {
        let r = 6.771e6;
        assert!(rel(circular_velocity(MU, r), 7672.0) < 1e-3);
        assert!(
            rel(
                escape_velocity(MU, r),
                2f64.sqrt() * circular_velocity(MU, r)
            ) < 1e-12
        );
        assert!(rel(vis_viva(MU, r, r), circular_velocity(MU, r)) < 1e-12);
        // LEO to GEO transfer needs about 2.4 km/s and 5.3 hours of coasting.
        let h = hohmann(MU, 6.678e6, 4.2164e7);
        assert!((h.dv1 + h.dv2 - 3880.0).abs() < 60.0, "{h:?}");
        assert!((h.coast / 3600.0 - 5.27).abs() < 0.05, "{h:?}");
        let up = hohmann(MU, 7.0e6, 1.4e7);
        let down = hohmann(MU, 1.4e7, 7.0e6);
        assert!(rel(up.dv1 + up.dv2, down.dv1 + down.dv2) < 1e-12);
    }

    #[test]
    fn influence_spheres_of_known_bodies() {
        let a = sol::EARTH.orbit.map(|o| o.semi_major_axis).unwrap_or(0.0);
        let soi = sphere_of_influence(a, sol::EARTH.mass, sol::SUN.mass);
        // Earth's sphere of influence is about 925,000 km.
        assert!((soi / 1.0e3 - 925_000.0).abs() < 10_000.0, "{soi}");
        let hill = hill_radius(a, 0.0167, sol::EARTH.mass, sol::SUN.mass);
        assert!((hill / 1.0e3 - 1_470_000.0).abs() < 50_000.0, "{hill}");
    }
}
