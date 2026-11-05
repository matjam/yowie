use serde::{Deserialize, Serialize};
use yowie_core::glam::DVec3;

use crate::nbody::{Body, field_at};
use crate::{Elements, G, OrbitError, State};

/// Identifies a body in a [`System`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct BodyId(pub(crate) usize);

/// Identifies a ship in a [`System`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ShipId(pub(crate) usize);

/// How a body moves.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Motion {
    /// Moves in a straight line at constant velocity from this state,
    /// which is its state now: a star, or anything whose path the game
    /// decides itself. Change it with [`System::set_fixed_state`].
    Fixed(State),
    /// Follows a Keplerian orbit around another body.
    Rails {
        /// The body it orbits, or `None` for the origin.
        primary: Option<BodyId>,
        /// The orbit at time 0 of the system.
        elements: Elements,
        /// The gravitational parameter of the primary, used for the orbit.
        mu: f64,
    },
}

/// A spacecraft: it feels the gravity of every massive body in the system
/// and its own thrust, and exerts no gravity.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Ship {
    /// Position and velocity in the system's frame.
    pub state: State,
    /// Thrust acceleration, in the system's frame, applied for as long as
    /// it is non-zero. Not mass-dependent: set the acceleration the engine
    /// gives.
    pub thrust: DVec3,
}

#[derive(Clone, Debug)]
struct Entry {
    mass: f64,
    motion: Motion,
    state: State,
}

/// Bodies on rails plus ships that fly under their gravity.
///
/// Planets and moons follow exact Keplerian orbits, so they are cheap and
/// never drift however long a game runs or however much time is
/// accelerated. Ships are integrated numerically against the gravity of all
/// bodies that have mass, so they can be captured, slingshot, and respond
/// to thrust, which Kepler's equation cannot do.
#[derive(Clone, Debug)]
pub struct System {
    /// The gravitational constant. Defaults to [`G`].
    pub g: f64,
    /// Softening distance for ship gravity; see [`crate::field_at`].
    pub softening: f64,
    /// The least number of integration steps per [`System::step`] for each
    /// ship. More steps are taken automatically when a ship moves fast
    /// relative to its acceleration. Defaults to 8.
    pub substeps: u32,
    time: f64,
    bodies: Vec<Entry>,
    ships: Vec<Ship>,
}

impl Default for System {
    fn default() -> Self {
        Self::new(G)
    }
}

/// Reusable working memory for placing bodies at a time.
#[derive(Default)]
pub(crate) struct Scratch {
    states: Vec<State>,
    pub(crate) bodies: Vec<Body>,
}

impl System {
    /// Creates an empty system with gravitational constant `g`.
    pub fn new(g: f64) -> Self {
        Self {
            g,
            softening: 0.0,
            substeps: 8,
            time: 0.0,
            bodies: Vec::new(),
            ships: Vec::new(),
        }
    }

    /// Time elapsed in the system.
    pub fn time(&self) -> f64 {
        self.time
    }

    /// Adds a body that moves in a straight line at constant velocity.
    pub fn add_fixed(&mut self, mass: f64, state: State) -> BodyId {
        self.push(mass, Motion::Fixed(state), state)
    }

    /// Adds a body on a Keplerian orbit around `primary`. `elements` are the
    /// orbit at time 0 of the system. The orbit uses `g * primary mass` as
    /// its gravitational parameter, read now.
    ///
    /// Returns [`OrbitError::UnknownBody`] when `primary` is not in this
    /// system. Add primaries before their moons.
    pub fn add_orbiting(
        &mut self,
        mass: f64,
        primary: BodyId,
        elements: Elements,
    ) -> Result<BodyId, OrbitError> {
        let mu = self.g
            * self
                .bodies
                .get(primary.0)
                .ok_or(OrbitError::UnknownBody)?
                .mass;
        let motion = Motion::Rails {
            primary: Some(primary),
            elements,
            mu,
        };
        let id = self.push(mass, motion, State::default());
        self.refresh();
        Ok(id)
    }

