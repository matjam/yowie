//! The post block, written once a frame per scene or 2D frame; matches
//! `Post` in `shaders/prelude.wgsl`.

use bytemuck::{Pod, Zeroable};
use yowie_core::{Mat4, Vec2, Vec3};

use super::settings::PostSettings;

pub(crate) const FLAG_GRADE: u32 = 1;
pub(crate) const FLAG_LUT: u32 = 2;
pub(crate) const FLAG_LENS: u32 = 4;
pub(crate) const FLAG_BLOOM: u32 = 8;
pub(crate) const FLAG_RAYS: u32 = 16;
pub(crate) const FLAG_AO: u32 = 32;
pub(crate) const FLAG_SHOW_AO: u32 = 64;
pub(crate) const FLAG_AUTO_EXPOSURE: u32 = 128;
pub(crate) const FLAG_FLAT: u32 = 256;

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub(crate) struct PostUniform {
    pub(crate) background: [f32; 4],
    pub(crate) tone: [f32; 4],
    pub(crate) bloom: [f32; 4],
    pub(crate) occlusion: [f32; 4],
    pub(crate) balance: [[f32; 4]; 3],
    pub(crate) grade: [f32; 4],
    pub(crate) lift: [f32; 4],
    pub(crate) gamma: [f32; 4],
    pub(crate) gain: [f32; 4],
    pub(crate) lens: [f32; 4],
    pub(crate) size: [f32; 4],
    pub(crate) rays: [[f32; 4]; 3],
    pub(crate) dof: [f32; 4],
    pub(crate) temporal: [f32; 4],
    pub(crate) ssr: [f32; 4],
    pub(crate) exposure: [f32; 4],
    pub(crate) proj: [[f32; 4]; 4],
    pub(crate) inv_proj: [[f32; 4]; 4],
    pub(crate) view: [[f32; 4]; 4],
    pub(crate) view_proj: [[f32; 4]; 4],
    pub(crate) inv_view_proj: [[f32; 4]; 4],
    pub(crate) reproject: [[f32; 4]; 4],
}

/// The camera facts a scene's post chain reads, captured when the scene
/// renders.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PostCamera {
    pub(crate) view: Mat4,
    /// The projection the scene rasterised with, jitter included.
    pub(crate) proj: Mat4,
    /// `proj * view`.
    pub(crate) view_proj: Mat4,
    /// The view-projection without jitter, which the next frame
    /// reprojects against.
    pub(crate) steady_view_proj: Mat4,
    /// The sub-pixel offset of this frame's projection, in pixels.
    pub(crate) jitter: Vec2,
}

impl Default for PostCamera {
    fn default() -> Self {
        Self {
            view: Mat4::IDENTITY,
            proj: Mat4::IDENTITY,
            view_proj: Mat4::IDENTITY,
            steady_view_proj: Mat4::IDENTITY,
            jitter: Vec2::ZERO,
        }
    }
}

fn v4(v: Vec3, w: f32) -> [f32; 4] {
    [v.x, v.y, v.z, w]
}

