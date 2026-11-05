//! The listener, distance attenuation, source cones and Doppler.

use crate::error::{Error, Result};
use std::f32::consts::TAU;
use yowie_core::{Transform, Vec3};

/// The point of view audio is heard from.
///
/// Positions are in world units; velocities are in world units per second.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Listener {
    /// Listener position.
    pub position: Vec3,
    /// Facing direction. Need not be normalised; zero is replaced with -Z.
    pub forward: Vec3,
    /// Head-up direction. Need not be normalised; zero is replaced with +Y.
    pub up: Vec3,
    /// Velocity, used for Doppler.
    pub velocity: Vec3,
}

impl Default for Listener {
    fn default() -> Self {
        Self {
            position: Vec3::ZERO,
            forward: Vec3::NEG_Z,
            up: Vec3::Y,
            velocity: Vec3::ZERO,
        }
    }
}

impl Listener {
    /// A listener at the transform's position facing along its forward axis.
    pub fn from_transform(t: &Transform) -> Self {
        Self {
            position: t.translation,
            forward: t.forward(),
            up: t.rotation * Vec3::Y,
            velocity: Vec3::ZERO,
        }
    }

    /// A listener for a 2D game on the XY plane, y down, facing into the screen.
    pub fn new_2d(x: f32, y: f32) -> Self {
        Self {
            position: Vec3::new(x, y, 0.0),
            forward: Vec3::Z,
            up: Vec3::NEG_Y,
            velocity: Vec3::ZERO,
        }
    }

    pub(crate) fn sanitized(mut self) -> Self {
        if !self.forward.is_finite() || self.forward.length() == 0.0 {
            self.forward = Vec3::NEG_Z;
        }
        if !self.up.is_finite() || self.up.length() == 0.0 {
            self.up = Vec3::Y;
        }
        if !self.position.is_finite() {
            self.position = Vec3::ZERO;
        }
        if !self.velocity.is_finite() {
            self.velocity = Vec3::ZERO;
        }
        self
    }

    /// The listener's right-hand unit vector.
    pub(crate) fn right(&self) -> Vec3 {
        self.forward.cross(self.up).normalize_or_zero()
    }

    /// Maps a listener-local vector (x right, y up, -z forward) into the world.
    pub(crate) fn local_vector(&self, p: Vec3) -> Vec3 {
        let f = self.forward.normalize_or_zero();
        let r = f.cross(self.up).normalize_or_zero();
        let u = r.cross(f).normalize_or_zero();
        r * p.x + u * p.y - f * p.z
    }

    /// Distance gain parameters: the pan position in `-1..=1` for a source at `p`.
    pub(crate) fn pan_for(&self, p: Vec3, min_dist: f32) -> f32 {
        let d = p - self.position;
        let dist = d.length();
        if dist <= 1e-6 {
            return 0.0;
        }
        // Never fully silence an ear.
        let pan = (d / dist).dot(self.right()) * 0.8;
        if dist < min_dist {
            pan * dist / min_dist
        } else {
            pan
        }
    }

    /// The pitch factor Doppler shift applies to a source at `p` moving at `vel`.
    pub(crate) fn doppler(&self, p: Vec3, vel: Vec3, factor: f32, c: f32) -> f32 {
        let d = p - self.position;
        let dist = d.length();
        if dist <= 1e-6 || c <= 0.0 || factor <= 0.0 {
            return 1.0;
        }
        let dir = d / dist;
        const LIMIT: f32 = 0.9;
        let vl = (self.velocity.dot(dir) * factor).max(-c * LIMIT);
        let vs = (-vel.dot(dir) * factor).min(c * LIMIT);
        (c + vl) / (c - vs)
    }
}

/// How positional voices reach the ears.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SpatialSettings {
    /// Render positional voices through the binaural head model instead of stereo panning.
    pub binaural: bool,
    /// Head radius in metres for the binaural model; zero or less means 0.0875.
    pub head_radius: f32,
}

/// A directional source: full volume inside the inner angle, `outer_gain` outside the outer.
///
/// Angles are full cone angles in radians, `0 <= inner <= outer <= 2*PI`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cone {
    inner: f32,
    outer: f32,
    outer_gain: f32,
}

impl Cone {
    /// A cone that is omnidirectional.
    pub const OMNI: Self = Self {
        inner: TAU,
        outer: TAU,
        outer_gain: 1.0,
    };

    /// Makes a cone, or fails when the angles or gain are out of range.
    pub fn new(inner: f32, outer: f32, outer_gain: f32) -> Result<Self> {
        let ok = inner.is_finite()
            && outer.is_finite()
            && outer_gain.is_finite()
            && inner >= 0.0
            && outer >= inner
            && outer <= TAU
            && (0.0..=1.0).contains(&outer_gain);
        if ok {
            Ok(Self {
                inner,
                outer,
                outer_gain,
            })
        } else {
            Err(Error::Invalid("source cone"))
        }
    }

    /// Gain for a listener at `toward` (from the source) given the source `direction`.
    pub(crate) fn gain(&self, direction: Vec3, toward: Vec3) -> f32 {
        if self.inner >= TAU || toward.length() <= 1e-6 {
            return 1.0;
        }
        let cos = direction
            .normalize_or_zero()
            .dot(toward.normalize_or_zero())
            .clamp(-1.0, 1.0);
        let angle = cos.acos() * 2.0;
        if angle <= self.inner {
            1.0
        } else if angle >= self.outer {
            self.outer_gain
        } else {
            1.0 + (self.outer_gain - 1.0) * (angle - self.inner) / (self.outer - self.inner)
        }
    }
}