    fn push(&mut self, mass: f64, motion: Motion, state: State) -> BodyId {
        self.bodies.push(Entry {
            mass,
            motion,
            state,
        });
        BodyId(self.bodies.len() - 1)
    }

    /// Replaces the state of a body added with [`System::add_fixed`].
    /// Bodies on rails are unaffected and return false.
    pub fn set_fixed_state(&mut self, id: BodyId, state: State) -> bool {
        match self.bodies.get_mut(id.0) {
            Some(e) if matches!(e.motion, Motion::Fixed(_)) => {
                e.motion = Motion::Fixed(state);
                e.state = state;
                self.refresh();
                true
            }
            _ => false,
        }
    }

    /// The mass of a body.
    pub fn body_mass(&self, id: BodyId) -> Option<f64> {
        self.bodies.get(id.0).map(|e| e.mass)
    }

    /// The current position and velocity of a body.
    pub fn body_state(&self, id: BodyId) -> Option<State> {
        self.bodies.get(id.0).map(|e| e.state)
    }

    /// The ids of all bodies, in the order they were added.
    pub fn body_ids(&self) -> impl Iterator<Item = BodyId> + use<> {
        (0..self.bodies.len()).map(BodyId)
    }

    /// Adds a ship with the given state and no thrust.
    pub fn add_ship(&mut self, state: State) -> ShipId {
        self.ships.push(Ship {
            state,
            thrust: DVec3::ZERO,
        });
        ShipId(self.ships.len() - 1)
    }

    /// Returns a ship.
    pub fn ship(&self, id: ShipId) -> Option<&Ship> {
        self.ships.get(id.0)
    }

    /// Returns a ship mutably, to set its state or thrust.
    pub fn ship_mut(&mut self, id: ShipId) -> Option<&mut Ship> {
        self.ships.get_mut(id.0)
    }

    /// Sets a ship's thrust acceleration.
    pub fn set_thrust(&mut self, id: ShipId, accel: DVec3) -> Result<(), OrbitError> {
        self.ship_mut(id)
            .map(|s| s.thrust = accel)
            .ok_or(OrbitError::UnknownShip)
    }

    /// Fills `out` with the absolute state of every body `ahead` time units
    /// from now (negative looks back).
    pub(crate) fn states_at(&self, ahead: f64, out: &mut Vec<State>) {
        out.clear();
        let t = self.time + ahead;
        for e in &self.bodies {
            let st = match e.motion {
                Motion::Fixed(s) => State {
                    pos: s.pos + s.vel * ahead,
                    vel: s.vel,
                },
                Motion::Rails {
                    primary,
                    elements,
                    mu,
                } => {
                    let local = elements.at_time(mu, t).state(mu);
                    let base = primary
                        .and_then(|p| out.get(p.0).copied())
                        .unwrap_or_default();
                    State {
                        pos: base.pos + local.pos,
                        vel: base.vel + local.vel,
                    }
                }
            };
            out.push(st);
        }
    }

    /// Places the massive bodies `ahead` time units from now into `s`.
    pub(crate) fn place(&self, ahead: f64, s: &mut Scratch) {
        self.states_at(ahead, &mut s.states);
        s.bodies.clear();
        s.bodies.extend(
            self.bodies
                .iter()
                .zip(&s.states)
                .filter(|(e, _)| e.mass > 0.0)
                .map(|(e, st)| Body {
                    pos: st.pos,
                    vel: st.vel,
                    mass: e.mass,
                }),
        );
    }

    fn refresh(&mut self) {
        let mut states = Vec::with_capacity(self.bodies.len());
        self.states_at(0.0, &mut states);
        for (e, s) in self.bodies.iter_mut().zip(states) {
            e.state = s;
        }
    }

