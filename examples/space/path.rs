//! The ship's predicted path, kept between frames.
//!
//! Predicting integrates the ship ahead under the gravity of every body,
//! which costs more than drawing a frame, and the path barely changes from
//! one frame to the next. It is recomputed when the ship has moved one
//! sample along it or an input to the prediction changed, and between
//! recomputations it is moved with the body it is drawn around.

use yowie::orbit::{BodyId, DVec3, Elements, ShipId, System};

/// Samples in the path.
const SAMPLES: usize = 90;

/// What the dominant body and the ship's orbit around it say about the
/// path to draw.
pub struct Around {
    /// The body that dominates the ship's gravity.
    pub primary: BodyId,
    /// The ship's orbit around it.
    pub elements: Elements,
    /// The primary's gravitational parameter.
    pub mu: f64,
}

impl Around {
    /// Reads the ship's dominant body from the system.
    pub fn of(sys: &System, ship: ShipId) -> Option<Around> {
        let state = sys.ship(ship)?.state;
        let (primary, elements, mu) = sys.around(state)?;
        Some(Around {
            primary,
            elements,
            mu,
        })
    }

    /// How far ahead to predict: a lap and a half of a closed orbit, up to
    /// ten minutes, and a minute for an escape.
    pub fn horizon(&self) -> f64 {
        match self.elements.period(self.mu) {
            Some(p) => (1.5 * p).min(600.0),
            None => 60.0,
        }
    }
}

/// The cached path and the inputs it was computed from.
#[derive(Default)]
pub struct PathCache {
    points: Vec<DVec3>,
    at: f64,
    primary: Option<BodyId>,
    anchor: DVec3,
    thrust: DVec3,
    warp: f32,
    horizon: f64,
}

impl PathCache {
    /// Returns the path in the frame of `around.primary`, moved to where
    /// that body is now. Positions are in system coordinates.
    pub fn get(&mut self, sys: &System, ship: ShipId, around: &Around, warp: f32) -> Vec<DVec3> {
        let horizon = around.horizon();
        let thrust = sys.ship(ship).map_or(DVec3::ZERO, |s| s.thrust);
        let anchor = sys
            .body_state(around.primary)
            .map_or(DVec3::ZERO, |s| s.pos);
        let due = self.points.is_empty()
            || self.primary != Some(around.primary)
            || thrust != self.thrust
            || warp != self.warp
            || (horizon - self.horizon).abs() > 0.05 * self.horizon
            || sys.time() - self.at >= self.horizon / SAMPLES as f64;
        if due {
            self.points = sys
                .predict(ship, horizon, SAMPLES, Some(around.primary))
                .unwrap_or_default();
            self.at = sys.time();
            self.primary = Some(around.primary);
            self.anchor = anchor;
            self.thrust = thrust;
            self.warp = warp;
            self.horizon = horizon;
        }
        let shift = anchor - self.anchor;
        self.points.iter().map(|p| *p + shift).collect()
    }
}
