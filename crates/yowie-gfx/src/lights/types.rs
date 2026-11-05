//! The light description a game passes to `Graphics::set_lights_2d`.

use yowie_core::{Color, Vec2};

/// The most lights one frame's lit sprites can take.
pub const MAX_LIGHTS_2D: usize = 16;

/// A cone that limits a [`Light2D`] to one direction.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spot2D {
    /// The direction the cone points in, in radians, clockwise on screen
    /// with 0 pointing right.
    pub direction: f32,
    /// The full opening angle in radians.
    pub angle: f32,
    /// How much of the cone's edge fades out, from 0 (a hard edge) to 1
    /// (fading from the axis).
    pub softness: f32,
}

/// A light for lit sprites: a position in drawing coordinates, a height
/// above the sprite plane, a radius where it fades to nothing, a colour and
/// an optional cone.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Light2D {
    /// Where the light is.
    pub position: Vec2,
    /// Distance above the sprite plane in view units. Normal-mapped
    /// surfaces are shaded by the angle to this point, and a flat sprite
    /// is dimmer under a low light.
    pub height: f32,
    /// Distance in view units at which the light has faded to nothing.
    pub radius: f32,
    /// The light's colour, sRGB.
    pub color: Color,
    /// Multiplies the colour; above 1 over-brightens.
    pub intensity: f32,
    /// The exponent of the fade from full strength at the light to zero at
    /// `radius`: 1 is linear, 2 (the default) is softer near the edge.
    pub falloff: f32,
    /// Limits the light to a cone; `None` shines in every direction.
    pub spot: Option<Spot2D>,
    /// Whether occluders block this light.
    pub casts_shadows: bool,
    /// The width of a shadow's soft edge in view units at the shadowed
    /// point.
    pub softness: f32,
}

impl Light2D {
    /// A point light at `position` reaching `radius` view units, 40 units
    /// above the plane, without shadows.
    pub fn point(position: Vec2, radius: f32, color: Color) -> Self {
        Self {
            position,
            height: 40.0,
            radius,
            color,
            intensity: 1.0,
            falloff: 2.0,
            spot: None,
            casts_shadows: false,
            softness: 8.0,
        }
    }

    /// A spot light: a point light limited to a cone.
    pub fn spot(position: Vec2, radius: f32, color: Color, cone: Spot2D) -> Self {
        Self {
            spot: Some(cone),
            ..Self::point(position, radius, color)
        }
    }

    /// Sets the height above the sprite plane.
    pub fn height(self, height: f32) -> Self {
        Self { height, ..self }
    }

    /// Sets the intensity multiplier.
    pub fn intensity(self, intensity: f32) -> Self {
        Self { intensity, ..self }
    }

    /// Sets the falloff exponent.
    pub fn falloff(self, falloff: f32) -> Self {
        Self { falloff, ..self }
    }

    /// Sets whether occluders block the light.
    pub fn shadows(self, casts_shadows: bool) -> Self {
        Self {
            casts_shadows,
            ..self
        }
    }

    /// Sets the width of the shadow's soft edge in view units.
    pub fn softness(self, softness: f32) -> Self {
        Self { softness, ..self }
    }
}