    /// Flies a test particle from time offset `t0` to `t1` (relative to the
    /// current time) under gravity and `thrust`.
    pub(crate) fn fly(
        &self,
        state: State,
        thrust: DVec3,
        (t0, t1): (f64, f64),
        min_steps: u32,
        s: &mut Scratch,
    ) -> State {
        let span = t1 - t0;
        let (mut pos, mut vel) = (state.pos, state.vel);
        if span.is_nan() || span <= 0.0 {
            return state;
        }
        let max_step = span / f64::from(min_steps.max(1));
        let done = 1e-12 * span;
        let mut t = t0;
        while t1 - t > done {
            let mut step = (t1 - t).min(max_step);
            self.place(t, s);
            let a = (field_at(&s.bodies, self.g, self.softening, pos) + thrust).length();
            let v = vel.length();
            if a > 0.0 && v > 0.0 {
                step = step.min((0.01 * v / a).max(max_step / 64.0));
            }
            // Fourth-order Runge-Kutta with the bodies placed at each
            // stage's time, so fast-moving sources are not frozen mid-step.
            let h = step;
            let accel = |ahead: f64, p: DVec3, s: &mut Scratch| {
                self.place(ahead, s);
                field_at(&s.bodies, self.g, self.softening, p) + thrust
            };
            let k1v = accel(t, pos, s);
            let k2p = vel + k1v * (h / 2.0);
            let k2v = accel(t + h / 2.0, pos + vel * (h / 2.0), s);
            let k3p = vel + k2v * (h / 2.0);
            let k3v = accel(t + h / 2.0, pos + k2p * (h / 2.0), s);
            let k4p = vel + k3v * h;
            let k4v = accel(t + h, pos + k3p * h, s);
            pos += (vel + k2p * 2.0 + k3p * 2.0 + k4p) * (h / 6.0);
            vel += (k1v + k2v * 2.0 + k3v * 2.0 + k4v) * (h / 6.0);
            t += step;
        }
        State { pos, vel }
    }

