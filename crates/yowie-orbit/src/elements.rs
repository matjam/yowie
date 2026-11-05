use std::f64::consts::TAU;

use serde::{Deserialize, Serialize};
use yowie_core::glam::{DQuat, DVec3};

use crate::kepler::{
    mean_anomaly_from_true, mean_anomaly_from_true_hyperbolic, true_anomaly_from_mean,
    true_anomaly_from_mean_hyperbolic, wrap_angle,
};
use crate::propagate;

/// How close to 1 an eccentricity must be to count as a parabola.
const PARABOLA_BAND: f64 = 1e-9;

/// A position and velocity relative to some centre.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct State {
    /// Position.
    pub pos: DVec3,
    /// Velocity.
    pub vel: DVec3,
}

/// Classical orbital elements around a primary body.
///
/// Distances are in the caller's length unit and angles in radians. For
/// hyperbolic orbits (`eccentricity > 1`) the semi-major axis is negative.
/// For a parabola (`eccentricity == 1`) it holds the periapsis distance.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Elements {
    /// Semi-major axis, or the periapsis distance for a parabola.
    pub semi_major_axis: f64,
    /// Eccentricity: 0 circular, below 1 elliptic, 1 parabolic, above 1
    /// hyperbolic.
    pub eccentricity: f64,
    /// Inclination of the orbital plane to the XY plane, in `[0, π]`.
    pub inclination: f64,
    /// Longitude of the ascending node, measured from +X.
    pub node: f64,
    /// Argument of periapsis, measured from the ascending node.
    pub arg_periapsis: f64,
    /// True anomaly: the angle from periapsis to the body.
    pub true_anomaly: f64,
}

impl Elements {
    /// A circular, equatorial orbit of the given radius, starting on +X.
    pub fn circular(radius: f64) -> Self {
        Self {
            semi_major_axis: radius,
            ..Self::default()
        }
    }

    /// Returns true when the eccentricity is within 1e-9 of 1, which the
    /// conversions treat as an exact parabola.
    pub fn is_parabolic(&self) -> bool {
        (self.eccentricity - 1.0).abs() <= PARABOLA_BAND
    }

    /// Returns true for a closed orbit: a circle or an ellipse.
    pub fn is_closed(&self) -> bool {
        self.eccentricity < 1.0 && !self.is_parabolic()
    }

    /// The closest distance to the primary.
    pub fn periapsis(&self) -> f64 {
        if self.is_parabolic() {
            return self.semi_major_axis;
        }
        self.semi_major_axis * (1.0 - self.eccentricity)
    }

    /// The farthest distance from the primary, or `None` if the orbit is
    /// open (parabolic or hyperbolic).
    pub fn apoapsis(&self) -> Option<f64> {
        self.is_closed()
            .then_some(self.semi_major_axis * (1.0 + self.eccentricity))
    }

    /// The orbital period around a primary with gravitational parameter
    /// `mu` (`G * mass`), or `None` if the orbit is open.
    pub fn period(&self, mu: f64) -> Option<f64> {
        (self.is_closed() && self.semi_major_axis > 0.0)
            .then(|| TAU * (self.semi_major_axis.powi(3) / mu).sqrt())
    }

    /// The mean motion `sqrt(mu / |a|³)` in radians per time unit.
    pub fn mean_motion(&self, mu: f64) -> f64 {
        (mu / self.semi_major_axis.powi(3).abs()).sqrt()
    }

    /// The mean anomaly (elliptic) or hyperbolic mean anomaly (hyperbolic).
    /// Not defined for parabolas, which return the true anomaly.
    pub fn mean_anomaly(&self) -> f64 {
        let (e, nu) = (self.eccentricity, self.true_anomaly);
        if self.is_parabolic() {
            nu
        } else if e < 1.0 {
            mean_anomaly_from_true(nu, e)
        } else {
            mean_anomaly_from_true_hyperbolic(nu, e)
        }
    }

    /// Converts to a position and velocity relative to the primary.
    pub fn state(&self, mu: f64) -> State {
        let e = self.eccentricity;
        let p = if self.is_parabolic() {
            2.0 * self.semi_major_axis
        } else {
            self.semi_major_axis * (1.0 - e * e)
        };
        let (sv, cv) = self.true_anomaly.sin_cos();
        let r = p / (1.0 + e * cv);
        let k = (mu / p).sqrt();
        let rot = DQuat::from_rotation_z(self.node)
            * DQuat::from_rotation_x(self.inclination)
            * DQuat::from_rotation_z(self.arg_periapsis);
        State {
            pos: rot * DVec3::new(r * cv, r * sv, 0.0),
            vel: rot * DVec3::new(-k * sv, k * (e + cv), 0.0),
        }
    }

