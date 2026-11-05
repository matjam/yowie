//! Lights: one directional sun per scene, and any number of point and
//! spot lights, which are clustered.

use bytemuck::{Pod, Zeroable};
use yowie_core::{Color, Vec3};

/// The scene's directional light: the sun or the moon.
///
/// `direction` is the way the light travels, so a sun overhead points
/// down. Light colours are sRGB and `intensity` multiplies them in linear
/// light; a sun is usually brighter than 1. With `shadows`, cascaded
/// shadow maps cover `shadow_distance` world units from the camera.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DirectionalLight {
    /// The direction the light travels.
    pub direction: Vec3,
    /// The light's colour, sRGB.
    pub color: Color,
    /// Multiplies the colour.
    pub intensity: f32,
    /// Whether the light casts shadows.
    pub shadows: bool,
    /// How far from the camera shadows reach, in world units. Keep it as
    /// small as the scene allows; the cascades spread their texels over it.
    pub shadow_distance: f32,
    /// How dark shadows are: 1 blocks the light fully, 0 not at all.
    pub shadow_strength: f32,
}

impl Default for DirectionalLight {
    fn default() -> Self {
        Self {
            direction: Vec3::new(-0.4, -1.0, -0.3),
            color: Color::WHITE,
            intensity: 3.0,
            shadows: false,
            shadow_distance: 60.0,
            shadow_strength: 1.0,
        }
    }
}

impl DirectionalLight {
    /// A light travelling along `direction` with `color` at `intensity`.
    pub fn new(direction: Vec3, color: Color, intensity: f32) -> Self {
        Self {
            direction,
            color,
            intensity,
            ..Self::default()
        }
    }

    /// The same light casting shadows out to `distance` world units.
    pub fn with_shadows(self, distance: f32) -> Self {
        Self {
            shadows: true,
            shadow_distance: distance,
            ..self
        }
    }
}

/// A light shining in every direction from a point, fading to nothing at
/// `range` world units. A shadowed point light renders six depth maps, so
/// it is the most expensive light; give shadows to the few that matter.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PointLight {
    /// Where the light is.
    pub position: Vec3,
    /// The light's colour, sRGB.
    pub color: Color,
    /// Multiplies the colour; the radiance at the light itself.
    pub intensity: f32,
    /// Where the light reaches zero, in world units. The smaller the range,
    /// the fewer clusters the light lands in.
    pub range: f32,
    /// Whether the light casts shadows.
    pub shadows: bool,
}

impl PointLight {
    /// A point light at `position`.
    pub fn new(position: Vec3, color: Color, intensity: f32, range: f32) -> Self {
        Self {
            position,
            color,
            intensity,
            range,
            shadows: false,
        }
    }

    /// The same light casting shadows.
    pub fn with_shadows(self) -> Self {
        Self {
            shadows: true,
            ..self
        }
    }
}

/// A light shining in a cone from a point: full inside `inner_angle`,
/// fading to nothing at `outer_angle` (both radians from the axis) and at
/// `range` world units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpotLight {
    /// Where the light is.
    pub position: Vec3,
    /// The direction the cone points.
    pub direction: Vec3,
    /// The light's colour, sRGB.
    pub color: Color,
    /// Multiplies the colour.
    pub intensity: f32,
    /// Where the light reaches zero, in world units.
    pub range: f32,
    /// The half-angle of the full-strength core, radians.
    pub inner_angle: f32,
    /// The half-angle where the light reaches zero, radians.
    pub outer_angle: f32,
    /// Whether the light casts shadows.
    pub shadows: bool,
}

impl SpotLight {
    /// A spot light at `position` pointing along `direction`.
    pub fn new(position: Vec3, direction: Vec3, color: Color, intensity: f32, range: f32) -> Self {
        Self {
            position,
            direction,
            color,
            intensity,
            range,
            inner_angle: 20f32.to_radians(),
            outer_angle: 30f32.to_radians(),
            shadows: false,
        }
    }

    /// The same light with other cone angles in radians.
    pub fn with_cone(self, inner: f32, outer: f32) -> Self {
        Self {
            inner_angle: inner,
            outer_angle: outer,
            ..self
        }
    }

    /// The same light casting shadows.
    pub fn with_shadows(self) -> Self {
        Self {
            shadows: true,
            ..self
        }
    }
}

/// A light record in the frame's light buffer; matches `Light` in
/// `shaders/common.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub(crate) struct GpuLight {
    pub(crate) pos_range: [f32; 4],
    pub(crate) color_kind: [f32; 4],
    pub(crate) dir_cos_outer: [f32; 4],
    pub(crate) params: [f32; 4],
}

/// A point or spot light as the scene records it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum LocalLight {
    Point(PointLight),
    Spot(SpotLight),
}

impl LocalLight {
    pub(crate) fn position(&self) -> Vec3 {
        match self {
            LocalLight::Point(p) => p.position,
            LocalLight::Spot(s) => s.position,
        }
    }

    pub(crate) fn range(&self) -> f32 {
        match self {
            LocalLight::Point(p) => p.range,
            LocalLight::Spot(s) => s.range,
        }
        .max(1e-3)
    }

    pub(crate) fn shadows(&self) -> bool {
        match self {
            LocalLight::Point(p) => p.shadows,
            LocalLight::Spot(s) => s.shadows,
        }
    }

    /// The GPU record, with `shadow_view` the first shadow view or -1.
    pub(crate) fn gpu(&self, shadow_view: i32) -> GpuLight {
        let lin = |c: Color, k: f32| {
            let l = c.to_linear();
            [l[0] * k, l[1] * k, l[2] * k]
        };
        match self {
            LocalLight::Point(p) => {
                let c = lin(p.color, p.intensity);
                GpuLight {
                    pos_range: p.position.extend(self.range()).to_array(),
                    color_kind: [c[0], c[1], c[2], 0.0],
                    dir_cos_outer: [0.0, -1.0, 0.0, -1.0],
                    params: [1.0, shadow_view as f32, 0.0, 0.0],
                }
            }
            LocalLight::Spot(s) => {
                let c = lin(s.color, s.intensity);
                let outer = s.outer_angle.clamp(1e-3, 1.55);
                let inner = s.inner_angle.clamp(0.0, outer);
                GpuLight {
                    pos_range: s.position.extend(self.range()).to_array(),
                    color_kind: [c[0], c[1], c[2], 1.0],
                    dir_cos_outer: s
                        .direction
                        .normalize_or(Vec3::NEG_Y)
                        .extend(outer.cos())
                        .to_array(),
                    params: [inner.cos(), shadow_view as f32, 0.0, 0.0],
                }
            }
        }
    }
}
