//! Shadow maps: one depth atlas holding three sun cascades, four spot
//! maps and the six faces of four point lights, and the matrices that
//! render and sample them.
//!
//! The atlas is 4096 texels wide. The top 4096 square holds the cascades
//! (2048 each) in three quadrants and four 1024 spot maps in the fourth;
//! the 1536-texel strip below holds the 24 cube faces at 512 each.

use yowie_core::glam::camera::rh::proj::directx;
use yowie_core::glam::camera::rh::view::look_at_mat4;
use yowie_core::{Mat4, Vec3, Vec4};

use super::bounds::Frustum;
use super::frame_data::ShadowViewGpu;
use super::layouts::SHADOW_FORMAT;

/// The most spot lights that get a shadow map in one scene.
pub const MAX_SPOT_SHADOWS: usize = 4;
/// The most point lights that get cube shadow maps in one scene.
pub const MAX_POINT_SHADOWS: usize = 4;
/// Sun cascades.
pub(crate) const CASCADES: usize = 3;
/// The first spot view.
pub(crate) const SPOT_BASE: usize = CASCADES;
/// The first point light face.
pub(crate) const POINT_BASE: usize = SPOT_BASE + MAX_SPOT_SHADOWS;

const ATLAS_W: u32 = 4096;
const ATLAS_H: u32 = 4096 + 1536;
const CASCADE_SIZE: u32 = 2048;
const SPOT_SIZE: u32 = 1024;
const FACE_SIZE: u32 = 512;

/// The atlas texture and its views.
#[derive(Debug)]
pub(crate) struct ShadowAtlas {
    pub(crate) sample_view: wgpu::TextureView,
    pub(crate) target_view: wgpu::TextureView,
}

impl ShadowAtlas {
    pub(crate) fn new(device: &wgpu::Device) -> Self {
        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("yowie shadow atlas"),
            size: wgpu::Extent3d {
                width: ATLAS_W,
                height: ATLAS_H,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: SHADOW_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        Self {
            sample_view: tex.create_view(&Default::default()),
            target_view: tex.create_view(&Default::default()),
        }
    }

    /// A one-texel atlas for scenes without shadows, cleared to the far
    /// depth so every lookup is lit.
    pub(crate) fn empty(device: &wgpu::Device, encoder: &mut wgpu::CommandEncoder) -> Self {
        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("yowie empty shadow atlas"),
            size: wgpu::Extent3d::default(),
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: SHADOW_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = tex.create_view(&Default::default());
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("yowie clear empty atlas"),
            color_attachments: &[],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        Self {
            sample_view: view.clone(),
            target_view: view,
        }
    }
}

/// Where view `index` sits in the atlas, in texels: x, y, size.
pub(crate) fn atlas_rect(index: usize) -> (u32, u32, u32) {
    if index < CASCADES {
        let i = index as u32;
        ((i % 2) * CASCADE_SIZE, (i / 2) * CASCADE_SIZE, CASCADE_SIZE)
    } else if index < POINT_BASE {
        let j = (index - SPOT_BASE) as u32;
        (
            CASCADE_SIZE + (j % 2) * SPOT_SIZE,
            CASCADE_SIZE + (j / 2) * SPOT_SIZE,
            SPOT_SIZE,
        )
    } else {
        let k = (index - POINT_BASE) as u32;
        ((k % 8) * FACE_SIZE, 4096 + (k / 8) * FACE_SIZE, FACE_SIZE)
    }
}

/// One shadow map to render this frame.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ShadowView {
    pub(crate) index: usize,
    pub(crate) view_proj: Mat4,
    pub(crate) frustum: Frustum,
    /// Normal offset in world units, and per unit of distance.
    pub(crate) offset: (f32, f32),
}

impl ShadowView {
    fn new(index: usize, view_proj: Mat4, offset: (f32, f32)) -> Self {
        Self {
            index,
            view_proj,
            frustum: Frustum::from_view_proj(&view_proj),
            offset,
        }
    }

    pub(crate) fn gpu(&self) -> ShadowViewGpu {
        let (x, y, s) = atlas_rect(self.index);
        let (w, h) = (ATLAS_W as f32, ATLAS_H as f32);
        ShadowViewGpu {
            view_proj: self.view_proj.to_cols_array_2d(),
            rect: [x as f32 / w, y as f32 / h, s as f32 / w, s as f32 / h],
            params: [self.offset.0, self.offset.1, 0.0, 0.0],
        }
    }
}

/// The atlas's texel size in texture coordinates.
pub(crate) fn atlas_texel() -> [f32; 2] {
    [1.0 / ATLAS_W as f32, 1.0 / ATLAS_H as f32]
}

fn up_for(dir: Vec3) -> Vec3 {
    if dir.y.abs() > 0.99 { Vec3::Z } else { Vec3::Y }
}

/// The view depths where the cascades end: a blend of even and
/// logarithmic splits over `distance`.
pub(crate) fn cascade_splits(near: f32, distance: f32) -> [f32; CASCADES] {
    let far = distance.max(near * 2.0);
    std::array::from_fn(|i| {
        let t = (i + 1) as f32 / CASCADES as f32;
        let log = near * (far / near).powf(t);
        let even = near + (far - near) * t;
        even + (log - even) * 0.8
    })
}