    /// Computes the elements of the orbit through `s` around a primary with
    /// gravitational parameter `mu`.
    ///
    /// Angles that are undefined are set to 0: the node of an equatorial
    /// orbit, the periapsis of a circular one. For those, the remaining
    /// angles are measured from +X, so the result still round-trips through
    /// [`Elements::state`].
    pub fn from_state(s: State, mu: f64) -> Self {
        let (r, v) = (s.pos, s.vel);
        let rl = r.length();
        let h = r.cross(v);
        let n = DVec3::Z.cross(h);
        let ev = (r * (v.dot(v) - mu / rl) - v * r.dot(v)) / mu;
        let ecc = ev.length();
        let energy = v.dot(v) / 2.0 - mu / rl;
        let a = if (ecc - 1.0).abs() > PARABOLA_BAND {
            -mu / (2.0 * energy)
        } else {
            h.dot(h) / mu / 2.0
        };
        let mut out = Elements {
            semi_major_axis: a,
            eccentricity: ecc,
            ..Self::default()
        };
        let hl = h.length();
        if hl > 0.0 {
            out.inclination = (h.z / hl).clamp(-1.0, 1.0).acos();
        }
        let nl = n.length();
        let equatorial = nl < 1e-12 * hl.max(1e-300);
        let circular = ecc < 1e-12;
        let flip = |angle: f64, cond: bool| if cond { TAU - angle } else { angle };
        if !equatorial {
            out.node = flip((n.x / nl).clamp(-1.0, 1.0).acos(), n.y < 0.0);
        }
        if !circular {
            out.arg_periapsis = if equatorial {
                flip(ev.y.atan2(ev.x), h.z < 0.0)
            } else {
                flip((n.dot(ev) / (nl * ecc)).clamp(-1.0, 1.0).acos(), ev.z < 0.0)
            };
            out.true_anomaly = flip(
                (ev.dot(r) / (ecc * rl)).clamp(-1.0, 1.0).acos(),
                r.dot(v) < 0.0,
            );
        } else if !equatorial {
            out.true_anomaly = flip((n.dot(r) / (nl * rl)).clamp(-1.0, 1.0).acos(), r.z < 0.0);
        } else {
            out.true_anomaly = flip(r.y.atan2(r.x), h.z < 0.0);
        }
        out.node = wrap_angle(out.node);
        out.arg_periapsis = wrap_angle(out.arg_periapsis);
        out.true_anomaly = wrap_angle(out.true_anomaly);
        out
    }

