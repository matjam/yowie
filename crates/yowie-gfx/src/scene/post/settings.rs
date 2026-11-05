//! The post-processing settings a scene or a 2D frame is finished with.

use yowie_core::{Mat3, Vec3};

use super::lut::Lut;
use crate::scene::Tonemap;

/// How a scene's HDR image, or a 2D frame, is finished on its way to the
/// output: exposure, the tone curve, colour grading, bloom, screen-space
/// ambient occlusion and reflections, anti-aliasing, depth of field,
/// motion blur, light shafts and lens effects.
///
/// Set it on a scene with [`Scene::set_post`](crate::scene::Scene::set_post)
/// or on a whole 2D frame with
/// [`Graphics::set_frame_post`](crate::Graphics::set_frame_post), every
/// frame like the rest of the drawing. The default changes nothing but
/// exposure 1 and the ACES curve, which is what a scene got before post
/// processing existed; every effect is off until its strength or switch is
/// set, and an effect that is off costs no pass.
///
/// The names follow Bunyip's `PostSettings`. Unlike Bunyip, bloom, ambient
/// occlusion and FXAA are off by default.
///
/// ```no_run
/// # use yowie_gfx::scene::*;
/// # fn f(s: &mut Scene) {
/// s.set_post(PostSettings {
///     bloom: 0.3,
///     ambient_occlusion: 0.8,
///     temporal_aa: true,
///     vignette: 0.25,
///     ..PostSettings::default()
/// });
/// # }
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct PostSettings {
    /// Multiplies the scene's light before tone mapping; 1 by default.
    /// With [`auto_exposure`](Self::auto_exposure) it multiplies the
    /// automatic exposure.
    pub exposure: f32,
    /// Sets the exposure from the scene's average brightness each frame,
    /// so a dark interior and a sunlit field both land at mid-grey. The
    /// exposure moves towards its target at
    /// [`adaptation_speed`](Self::adaptation_speed). Off by default.
    pub auto_exposure: bool,
    /// Stops added to the automatic exposure: +1 doubles it. 0 by default.
    pub exposure_compensation: f32,
    /// How fast the automatic exposure follows the scene, per second;
    /// larger is faster. 1.5 by default.
    pub adaptation_speed: f32,
    /// The curve from HDR light to the output's range; ACES by default.
    /// A 2D frame is not tone mapped.
    pub tonemap: Tonemap,

    /// White balance: below zero cools the picture towards blue, above
    /// zero warms it towards orange; -1 to 1, 0 by default.
    pub temperature: f32,
    /// White balance: below zero shifts towards green, above zero towards
    /// magenta; -1 to 1, 0 by default.
    pub tint: f32,
    /// 0 is grey, 1 (the default) unchanged, above 1 more colourful.
    pub saturation: f32,
    /// Spreads colours away from mid-grey (above 1) or towards it (below);
    /// 1 by default.
    pub contrast: f32,
    /// Raises the shadows per channel; 0 by default.
    pub lift: Vec3,
    /// Bends the mid-tones per channel: above 1 brightens them; 1 by
    /// default.
    pub gamma: Vec3,
    /// Scales the highlights per channel; 1 by default.
    pub gain: Vec3,
    /// A colour lookup table applied after the rest of the grade; `None`
    /// by default.
    pub lut: Option<Lut>,
    /// How far towards the lookup table's colour to go, 0 to 1; 1 by
    /// default.
    pub lut_strength: f32,

    /// Bloom strength: how much of the light above the threshold spreads
    /// into a glow; 0 (off) by default.
    pub bloom: f32,
    /// The brightness (linear, after exposure) where bloom starts, with a
    /// soft knee below it; 1 by default.
    pub bloom_threshold: f32,

    /// Strength of screen-space ambient occlusion, 0 (off, the default) to
    /// 1. It darkens the ambient and image-based light in creases and
    /// where objects meet; direct light is left alone.
    pub ambient_occlusion: f32,
    /// How far, in world units, occlusion reaches; 1 by default.
    pub occlusion_radius: f32,
    /// Shows the occlusion buffer instead of the picture, for tuning.
    pub show_occlusion: bool,

    /// Strength of screen-space reflections, 0 (off, the default) to 1.
    /// Smooth opaque surfaces reflect what the screen already shows; where
    /// a ray leaves the screen or hits nothing the surface keeps its
    /// environment reflection.
    pub reflections: f32,
    /// The roughness above which a surface stops reflecting the screen,
    /// fading out over the half below it; 0.35 by default.
    pub reflection_roughness: f32,
    /// How far a reflection ray travels in world units; 30 by default.
    pub reflection_distance: f32,
    /// The most steps a reflection ray takes through the depth pyramid,
    /// each one cell of the level it is on; 128 by default. Fewer cost
    /// less and lose long grazing reflections.
    pub reflection_steps: u32,

    /// Multisampling of the scene pass: 1 (off, the default), 2 or 4,
    /// lowered to what the GPU supports. Edges are resolved from that many
    /// coverage samples without blurring the picture.
    pub samples: u32,
    /// Fast approximate anti-aliasing of the finished picture; off by
    /// default.
    pub fxaa: bool,
    /// Temporal anti-aliasing: the projection moves by a fraction of a
    /// pixel every frame and each frame is averaged with the ones before
    /// it, reprojected by its motion; off by default. Moving objects need
    /// [`Scene::draw_moved`](crate::scene::Scene::draw_moved) to
    /// reproject cleanly, and a cut needs
    /// [`Scene::cut`](crate::scene::Scene::cut).
    pub temporal_aa: bool,
    /// How much of the new frame goes into the temporal average, 0.02 to
    /// 1; lower is steadier and softer. 0.1 by default.
    pub temporal_blend: f32,

    /// Distance in front of the camera, in world units, that is in focus;
    /// 0 turns depth of field off (the default).
    pub focus_distance: f32,
    /// How far either side of the focus distance stays sharp, and how far
    /// beyond that the blur takes to reach its full size; 0 means a
    /// quarter of the focus distance.
    pub focus_range: f32,
    /// The widest blur in pixels of a 1080-pixel-high picture (the
    /// aperture); 12 by default.
    pub bokeh_radius: f32,
    /// Taps in the blur's disc; 16 by default.
    pub bokeh_samples: u32,

    /// Smears each pixel back along the way it moved since the last
    /// frame, 0 (off, the default) to 1 of a frame's motion.
    pub motion_blur: f32,
    /// Taps along the motion; 8 by default.
    pub motion_samples: u32,

    /// Strength of the light shafts the sun throws past what stands in
    /// front of the sky; 0 (off) by default. Skipped while the sun is
    /// behind the camera.
    pub god_rays: f32,
    /// How fast a shaft fades along its length; 0.96 by default.
    pub god_ray_decay: f32,
    /// How far towards the sun each pixel looks, as a fraction of the way;
    /// 0.6 by default.
    pub god_ray_density: f32,
    /// Steps along each shaft; 32 by default.
    pub god_ray_samples: u32,

    /// Darkens the corners, 0 (off, the default) to 1.
    pub vignette: f32,
    /// Splits red and blue apart towards the edges as a cheap lens does;
    /// 1 is about three pixels at the edge of a 1080-pixel-wide picture. 0
    /// by default.
    pub aberration: f32,
    /// Bends the picture about its centre: above zero is barrel, below
    /// pincushion. 0 by default.
    pub distortion: f32,
    /// Mirrored copies of the bright parts through the centre, the ghosts
    /// a lens makes of a bright light; needs bloom. 0 by default.
    pub ghosts: f32,
    /// Noise that changes every frame, as film shows; 0.05 is subtle. 0 by
    /// default.
    pub grain: f32,
}

