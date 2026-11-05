use yowie_core::glam::DVec3;

use crate::system::Scratch;
use crate::{BodyId, Elements, OrbitError, ShipId, State, System};

impl System {
    /// Predicts the path of a ship over the next `duration` time units and
    /// returns its position at `samples` equally spaced times, the last at
    /// `duration`. The ship's current thrust is assumed to continue.
    ///
    /// With a `reference` body the path is drawn in that body's frame: each
    /// sample has the body's displacement since now subtracted, so a path
    /// around a moving planet closes into an orbit instead of a spiral.
    pub fn predict(
        &self,
        ship: ShipId,
        duration: f64,
        samples: usize,
        reference: Option<BodyId>,
    ) -> Result<Vec<DVec3>, OrbitError> {
        let ship = self.ship(ship).ok_or(OrbitError::UnknownShip)?;
        self.predict_state(ship.state, ship.thrust, duration, samples, reference)
    }

    /// Like [`System::predict`] for any state and constant thrust, such as
    /// the ship after a planned burn.
    pub fn predict_state(
        &self,
        state: State,
        thrust: DVec3,
        duration: f64,
        samples: usize,
        reference: Option<BodyId>,
    ) -> Result<Vec<DVec3>, OrbitError> {
        if let Some(r) = reference
            && self.body_state(r).is_none()
        {
            return Err(OrbitError::UnknownBody);
        }
        let mut out = Vec::with_capacity(samples);
        if samples == 0 || duration.is_nan() || duration <= 0.0 {
            return Ok(out);
        }
        let mut scratch = Scratch::default();
        let mut states = Vec::new();
        let ref_now = reference
            .and_then(|r| self.body_state(r))
            .map_or(DVec3::ZERO, |s| s.pos);
        let h = duration / samples as f64;
        let mut cur = state;
        for i in 0..samples {
            let (t0, t1) = (i as f64 * h, (i + 1) as f64 * h);
            cur = self.fly(cur, thrust, (t0, t1), 1, &mut scratch);
            let shift = match reference {
                Some(r) => {
                    self.states_at(t1, &mut states);
                    states.get(r.0).map_or(DVec3::ZERO, |s| s.pos - ref_now)
                }
                None => DVec3::ZERO,
            };
            out.push(cur.pos - shift);
        }
        Ok(out)
    }

    /// Finds the body whose gravity dominates at `state` (the largest
    /// `mass / distance²`) and returns it with the orbit of `state` around
    /// it and that body's gravitational parameter. Returns `None` when no
    /// body has mass.
    pub fn around(&self, state: State) -> Option<(BodyId, Elements, f64)> {
        let (id, body) = self
            .body_ids()
            .filter_map(|id| Some((id, self.body_state(id)?, self.body_mass(id)?)))
            .filter(|&(_, s, m)| m > 0.0 && (s.pos - state.pos).length() > 0.0)
            .map(|(id, s, m)| (id, s, m / (s.pos - state.pos).length_squared()))
            .max_by(|a, b| a.2.total_cmp(&b.2))
            .map(|(id, s, _)| (id, s))?;
        let mu = self.g * self.body_mass(id)?;
        let rel = State {
            pos: state.pos - body.pos,
            vel: state.vel - body.vel,
        };
        Some((id, Elements::from_state(rel, mu), mu))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sol;
    use std::f64::consts::TAU;

    #[test]
    fn predicted_path_matches_the_exact_orbit() {
        let mut sys = System::new(1.0);
        sys.add_fixed(1.0, State::default());
        let el = Elements {
            semi_major_axis: 1.0,
            eccentricity: 0.3,
            inclination: 0.4,
            node: 0.5,
            arg_periapsis: 1.0,
            true_anomaly: 0.2,
        };
        let ship = sys.add_ship(el.state(1.0));
        let path = sys.predict(ship, TAU, 16, None).unwrap_or_default();
        assert_eq!(path.len(), 16);
        for (i, p) in path.iter().enumerate() {
            let want = el.at_time(1.0, TAU * (i + 1) as f64 / 16.0).state(1.0).pos;
            assert!(
                (*p - want).length() < 1e-5,
                "sample {i}: {}",
                (*p - want).length()
            );
        }
        // Predicting does not move the ship.
        assert_eq!(sys.ship(ship).map(|s| s.state), Some(el.state(1.0)));
    }

    #[test]
    fn reference_frame_closes_an_orbit_around_a_moving_planet() {
        let mut sys = System::default();
        let sun = sys.add_fixed(sol::SUN.mass, State::default());
        let earth_orbit = sol::EARTH.orbit.map(|o| o.elements()).unwrap_or_default();
        let earth = sys
            .add_orbiting(sol::EARTH.mass, sun, earth_orbit)
            .unwrap_or(sun);
        let e = sys.body_state(earth).unwrap_or_default();
        let r = sol::EARTH.radius + 4.0e5;
        let v = (mu_earth() / r).sqrt();
        let ship = sys.add_ship(State {
            pos: e.pos + DVec3::new(r, 0.0, 0.0),
            vel: e.vel + DVec3::new(0.0, v, 0.0),
        });
        let period = TAU * r / v;
        let path = sys
            .predict(ship, period, 40, Some(earth))
            .unwrap_or_default();
        let radii: Vec<f64> = path.iter().map(|p| (*p - e.pos).length()).collect();
        let (lo, hi) = radii
            .iter()
            .fold((f64::MAX, 0.0f64), |(l, h), &x| (l.min(x), h.max(x)));
        assert!(hi / lo < 1.01, "radius varied from {lo} to {hi}");
        assert!((path[39] - (e.pos + DVec3::new(r, 0.0, 0.0))).length() < 0.02 * r);
        assert!(sys.predict(ship, 1.0, 4, Some(BodyId(99))).is_err());
        assert!(sys.predict(ShipId(9), 1.0, 4, None).is_err());
    }

    fn mu_earth() -> f64 {
        crate::G * sol::EARTH.mass
    }

    #[test]
    fn degenerate_requests_give_empty_paths() {
        let sys = System::default();
        let s = State::default();
        for (d, n) in [(0.0, 5), (-1.0, 5), (1.0, 0), (f64::NAN, 5)] {
            assert_eq!(
                sys.predict_state(s, DVec3::ZERO, d, n, None),
                Ok(Vec::new())
            );
        }
    }

    #[test]
    fn around_picks_the_dominant_body() {
        let mut sys = System::default();
        let sun = sys.add_fixed(sol::SUN.mass, State::default());
        let earth_orbit = sol::EARTH.orbit.map(|o| o.elements()).unwrap_or_default();
        let earth = sys
            .add_orbiting(sol::EARTH.mass, sun, earth_orbit)
            .unwrap_or(sun);
        let e = sys.body_state(earth).unwrap_or_default();
        let r = 7.0e6;
        let v = (mu_earth() / r).sqrt();
        let near = State {
            pos: e.pos + DVec3::new(r, 0.0, 0.0),
            vel: e.vel + DVec3::new(0.0, v, 0.0),
        };
        let (id, el, mu) = sys.around(near).unwrap();
        assert_eq!(id, earth);
        assert!((mu / mu_earth() - 1.0).abs() < 1e-12);
        assert!(el.eccentricity < 1e-6 && (el.semi_major_axis / r - 1.0).abs() < 1e-6);
        let far = State {
            pos: e.pos * 0.5,
            vel: DVec3::ZERO,
        };
        assert_eq!(sys.around(far).map(|a| a.0), Some(sun));
        assert!(System::default().around(far).is_none());
    }
}
