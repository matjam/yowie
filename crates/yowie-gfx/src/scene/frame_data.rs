//! The per-scene uniform block; matches `Frame` in `shaders/common.wgsl`.

use bytemuck::{Pod, Zeroable};
use yowie_core::Vec3;

use super::prepare::View;
use super::record::SceneRecord;

/// The most shadow views a scene renders: three cascades, four spot maps
/// and the six faces of four point lights.
pub(crate) const MAX_SHADOW_VIEWS: usize = 31;

/// One shadow map: its matrix, where it sits in the atlas, and its normal
/// offset.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub(crate) struct ShadowViewGpu {
    pub(crate) view_proj: [[f32; 4]; 4],
    pub(crate) rect: [f32; 4],
    /// Normal offset in world units, normal offset per unit of distance
    /// from the light, 0, 0.
    pub(crate) params: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub(crate) struct FrameUniform {
    pub(crate) view: [[f32; 4]; 4],
    pub(crate) proj: [[f32; 4]; 4],
    pub(crate) view_proj: [[f32; 4]; 4],
    pub(crate) inv_view_proj: [[f32; 4]; 4],
    pub(crate) camera: [f32; 4],
    pub(crate) target_size: [f32; 4],
    pub(crate) sun_dir: [f32; 4],
    pub(crate) sun_color: [f32; 4],
    pub(crate) ambient: [f32; 4],
    pub(crate) env: [f32; 4],
    pub(crate) sky_up: [f32; 4],
    pub(crate) sky_zenith: [f32; 4],
    pub(crate) sky_horizon: [f32; 4],
    pub(crate) sky_ground: [f32; 4],
    pub(crate) sun_disc: [f32; 4],
    pub(crate) atmos: [f32; 4],
    pub(crate) beta_r: [f32; 4],
    pub(crate) beta_m: [f32; 4],
    pub(crate) fog_color: [f32; 4],
    pub(crate) fog_params: [f32; 4],
    pub(crate) cluster: [f32; 4],
    pub(crate) cluster_depth: [f32; 4],
    pub(crate) shadow: [f32; 4],
    pub(crate) cascade_far: [f32; 4],
    pub(crate) counts: [u32; 4],
    pub(crate) shadow_views: [ShadowViewGpu; MAX_SHADOW_VIEWS],
}

impl Default for FrameUniform {
    fn default() -> Self {
        let mut f = Self::zeroed();
        f.sky_up = [0.0, 1.0, 0.0, 0.0];
        f.sky_horizon[3] = 1.0;
        f.atmos = [1.0, 1.0, 1.0, 1.0];
        f
    }
}

/// Writes the camera, sun, ambient, sky and fog into the frame block. The
/// light, cluster and shadow fields are written by `prepare`.
pub(crate) fn fill_frame(rec: &SceneRecord, v: &View, f: &mut FrameUniform) {
    f.view = v.view.to_cols_array_2d();
    f.proj = v.proj.to_cols_array_2d();
    f.view_proj = v.view_proj.to_cols_array_2d();
    f.inv_view_proj = v.view_proj.inverse().to_cols_array_2d();
    // W carries the scene time surface shaders read with `scene_time()`.
    f.camera = v.position.extend(rec.time).to_array();
    f.target_size = [v.size.x, v.size.y, 1.0 / v.size.x, 1.0 / v.size.y];
    f.counts[1] = u32::from(v.ortho);
    let ambient = rec.ambient.to_linear();
    f.ambient = [ambient[0], ambient[1], ambient[2], 0.0];
    let mut sun_radiance = Vec3::ZERO;
    if let Some(sun) = rec.sun {
        let to_sun = -sun.direction.normalize_or(Vec3::NEG_Y);
        let c = sun.color.to_linear();
        let mut radiance = Vec3::new(c[0], c[1], c[2]) * sun.intensity;
        sun_radiance = radiance;
        if let Some(a) = rec
            .sky
            .and_then(|s| s.atmosphere)
            .filter(|a| a.height > 0.0)
        {
            let up = rec.sky.map_or(Vec3::Y, |s| s.up_axis());
            radiance *= a.sun_transmittance(up, to_sun);
        }
        f.sun_dir = to_sun.extend(1.0).to_array();
        f.sun_color = radiance
            .extend(sun.shadow_strength.clamp(0.0, 1.0))
            .to_array();
    }
    if let Some(sky) = &rec.sky {
        sky.write(f, sun_radiance);
    }
    if let Some(fog) = &rec.fog {
        let c = fog.color.to_linear();
        f.fog_color = [c[0], c[1], c[2], fog.density.max(0.0)];
        f.fog_params = [fog.start, fog.end, fog.height, fog.height_falloff.max(0.0)];
    }
}

/// A key for the sky's look, quantised so small sun moves reuse a bake.
pub(crate) fn sky_key(f: &FrameUniform) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    let q = |v: f32, step: f32| (v / step).round() as i64;
    for v in f.sun_dir.iter().take(3) {
        q(*v, 1.0 / 64.0).hash(&mut h);
    }
    for arr in [
        f.sky_up,
        f.sky_zenith,
        f.sky_horizon,
        f.sky_ground,
        f.atmos,
        f.beta_r,
    ] {
        for v in arr {
            v.to_bits().hash(&mut h);
        }
    }
    q(f.beta_m[2], f.atmos[1] / 256.0).hash(&mut h);
    for v in [f.beta_m[0], f.beta_m[1], f.beta_m[3]] {
        v.to_bits().hash(&mut h);
    }
    h.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn size_matches_the_shader() {
        // 4 matrices, 20 vectors, the counts and 31 views of 96 bytes.
        assert_eq!(
            std::mem::size_of::<FrameUniform>(),
            256 + 20 * 16 + 16 + 31 * 96
        );
    }
}