    /// Returns the same orbit `dt` later, with only the true anomaly
    /// changed. Exact for every eccentricity. For hyperbolas the true
    /// anomaly is returned in `[0, 2π)` like every other angle, so values
    /// above π mean "inbound".
    pub fn at_time(&self, mu: f64, dt: f64) -> Self {
        let e = self.eccentricity;
        let mut out = *self;
        if self.is_parabolic() {
            out = Self::from_state(propagate(self.state(mu), mu, dt), mu);
        } else {
            let m = self.mean_anomaly() + self.mean_motion(mu) * dt;
            out.true_anomaly = if e < 1.0 {
                true_anomaly_from_mean(m, e)
            } else {
                wrap_angle(true_anomaly_from_mean_hyperbolic(m, e))
            };
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    /// The smallest angle between two directions.
    fn angle_diff(a: f64, b: f64) -> f64 {
        let d = (a - b).rem_euclid(TAU);
        if d > PI { TAU - d } else { d }
    }

    const MU: f64 = 3.986004418e14;

    fn close(a: DVec3, b: DVec3, tol: f64) -> bool {
        (a - b).length() <= tol * a.length().max(b.length()).max(1.0)
    }

    #[test]
    fn element_state_round_trip() {
        let cases = [
            Elements {
                semi_major_axis: 7.0e6,
                eccentricity: 0.1,
                inclination: 0.5,
                node: 1.0,
                arg_periapsis: 2.0,
                true_anomaly: 3.0,
            },
            Elements {
                semi_major_axis: 4.2e7,
                eccentricity: 0.7,
                inclination: 1.2,
                node: 5.0,
                arg_periapsis: 0.3,
                true_anomaly: 4.5,
            },
            Elements {
                semi_major_axis: 7.0e6,
                eccentricity: 0.2,
                inclination: 0.0,
                node: 0.0,
                arg_periapsis: 1.0,
                true_anomaly: 2.0,
            },
            Elements {
                semi_major_axis: 7.0e6,
                eccentricity: 0.0,
                inclination: 0.8,
                node: 2.0,
                arg_periapsis: 0.0,
                true_anomaly: 1.5,
            },
            Elements {
                semi_major_axis: 7.0e6,
                eccentricity: 0.0,
                inclination: 0.0,
                node: 0.0,
                arg_periapsis: 0.0,
                true_anomaly: 5.0,
            },
            Elements {
                semi_major_axis: -2.0e7,
                eccentricity: 1.6,
                inclination: 0.4,
                node: 3.0,
                arg_periapsis: 1.0,
                true_anomaly: 0.8,
            },
            Elements {
                semi_major_axis: 7.0e6,
                eccentricity: 0.3,
                inclination: 3.0,
                node: 1.0,
                arg_periapsis: 4.0,
                true_anomaly: 0.5,
            },
        ];
        for el in cases {
            let s = el.state(MU);
            let back = Elements::from_state(s, MU);
            let s2 = back.state(MU);
            assert!(close(s.pos, s2.pos, 1e-9), "{el:?} -> {back:?}");
            assert!(close(s.vel, s2.vel, 1e-9), "{el:?} -> {back:?}");
            assert!(
                (back.semi_major_axis / el.semi_major_axis - 1.0).abs() < 1e-9,
                "{el:?}"
            );
            assert!((back.eccentricity - el.eccentricity).abs() < 1e-9, "{el:?}");
            let well_defined =
                el.eccentricity > 1e-6 && el.inclination > 1e-6 && el.inclination < PI - 1e-6;
            if well_defined {
                for (a, b) in [
                    (back.inclination, el.inclination),
                    (back.node, el.node),
                    (back.arg_periapsis, el.arg_periapsis),
                    (back.true_anomaly, el.true_anomaly),
                ] {
                    assert!(angle_diff(a, b) < 1e-8, "{el:?} -> {back:?}");
                }
            }
        }
    }

    #[test]
    fn circular_period_matches_formula() {
        let r = 6.771e6;
        let el = Elements::circular(r);
        let want = TAU * (r.powi(3) / MU).sqrt();
        assert_eq!(el.period(MU), Some(want));
        // The speed on a circular orbit is sqrt(mu / r), perpendicular to r.
        let s = el.state(MU);
        assert!((s.vel.length() - (MU / r).sqrt()).abs() < 1e-6);
        assert!(s.pos.dot(s.vel).abs() < 1e-6);
        // After one period the body is back where it started.
        let later = el.at_time(MU, want);
        assert!(angle_diff(later.true_anomaly, el.true_anomaly) < 1e-9);
    }

    #[test]
    fn apsides_and_open_orbits() {
        let el = Elements {
            semi_major_axis: 10.0,
            eccentricity: 0.5,
            ..Elements::default()
        };
        assert_eq!((el.periapsis(), el.apoapsis()), (5.0, Some(15.0)));
        let hyp = Elements {
            semi_major_axis: -10.0,
            eccentricity: 2.0,
            ..Elements::default()
        };
        assert_eq!(
            (hyp.periapsis(), hyp.apoapsis(), hyp.period(MU)),
            (10.0, None, None)
        );
    }

    #[test]
    fn at_time_matches_state_propagation() {
        let cases = [
            Elements {
                semi_major_axis: 7.0e6,
                eccentricity: 0.4,
                inclination: 0.5,
                node: 1.0,
                arg_periapsis: 2.0,
                true_anomaly: 1.0,
            },
            Elements {
                semi_major_axis: -2.0e7,
                eccentricity: 1.8,
                inclination: 0.4,
                node: 3.0,
                arg_periapsis: 1.0,
                true_anomaly: 0.3,
            },
            Elements {
                semi_major_axis: 1.0e7,
                eccentricity: 1.0,
                inclination: 0.2,
                node: 0.5,
                arg_periapsis: 0.1,
                true_anomaly: 0.7,
            },
        ];
        for el in cases {
            for dt in [600.0, 2500.0, -1200.0] {
                let by_elements = el.at_time(MU, dt).state(MU);
                let by_state = propagate(el.state(MU), MU, dt);
                assert!(close(by_elements.pos, by_state.pos, 1e-6), "{el:?} dt {dt}");
                assert!(close(by_elements.vel, by_state.vel, 1e-6), "{el:?} dt {dt}");
            }
        }
    }

    #[test]
    fn serde_round_trip() {
        let el = Elements::circular(5.0);
        let json = serde_json::to_string(&el).unwrap();
        assert_eq!(serde_json::from_str::<Elements>(&json).unwrap(), el);
    }
}