impl Default for PostSettings {
    fn default() -> Self {
        Self {
            exposure: 1.0,
            auto_exposure: false,
            exposure_compensation: 0.0,
            adaptation_speed: 1.5,
            tonemap: Tonemap::default(),
            temperature: 0.0,
            tint: 0.0,
            saturation: 1.0,
            contrast: 1.0,
            lift: Vec3::ZERO,
            gamma: Vec3::ONE,
            gain: Vec3::ONE,
            lut: None,
            lut_strength: 1.0,
            bloom: 0.0,
            bloom_threshold: 1.0,
            ambient_occlusion: 0.0,
            occlusion_radius: 1.0,
            show_occlusion: false,
            reflections: 0.0,
            reflection_roughness: 0.35,
            reflection_distance: 30.0,
            reflection_steps: 128,
            samples: 1,
            fxaa: false,
            temporal_aa: false,
            temporal_blend: 0.1,
            focus_distance: 0.0,
            focus_range: 0.0,
            bokeh_radius: 12.0,
            bokeh_samples: 16,
            motion_blur: 0.0,
            motion_samples: 8,
            god_rays: 0.0,
            god_ray_decay: 0.96,
            god_ray_density: 0.6,
            god_ray_samples: 32,
            vignette: 0.0,
            aberration: 0.0,
            distortion: 0.0,
            ghosts: 0.0,
            grain: 0.0,
        }
    }
}

