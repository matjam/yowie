//! The procedural sky, the atmosphere, fog and the tone curve.

use yowie_core::{Color, Vec3};

use super::frame_data::FrameUniform;

/// A computed sky: single scattering of sunlight by air (Rayleigh) and
/// haze (Mie), blue overhead at noon and red along the horizon at dusk.
///
/// `height` is how deep the air is in world units, and fixes the scale of
/// everything else: a scene a hundred units across under a `height` of 60
/// looks through a lot of air and washes out, while under 3000 it keeps
/// crisp distance. Every other field has an Earth-like default scaled to
/// `height` when left at zero.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Atmosphere {
    /// The depth of the air in world units; must be above zero.
    pub height: f32,
    /// The planet's radius; zero means 100 times `height`.
    pub planet_radius: f32,
    /// The camera's height above the ground, which thins the air as it
    /// climbs; at `height` and above the sky turns black.
    pub altitude: f32,
    /// Rayleigh scattering per world unit at the ground, per channel;
    /// zero means Earth's, scaled to `height`.
    pub rayleigh: Vec3,
    /// Mie scattering per world unit at the ground; zero means Earth's.
    pub mie: f32,
    /// How tightly haze scatters forwards (the glare around the sun), 0 to
    /// 1; zero means 0.76.
    pub mie_forward: f32,
    /// The sunlight the scattering divides up; zero means 22.
    pub intensity: f32,
}

impl Atmosphere {
    /// An Earth-like atmosphere `height` world units deep.
    pub fn new(height: f32) -> Self {
        Self {
            height,
            ..Self::default()
        }
    }

    /// The same atmosphere seen from `altitude` above the ground.
    pub fn at_altitude(self, altitude: f32) -> Self {
        Self { altitude, ..self }
    }

    pub(crate) fn resolved(&self) -> Atmosphere {
        let mut a = *self;
        let k = 60000.0 / a.height.max(1e-3);
        if a.planet_radius <= 0.0 {
            a.planet_radius = 100.0 * a.height;
        }
        if a.rayleigh == Vec3::ZERO {
            a.rayleigh = Vec3::new(5.8e-6, 13.5e-6, 33.1e-6) * k;
        }
        if a.mie <= 0.0 {
            a.mie = 21e-6 * k;
        }
        if a.mie_forward == 0.0 {
            a.mie_forward = 0.76;
        }
        if a.intensity <= 0.0 {
            a.intensity = 22.0;
        }
        a.altitude = a.altitude.max(0.0);
        a
    }

    /// How much of the sunlight survives the air between the camera and
    /// the sun, per channel, which reddens the sun at dusk.
    pub(crate) fn sun_transmittance(&self, up: Vec3, to_sun: Vec3) -> Vec3 {
        let a = self.resolved();
        let (radius, height) = (a.planet_radius, a.height);
        let origin = up * (radius + a.altitude);
        let (h_r, h_m) = (height / 7.5, height / 50.0);
        // The planet blocks the sun when the ray meets it ahead; at ground
        // level the origin sits on the sphere, where a downward ray counts.
        let (g0, g1) = ray_sphere(origin, to_sun, radius);
        if g1 > 0.0 && (g0 > 0.0 || origin.dot(to_sun) < 0.0) {
            return Vec3::ZERO;
        }
        let (_, far) = ray_sphere(origin, to_sun, radius + height);
        let steps = 16;
        let ds = far.max(0.0) / steps as f32;
        let (mut od_r, mut od_m) = (0.0, 0.0);
        for i in 0..steps {
            let p = origin + to_sun * ((i as f32 + 0.5) * ds);
            let h = (p.length() - radius).max(0.0);
            od_r += (-h / h_r).exp() * ds;
            od_m += (-h / h_m).exp() * ds;
        }
        let tau = a.rayleigh * od_r + Vec3::splat(a.mie * 1.1 * od_m);
        Vec3::new((-tau.x).exp(), (-tau.y).exp(), (-tau.z).exp())
    }
}

fn ray_sphere(o: Vec3, d: Vec3, r: f32) -> (f32, f32) {
    let b = o.dot(d);
    let c = o.dot(o) - r * r;
    let h = b * b - c;
    if h < 0.0 {
        return (1.0, -1.0);
    }
    let h = h.sqrt();
    (-b - h, -b + h)
}

/// A procedural sky: lights the scene like an environment map and, unless
/// the scene turns the background off, draws behind it with the sun's
/// disc and stars.
///
/// Without an atmosphere the sky is a gradient from `horizon` to `zenith`
/// above the horizon and towards `ground` below. With one, the computed
/// scattering replaces the gradient above the horizon. Colours are sRGB;
/// `vacuum` thins the air from 0 (full) to 1 (none, a black sky with
/// stars), for a ship climbing to orbit.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sky {
    /// Away from the ground; zero means +Y.
    pub up: Vec3,
    /// The sky straight up.
    pub zenith: Color,
    /// The sky at the horizon.
    pub horizon: Color,
    /// Light from below: terrain, sea, a planet's face.
    pub ground: Color,
    /// How thin the air is, 0 to 1.
    pub vacuum: f32,
    /// The angular radius of the sun's disc in radians; zero hides it.
    pub sun_size: f32,
    /// The disc's brightness as a multiple of the sun light's radiance.
    pub sun_brightness: f32,
    /// The brightness of stars showing through thin air; zero means none.
    pub stars: f32,
    /// Computed scattering in place of the gradient above the horizon.
    pub atmosphere: Option<Atmosphere>,
}