/// The cascades for a camera with inverse view-projection `inv` (reversed
/// depth) and projection depth range `near..far`. `to_sun` points at the
/// light; `caster_reach` is the farthest any shadow caster stands towards
/// the light, measured along `to_sun`.
pub(crate) fn cascades(
    inv_view_proj: &Mat4,
    camera_near: f32,
    camera_far: f32,
    ortho: bool,
    splits: &[f32; CASCADES],
    to_sun: Vec3,
    caster_reach: f32,
) -> [ShadowView; CASCADES] {
    // The NDC depth of a view distance, for reversed depth.
    let ndc_z = |d: f32| {
        if ortho {
            (camera_far - d) / (camera_far - camera_near)
        } else {
            camera_near * (camera_far - d) / (d * (camera_far - camera_near))
        }
    };
    let rot = look_at_mat4(Vec3::ZERO, -to_sun, up_for(to_sun));
    let rot_inv = rot.inverse();
    let mut prev = camera_near;
    std::array::from_fn(|i| {
        let (d0, d1) = (prev, splits[i]);
        prev = splits[i];
        let mut corners = [Vec3::ZERO; 8];
        let mut k = 0;
        for z in [ndc_z(d0), ndc_z(d1)] {
            for y in [-1.0, 1.0] {
                for x in [-1.0, 1.0] {
                    let p = *inv_view_proj * Vec4::new(x, y, z, 1.0);
                    corners[k] = p.truncate() / p.w;
                    k += 1;
                }
            }
        }
        let center = corners.iter().copied().sum::<Vec3>() / 8.0;
        let radius = corners
            .iter()
            .map(|c| (*c - center).length())
            .fold(0.0f32, f32::max);
        // A radius rounded up and a centre snapped to whole texels keep
        // the map from shimmering as the camera moves.
        let radius = (radius * 16.0).ceil() / 16.0;
        let texel = 2.0 * radius / CASCADE_SIZE as f32;
        let mut ls = rot.transform_point3(center);
        ls.x = (ls.x / texel).floor() * texel;
        ls.y = (ls.y / texel).floor() * texel;
        let center = rot_inv.transform_point3(ls);
        let back = (caster_reach - center.dot(to_sun)).max(radius) + radius * 0.1;
        let eye = center + to_sun * back;
        let view = look_at_mat4(eye, center, up_for(to_sun));
        let proj = directx::orthographic(-radius, radius, -radius, radius, 0.0, back + radius);
        ShadowView::new(i, proj * view, (texel * 1.5, 0.0))
    })
}

/// The shadow map of a spot light.
pub(crate) fn spot(
    index: usize,
    position: Vec3,
    direction: Vec3,
    outer: f32,
    range: f32,
) -> ShadowView {
    let dir = direction.normalize_or(Vec3::NEG_Y);
    let fov = (outer.clamp(0.01, 1.5) * 2.0 + 0.1).min(3.0);
    let near = (range * 0.01).max(0.02);
    let view = look_at_mat4(position, position + dir, up_for(dir));
    let proj = directx::perspective(fov, 1.0, near, range);
    let per_unit = 2.0 * (fov * 0.5).tan() / SPOT_SIZE as f32 * 1.5;
    ShadowView::new(index, proj * view, (0.0, per_unit))
}

/// The six cube face maps of a point light, in the face order the surface
/// program picks them: +X, -X, +Y, -Y, +Z, -Z.
pub(crate) fn point_faces(base: usize, position: Vec3, range: f32) -> [ShadowView; 6] {
    let dirs = [
        (Vec3::X, Vec3::NEG_Y),
        (Vec3::NEG_X, Vec3::NEG_Y),
        (Vec3::Y, Vec3::Z),
        (Vec3::NEG_Y, Vec3::NEG_Z),
        (Vec3::Z, Vec3::NEG_Y),
        (Vec3::NEG_Z, Vec3::NEG_Y),
    ];
    let near = (range * 0.01).max(0.02);
    // A little over 90 degrees, so filtering at a face's edge stays inside
    // the face.
    let fov = 92f32.to_radians();
    let proj = directx::perspective(fov, 1.0, near, range);
    let per_unit = 2.0 * (fov * 0.5).tan() / FACE_SIZE as f32 * 1.5;
    std::array::from_fn(|f| {
        let (d, up) = dirs[f];
        let view = look_at_mat4(position, position + d, up);
        ShadowView::new(base + f, proj * view, (0.0, per_unit))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atlas_regions_do_not_overlap() {
        let views = POINT_BASE + MAX_POINT_SHADOWS * 6;
        assert_eq!(views, super::super::frame_data::MAX_SHADOW_VIEWS);
        let rects: Vec<_> = (0..views).map(atlas_rect).collect();
        for (i, a) in rects.iter().enumerate() {
            assert!(a.0 + a.2 <= ATLAS_W && a.1 + a.2 <= ATLAS_H, "{i}");
            for b in &rects[i + 1..] {
                let apart =
                    a.0 + a.2 <= b.0 || b.0 + b.2 <= a.0 || a.1 + a.2 <= b.1 || b.1 + b.2 <= a.1;
                assert!(apart, "{a:?} {b:?}");
            }
        }
    }

    #[test]
    fn splits_grow() {
        let s = cascade_splits(0.1, 60.0);
        assert!(s[0] < s[1] && s[1] < s[2] && (s[2] - 60.0).abs() < 1e-3);
    }
}
