//! Celestial mechanics for any star system: orbital elements, exact
//! two-body propagation, N-body integration, ships under thrust, and
//! real-world constants in [`sol`].
//!
//! Everything is `f64`, in whatever units the caller picks, as long as they
//! agree: the gravitational constant `g`, masses, distances and times must
//! be consistent. The [`sol`] module uses metres, kilograms and seconds.
//! Angles are radians. The reference plane is XY with +Z as the north pole;
//! elements are measured from +X, the direction of the reference longitude.
//!
//! - [`Elements`] describe an orbit and convert to and from a [`State`]
//!   (position and velocity).
//! - [`propagate`] advances a state exactly along its two-body orbit, for
//!   ellipses, parabolas and hyperbolas alike.
//! - [`Simulation`] integrates many bodies with a leapfrog scheme.
//! - [`System`] mixes bodies on fixed Keplerian rails with ships that feel
//!   their gravity and their own thrust, and can [predict] a ship's path.
//!
//! [predict]: System::predict
//!
//! ```
//! use yowie_orbit::{Elements, sol};
//!
//! // Earth's period around the Sun, from its orbit's semi-major axis.
//! let earth = Elements { semi_major_axis: sol::AU, ..Elements::default() };
//! let year = earth.period(sol::MU_SUN).unwrap();
//! assert!((year / sol::YEAR - 1.0).abs() < 1e-3);
//! ```

#![warn(missing_docs)]

mod elements;
mod error;
mod kepler;
mod nbody;
mod predict;
mod propagate;
pub mod sol;
mod system;

pub use elements::{Elements, State};
pub use error::OrbitError;
pub use kepler::{
    mean_anomaly_from_true, mean_anomaly_from_true_hyperbolic, solve_kepler,
    solve_kepler_hyperbolic, true_anomaly_from_mean, true_anomaly_from_mean_hyperbolic, wrap_angle,
};
pub use nbody::{Body, Simulation, field_at, rk4};
pub use propagate::{
    Hohmann, circular_velocity, energy, escape_velocity, hill_radius, hohmann, propagate,
    sphere_of_influence, vis_viva,
};
pub use system::{BodyId, Motion, Ship, ShipId, System};
pub use yowie_core::glam::{DQuat, DVec3};

/// The Newtonian gravitational constant in m³ kg⁻¹ s⁻² (CODATA 2018).
pub const G: f64 = 6.674_30e-11;