impl Default for Sky {
    fn default() -> Self {
        Self {
            up: Vec3::Y,
            zenith: Color::rgb8(70, 120, 210),
            horizon: Color::rgb8(185, 205, 230),
            ground: Color::rgb8(90, 85, 75),
            vacuum: 0.0,
            sun_size: 0.009,
            sun_brightness: 30.0,
            stars: 0.0,
            atmosphere: None,
        }
    }
}

impl Sky {
    /// A sky computed by `atmosphere`, over `ground`.
    pub fn atmospheric(atmosphere: Atmosphere, ground: Color) -> Self {
        Self {
            ground,
            atmosphere: Some(atmosphere),
            ..Self::default()
        }
    }

    pub(crate) fn up_axis(&self) -> Vec3 {
        self.up.normalize_or(Vec3::Y)
    }

    /// Writes the sky's fields into the frame block, after the sun's.
    /// `sun_radiance` is the sun light's linear colour.
    pub(crate) fn write(&self, f: &mut FrameUniform, sun_radiance: Vec3) {
        let air = 1.0 - self.vacuum.clamp(0.0, 1.0);
        let lin = |c: Color, w: f32| {
            let l = c.to_linear();
            [l[0], l[1], l[2], w]
        };
        let up = self.up_axis();
        f.sky_up = up.extend(self.stars.max(0.0)).to_array();
        f.sky_zenith = lin(self.zenith, 1.0);
        f.sky_horizon = lin(self.horizon, air);
        f.sky_ground = lin(self.ground, 0.0);
        let disc = sun_radiance * self.sun_brightness.max(0.0);
        f.sun_disc = disc.extend(self.sun_size.max(0.0)).to_array();
        if f.sun_dir[3] < 0.5 {
            // Without a sun the disc and scattering use straight up.
            f.sun_dir = [up.x, up.y, up.z, 0.0];
        }
        if let Some(a) = self.atmosphere.filter(|a| a.height > 0.0) {
            let a = a.resolved();
            f.atmos = [a.planet_radius, a.height, a.height / 7.5, a.height / 50.0];
            f.beta_r = a.rayleigh.extend(a.intensity).to_array();
            f.beta_m = [a.mie, a.mie_forward, a.altitude, 1.0];
        }
    }
}

/// Distance and height fog. Linear fog ramps from `start` to full at `end`
/// (when `end` is beyond `start`); exponential fog thickens with
/// `density`; the denser wins. With `height_falloff` above zero the fog is
/// full at and below `height` and thins above it along +Y, for mist in a
/// valley. The sky is not fogged.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fog {
    /// The fog's colour, sRGB.
    pub color: Color,
    /// Where linear fog starts, world units from the camera.
    pub start: f32,
    /// Where linear fog is full.
    pub end: f32,
    /// Exponential density per world unit; zero for none.
    pub density: f32,
    /// The height below which height fog is full.
    pub height: f32,
    /// How fast height fog thins above `height`; zero turns height fog
    /// off.
    pub height_falloff: f32,
}

impl Default for Fog {
    fn default() -> Self {
        Self {
            color: Color::rgb8(180, 190, 200),
            start: 0.0,
            end: 0.0,
            density: 0.0,
            height: 0.0,
            height_falloff: 0.0,
        }
    }
}

impl Fog {
    /// Linear fog from `start` to `end` world units.
    pub fn linear(color: Color, start: f32, end: f32) -> Self {
        Self {
            color,
            start,
            end,
            ..Self::default()
        }
    }

    /// Exponential fog with `density` per world unit.
    pub fn exponential(color: Color, density: f32) -> Self {
        Self {
            color,
            density,
            ..Self::default()
        }
    }

    /// The same fog confined below `height`, thinning above it by
    /// `falloff` per unit.
    pub fn with_height(self, height: f32, falloff: f32) -> Self {
        Self {
            height,
            height_falloff: falloff,
            ..self
        }
    }
}

/// The curve that maps the HDR scene to the output's range. Each curve
/// is monotone; [`Tonemap::apply`] evaluates it on the processor, and the
/// variant documentation gives where linear mid-grey (0.18) lands.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Tonemap {
    /// Clamps; bright areas clip. Mid-grey stays 0.18.
    None,
    /// The ACES filmic fit (Stephen Hill's RRT and ODT fit): contrasty,
    /// saturated highlights. Mid-grey lands at 0.106 (the fit includes the
    /// reference curve's darkening of mid-tones).
    #[default]
    Aces,
    /// AgX: gentler, desaturates highlights towards white. Mid-grey lands
    /// at 0.215.
    Agx,
    /// Reinhard, `c / (1 + c)` per channel: soft, never reaches white.
    /// Mid-grey lands at 0.153.
    Reinhard,
    /// The Khronos PBR Neutral curve: keeps base colours as authored up
    /// to 0.76 and compresses only the highlights. Mid-grey lands at 0.14.
    Neutral,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sunset_light_is_redder_than_noon() {
        let a = Atmosphere::new(3000.0);
        let noon = a.sun_transmittance(Vec3::Y, Vec3::Y);
        let dusk = a.sun_transmittance(Vec3::Y, Vec3::new(1.0, 0.03, 0.0).normalize());
        assert!(noon.x > 0.8 && noon.z > 0.5, "{noon:?}");
        assert!(dusk.z < dusk.x * 0.5, "{dusk:?}");
        let below = a.sun_transmittance(Vec3::Y, Vec3::new(1.0, -0.3, 0.0).normalize());
        assert_eq!(below, Vec3::ZERO);
    }
}