impl PostSettings {
    /// Whether the colour grade (white balance, saturation, contrast,
    /// lift, gamma, gain or a lookup table) changes anything.
    pub(crate) fn grades(&self) -> bool {
        self.temperature != 0.0
            || self.tint != 0.0
            || self.saturation != 1.0
            || self.contrast != 1.0
            || self.lift != Vec3::ZERO
            || self.gamma != Vec3::ONE
            || self.gain != Vec3::ONE
            || (self.lut.is_some() && self.lut_strength > 0.0)
    }

    /// Whether a lens effect (vignette, aberration, distortion, grain) is
    /// on. Ghosts belong to bloom.
    pub(crate) fn lens(&self) -> bool {
        self.vignette != 0.0
            || self.aberration != 0.0
            || self.distortion != 0.0
            || self.grain != 0.0
    }

    pub(crate) fn wants_bloom(&self) -> bool {
        self.bloom > 0.0 || self.ghosts > 0.0
    }

    pub(crate) fn wants_occlusion(&self) -> bool {
        self.ambient_occlusion > 0.0 || self.show_occlusion
    }

    pub(crate) fn wants_reflections(&self) -> bool {
        self.reflections > 0.0
    }

    pub(crate) fn wants_dof(&self) -> bool {
        self.focus_distance > 0.0
    }

    pub(crate) fn wants_motion_blur(&self) -> bool {
        self.motion_blur > 0.0
    }

    /// The focus range with its default filled in.
    pub(crate) fn focus_range_or_default(&self) -> f32 {
        if self.focus_range > 0.0 {
            self.focus_range
        } else {
            self.focus_distance * 0.25
        }
    }

    /// The white balance as a matrix on linear RGB: the temperature and
    /// tint move the white point along the daylight locus and scale the
    /// colour in LMS cone space to match (the method of Unity's white
    /// balance), normalised so white keeps its brightness.
    pub(crate) fn white_balance(&self) -> Mat3 {
        if self.temperature == 0.0 && self.tint == 0.0 {
            return Mat3::IDENTITY;
        }
        let t1 = self.temperature.clamp(-1.0, 1.0) * 1.67;
        let t2 = self.tint.clamp(-1.0, 1.0) * 1.67;
        // The chromaticity of the shifted white point.
        let x = 0.31271 - t1 * if t1 < 0.0 { 0.1 } else { 0.05 };
        let standard_y = 2.87 * x - 3.0 * x * x - 0.275_095_7;
        let y = standard_y + t2 * 0.05;
        let to_lms = |x: f32, y: f32| {
            let big_x = x / y;
            let big_z = (1.0 - x - y) / y;
            Vec3::new(
                0.7328 * big_x + 0.4296 - 0.1624 * big_z,
                -0.7036 * big_x + 1.6975 + 0.0061 * big_z,
                0.0030 * big_x + 0.0136 + 0.9834 * big_z,
            )
        };
        let gains = to_lms(0.31271, 0.32902) / to_lms(x, y);
        // Rows of the linear sRGB to LMS matrix and its inverse; glam
        // stores columns, so each is transposed.
        let rgb_to_lms = Mat3::from_cols_array(&[
            0.390_405,
            0.549_941,
            0.008_926_32, //
            0.070_841_6,
            0.963_172,
            0.001_357_75, //
            0.023_107_8,
            0.128_021,
            0.936_245,
        ])
        .transpose();
        let lms_to_rgb = Mat3::from_cols_array(&[
            2.858_47,
            -1.628_79,
            -0.024_891, //
            -0.210_182,
            1.158_2,
            0.000_324_281, //
            -0.041_812,
            -0.118_169,
            1.068_67,
        ])
        .transpose();
        let m = lms_to_rgb * Mat3::from_diagonal(gains) * rgb_to_lms;
        let white = (m * Vec3::ONE).dot(Vec3::new(0.2126, 0.7152, 0.0722));
        m * (1.0 / white.max(1e-4))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_turns_every_effect_off() {
        let p = PostSettings::default();
        assert!(!p.grades());
        assert!(!p.lens());
        assert!(!p.wants_bloom());
        assert!(!p.wants_occlusion());
        assert!(!p.wants_reflections());
        assert!(!p.wants_dof());
        assert!(!p.wants_motion_blur());
        assert_eq!(p.white_balance(), Mat3::IDENTITY);
    }

    #[test]
    fn warm_white_balance_raises_red_over_blue() {
        let balance = |temperature| {
            PostSettings {
                temperature,
                ..PostSettings::default()
            }
            .white_balance()
                * Vec3::splat(0.5)
        };
        let (warm, cool) = (balance(0.8), balance(-0.8));
        assert!(warm.x > warm.z * 1.1, "{warm}");
        assert!(cool.z > cool.x * 1.1, "{cool}");
    }
}