impl PostUniform {
    /// The block for `settings` over a picture `size` pixels large. The
    /// flags of the passes that need inputs (bloom, shafts, occlusion) are
    /// added by the chain once it has made them.
    pub(crate) fn new(
        settings: &PostSettings,
        size: Vec2,
        background: [f32; 4],
        time: f32,
    ) -> Self {
        let mut flags = 0;
        if settings.grades() {
            flags |= FLAG_GRADE;
            if settings.lut.is_some() && settings.lut_strength > 0.0 {
                flags |= FLAG_LUT;
            }
        }
        if settings.lens() {
            flags |= FLAG_LENS;
        }
        let balance = settings.white_balance();
        let threshold = settings.bloom_threshold.max(0.0);
        Self {
            background,
            tone: [
                settings.exposure,
                settings.tonemap.shader_index(),
                flags as f32,
                time,
            ],
            bloom: [
                settings.bloom.max(0.0),
                threshold,
                threshold * 0.5,
                settings.ghosts.max(0.0),
            ],
            occlusion: [
                settings.ambient_occlusion.clamp(0.0, 1.0),
                settings.occlusion_radius.max(1e-3),
                0.0,
                0.0,
            ],
            balance: [
                v4(balance.x_axis, 0.0),
                v4(balance.y_axis, 0.0),
                v4(balance.z_axis, 0.0),
            ],
            grade: [
                settings.contrast.max(0.0),
                settings.saturation.max(0.0),
                settings.lut_strength.clamp(0.0, 1.0),
                settings.lut.as_ref().map_or(2.0, |l| l.size() as f32),
            ],
            lift: v4(settings.lift, 0.0),
            gamma: v4(settings.gamma, 0.0),
            gain: v4(settings.gain, 0.0),
            lens: [
                settings.vignette.max(0.0),
                settings.aberration.max(0.0),
                settings.distortion,
                settings.grain.max(0.0),
            ],
            size: [size.x, size.y, 1.0 / size.x, 1.0 / size.y],
            rays: [
                [
                    0.5,
                    0.5,
                    settings.god_rays.max(0.0),
                    settings.god_ray_decay.clamp(0.0, 1.0),
                ],
                [
                    settings.god_ray_samples.clamp(2, 256) as f32,
                    settings.god_ray_density.max(0.0),
                    1.0 / settings.god_ray_samples.clamp(2, 256) as f32,
                    0.0,
                ],
                [1.0, 1.0, 1.0, 0.0],
            ],
            dof: [
                settings.focus_distance.max(0.0),
                settings.focus_range_or_default().max(1e-4),
                // The gather runs at half size.
                settings.bokeh_radius.max(0.0) * 0.5,
                settings.bokeh_samples.clamp(1, 256) as f32,
            ],
            temporal: [
                settings.motion_blur.max(0.0),
                settings.motion_samples.max(2) as f32,
                settings.temporal_blend.clamp(0.02, 1.0),
                0.0,
            ],
            ssr: [
                settings.reflections.clamp(0.0, 1.0),
                settings.reflection_roughness.max(1e-3),
                settings.reflection_distance.max(1e-3),
                settings.reflection_steps.clamp(1, 512) as f32,
            ],
            exposure: [settings.exposure_compensation, 0.0, 0.0, 0.0],
            proj: Mat4::IDENTITY.to_cols_array_2d(),
            inv_proj: Mat4::IDENTITY.to_cols_array_2d(),
            view: Mat4::IDENTITY.to_cols_array_2d(),
            view_proj: Mat4::IDENTITY.to_cols_array_2d(),
            inv_view_proj: Mat4::IDENTITY.to_cols_array_2d(),
            reproject: Mat4::IDENTITY.to_cols_array_2d(),
        }
    }

    /// Writes the camera matrices; `previous` is last frame's steady
    /// view-projection.
    pub(crate) fn set_camera(&mut self, cam: &PostCamera, previous: Mat4) {
        self.proj = cam.proj.to_cols_array_2d();
        self.inv_proj = cam.proj.inverse().to_cols_array_2d();
        self.view = cam.view.to_cols_array_2d();
        self.view_proj = cam.view_proj.to_cols_array_2d();
        let inv = cam.view_proj.inverse();
        self.inv_view_proj = inv.to_cols_array_2d();
        self.reproject = (previous * inv).to_cols_array_2d();
        self.exposure[2] = cam.jitter.x;
        self.exposure[3] = cam.jitter.y;
    }

    /// Points the light shafts at the sun, at `uv` on screen, with the
    /// light's linear colour.
    pub(crate) fn set_sun(&mut self, uv: Vec2, color: Vec3) {
        self.rays[0][0] = uv.x;
        self.rays[0][1] = uv.y;
        self.rays[2] = v4(color, 0.0);
    }

    pub(crate) fn flags(&self) -> u32 {
        self.tone[2] as u32
    }

    pub(crate) fn add_flags(&mut self, flags: u32) {
        self.tone[2] = (self.flags() | flags) as f32;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn size_is_a_multiple_of_sixteen() {
        assert_eq!(std::mem::size_of::<PostUniform>() % 16, 0);
        assert_eq!(std::mem::size_of::<PostUniform>(), 20 * 16 + 6 * 64);
    }
}