impl Default for Cone {
    fn default() -> Self {
        Self::OMNI
    }
}

/// How distance reduces a positional voice's volume.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AttenuationModel {
    /// Inverse distance times a linear cutoff to silence at the maximum distance.
    #[default]
    Standard,
    /// Distance does not affect gain.
    None,
    /// Linear falloff from the minimum to the maximum distance.
    Linear,
    /// Inverse-distance falloff.
    Inverse,
    /// Power-law falloff.
    Exponential,
}

/// A distance attenuation model with its rolloff factor.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Attenuation {
    /// The falloff curve.
    pub model: AttenuationModel,
    /// Steepness of the curve; 1 is the reference rate. Must be finite and not negative.
    pub rolloff: f32,
}

impl Default for Attenuation {
    fn default() -> Self {
        Self {
            model: AttenuationModel::Standard,
            rolloff: 1.0,
        }
    }
}

impl Attenuation {
    /// Makes an attenuation, or fails when the rolloff is negative or not finite.
    pub fn new(model: AttenuationModel, rolloff: f32) -> Result<Self> {
        if rolloff.is_finite() && rolloff >= 0.0 {
            Ok(Self {
                model,
                rolloff: if rolloff == 0.0 { 1.0 } else { rolloff },
            })
        } else {
            Err(Error::Invalid("attenuation rolloff"))
        }
    }

    /// Gain at `dist` for a voice with the given distance range.
    pub(crate) fn gain(&self, dist: f32, min_d: f32, max_d: f32) -> f32 {
        if self.model == AttenuationModel::None || dist <= min_d {
            return 1.0;
        }
        if dist >= max_d {
            return 0.0;
        }
        let x = (dist - min_d) / (max_d - min_d);
        let r = self.rolloff;
        match self.model {
            AttenuationModel::Linear => (1.0 - r * x).max(0.0),
            AttenuationModel::Inverse => min_d / (min_d + r * (dist - min_d)),
            AttenuationModel::Exponential => (dist / min_d).powf(-r),
            _ => (min_d / (min_d + r * (dist - min_d))) * (1.0 - x),
        }
    }
}

/// Gain kept by an occluded voice: 0 leaves it alone, 1 drops it 20 dB.
pub(crate) fn occlusion_gain(o: f32) -> f32 {
    10f32.powf(-o)
}

/// Low-pass cutoff for an occlusion amount: 20 kHz open, 400 Hz fully blocked.
pub(crate) fn occlusion_cutoff(o: f32) -> f32 {
    20000.0 * 0.02f32.powf(o)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attenuation_table() {
        let std = Attenuation::default();
        let cases = [
            (std, 0.5, 1.0),
            (std, 1.0, 1.0),
            (std, 100.0, 0.0),
            (
                Attenuation::new(AttenuationModel::None, 1.0).unwrap(),
                50.0,
                1.0,
            ),
            (
                Attenuation::new(AttenuationModel::Linear, 1.0).unwrap(),
                50.5,
                0.5,
            ),
            (
                Attenuation::new(AttenuationModel::Inverse, 1.0).unwrap(),
                2.0,
                0.5,
            ),
            (
                Attenuation::new(AttenuationModel::Exponential, 2.0).unwrap(),
                2.0,
                0.25,
            ),
        ];
        for (a, dist, want) in cases {
            let got = a.gain(dist, 1.0, 100.0);
            assert!(
                (got - want).abs() < 0.02,
                "{a:?} at {dist}: {got} want {want}"
            );
        }
    }

    #[test]
    fn cone_and_attenuation_validate() {
        assert!(Cone::new(1.0, 0.5, 0.5).is_err());
        assert!(Cone::new(0.5, 1.0, 1.5).is_err());
        assert!(Cone::new(0.5, 1.0, 0.2).is_ok());
        assert!(Attenuation::new(AttenuationModel::Linear, -1.0).is_err());
    }

    #[test]
    fn cone_gain_falls_off_axis() {
        let c = Cone::new(0.5, 2.0, 0.25).unwrap();
        let on_axis = c.gain(Vec3::NEG_Z, Vec3::NEG_Z);
        let behind = c.gain(Vec3::NEG_Z, Vec3::Z);
        assert_eq!(on_axis, 1.0);
        assert_eq!(behind, 0.25);
    }

    #[test]
    fn doppler_rises_when_approaching() {
        let l = Listener::default();
        let p = Vec3::new(0.0, 0.0, -10.0);
        let approaching = l.doppler(p, Vec3::new(0.0, 0.0, 30.0), 1.0, 343.0);
        let receding = l.doppler(p, Vec3::new(0.0, 0.0, -30.0), 1.0, 343.0);
        assert!(approaching > 1.0 && receding < 1.0);
    }

    #[test]
    fn pan_follows_side() {
        let l = Listener::default();
        assert!(l.pan_for(Vec3::new(5.0, 0.0, 0.0), 1.0) > 0.5);
        assert!(l.pan_for(Vec3::new(-5.0, 0.0, 0.0), 1.0) < -0.5);
        assert!(l.pan_for(Vec3::new(0.0, 0.0, -5.0), 1.0).abs() < 1e-6);
    }
}