    /// Advances the system by `dt`: ships fly under gravity and thrust, then
    /// bodies on rails move to their new places. Non-positive or non-finite
    /// `dt` is ignored.
    pub fn step(&mut self, dt: f64) {
        if !(dt > 0.0 && dt.is_finite()) {
            return;
        }
        let mut scratch = Scratch::default();
        let ships: Vec<State> = self
            .ships
            .iter()
            .map(|s| self.fly(s.state, s.thrust, (0.0, dt), self.substeps, &mut scratch))
            .collect();
        for (ship, st) in self.ships.iter_mut().zip(ships) {
            ship.state = st;
        }
        self.time += dt;
        for e in &mut self.bodies {
            if let Motion::Fixed(s) = &mut e.motion {
                s.pos += s.vel * dt;
            }
        }
        self.refresh();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sol;
    use std::f64::consts::TAU;

    fn sun_and_earth() -> (System, BodyId, BodyId) {
        let mut sys = System::default();
        let sun = sys.add_fixed(sol::SUN.mass, State::default());
        let earth_orbit = sol::EARTH.orbit.map(|o| o.elements()).unwrap_or_default();
        let earth = sys
            .add_orbiting(sol::EARTH.mass, sun, earth_orbit)
            .unwrap_or(sun);
        (sys, sun, earth)
    }

    #[test]
    fn earth_on_rails_returns_after_a_year() {
        let (mut sys, _, earth) = sun_and_earth();
        let start = sys.body_state(earth).unwrap_or_default();
        let a = sol::EARTH.orbit.map_or(0.0, |o| o.semi_major_axis);
        assert!((start.pos.length() / a - 1.0).abs() < 0.02);
        let period = TAU * (a.powi(3) / (G * sol::SUN.mass)).sqrt();
        assert!((period / sol::YEAR - 1.0).abs() < 1e-3);
        for _ in 0..365 {
            sys.step(period / 365.0);
        }
        let end = sys.body_state(earth).unwrap_or_default();
        assert!((end.pos - start.pos).length() < 1e-9 * a);
        assert!((sys.time() - period).abs() < 1.0);
    }

    #[test]
    fn moons_ride_on_their_planets() {
        let (mut sys, _, earth) = sun_and_earth();
        let moon_orbit = sol::MOON.orbit.map(|o| o.elements()).unwrap_or_default();
        let moon = sys
            .add_orbiting(sol::MOON.mass, earth, moon_orbit)
            .unwrap_or(earth);
        sys.step(3.0 * sol::DAY);
        let e = sys.body_state(earth).unwrap_or_default();
        let m = sys.body_state(moon).unwrap_or_default();
        let d = (m.pos - e.pos).length();
        assert!((3.5e8..4.1e8).contains(&d), "{d}");
        // The relative speed is about 1 km/s.
        assert!(((m.vel - e.vel).length() - 1000.0).abs() < 100.0);
    }

    #[test]
    fn unknown_ids_are_errors() {
        let mut sys = System::default();
        assert_eq!(
            sys.add_orbiting(1.0, BodyId(3), Elements::circular(1.0)),
            Err(OrbitError::UnknownBody)
        );
        assert_eq!(
            sys.set_thrust(ShipId(0), DVec3::X),
            Err(OrbitError::UnknownShip)
        );
        assert!(!sys.set_fixed_state(BodyId(0), State::default()));
        assert!(sys.body_state(BodyId(0)).is_none());
    }

    #[test]
    fn ship_in_circular_orbit_closes_its_loop() {
        let mut sys = System::new(1.0);
        sys.add_fixed(1.0, State::default());
        let r = 1.0;
        let ship = sys.add_ship(State {
            pos: DVec3::new(r, 0.0, 0.0),
            vel: DVec3::new(0.0, 1.0, 0.0),
        });
        let l0 = sys
            .ship(ship)
            .map(|s| s.state.pos.cross(s.state.vel))
            .unwrap_or_default();
        for _ in 0..100 {
            sys.step(TAU / 100.0);
        }
        let s = sys.ship(ship).map(|s| s.state).unwrap_or_default();
        assert!((s.pos - DVec3::X).length() < 1e-5, "{:?}", s.pos);
        assert!((s.pos.length() - r).abs() < 1e-6);
        assert!((s.pos.cross(s.vel) - l0).length() < 1e-6);
    }

    #[test]
    fn thrust_in_empty_space_is_uniform_acceleration() {
        let mut sys = System::new(G);
        let ship = sys.add_ship(State {
            pos: DVec3::ZERO,
            vel: DVec3::new(10.0, 0.0, 0.0),
        });
        assert!(sys.set_thrust(ship, DVec3::new(0.0, 2.0, 0.0)).is_ok());
        sys.step(3.0);
        let s = sys.ship(ship).map(|s| s.state).unwrap_or_default();
        assert!((s.pos - DVec3::new(30.0, 9.0, 0.0)).length() < 1e-9);
        assert!((s.vel - DVec3::new(10.0, 6.0, 0.0)).length() < 1e-9);
        sys.step(-1.0);
        sys.step(f64::NAN);
        assert_eq!(sys.time(), 3.0);
    }

    #[test]
    fn moving_fixed_body_drifts_and_can_be_reset() {
        let mut sys = System::new(G);
        let star = sys.add_fixed(
            0.0,
            State {
                pos: DVec3::ZERO,
                vel: DVec3::X,
            },
        );
        sys.step(5.0);
        assert_eq!(
            sys.body_state(star).map(|s| s.pos),
            Some(DVec3::new(5.0, 0.0, 0.0))
        );
        assert!(sys.set_fixed_state(
            star,
            State {
                pos: DVec3::Y,
                vel: DVec3::ZERO
            }
        ));
        assert_eq!(sys.body_state(star).map(|s| s.pos), Some(DVec3::Y));
        assert_eq!(sys.body_ids().count(), 1);
    }
}
