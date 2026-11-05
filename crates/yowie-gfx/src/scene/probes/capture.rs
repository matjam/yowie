//! Rendering a recorded scene into the six faces of a cube from one point,
//! through the ordinary scene passes.
//!
//! # Face convention
//!
//! wgpu samples cube maps in a left-handed convention, so an ordinary
//! right-handed camera renders every face mirrored. Rather than flip the
//! image (which flips triangle winding and back-face culling with it), a
//! capture stores at the texel where the cube convention has direction
//! `d` the light arriving from `M(d)`, where `M` mirrors z. Lookups mirror
//! z back (`probe_dir` in `probes.wgsl`, [`capture_dir`] on the
//! processor); prefiltering is the same in a mirror, so it runs on the
//! stored cube unchanged.

use std::f32::consts::FRAC_PI_2;

use yowie_core::{UVec2, Vec2, Vec3};

use super::Capture;
use crate::graphics::FrameStats;
use crate::scene::camera::{Camera3D, Projection};
use crate::scene::prepare::View;
use crate::scene::render::SceneRenderer;
use crate::scene::slot::{SceneSlot, SlotId, Targets};
use crate::shared::Shared;

/// Each face's camera direction and up vector, in cube face order (+X,
/// -X, +Y, -Y, +Z, -Z), under the mirrored convention above.
pub(crate) const FACES: [(Vec3, Vec3); 6] = [
    (Vec3::X, Vec3::Y),
    (Vec3::NEG_X, Vec3::Y),
    (Vec3::Y, Vec3::Z),
    (Vec3::NEG_Y, Vec3::NEG_Z),
    (Vec3::NEG_Z, Vec3::Y),
    (Vec3::Z, Vec3::Y),
];

/// The camera that captures face `face` from `position`.
pub(crate) fn face_camera(position: Vec3, face: usize, near: f32, far: f32) -> Camera3D {
    let (forward, up) = FACES[face % 6];
    Camera3D {
        position,
        target: position + forward,
        up,
        projection: Projection::Perspective { fov_y: FRAC_PI_2 },
        near,
        far,
        viewport: None,
    }
}

/// The cube convention's (unnormalised) direction through face `face` at
/// `u` (right) and `v` (down), each -1 to 1; matches `face_dir` in
/// `ibl.wgsl`.
pub(crate) fn cube_dir(face: usize, u: f32, v: f32) -> Vec3 {
    match face {
        0 => Vec3::new(1.0, -v, -u),
        1 => Vec3::new(-1.0, -v, u),
        2 => Vec3::new(u, 1.0, v),
        3 => Vec3::new(u, -1.0, -v),
        4 => Vec3::new(u, -v, 1.0),
        _ => Vec3::new(-u, -v, -1.0),
    }
}

/// The world direction whose light a capture stores at face `face`, `u`,
/// `v` (unnormalised).
pub(crate) fn capture_dir(face: usize, u: f32, v: f32) -> Vec3 {
    let d = cube_dir(face, u, v);
    Vec3::new(d.x, d.y, -d.z)
}

impl SceneRenderer {
    /// Records record `ri` seen from `position` into the six `faces`
    /// (single-layer views `size` texels square, in face order) with
    /// capture slots 0 to 5. The faces are linear HDR colour.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn capture_faces(
        &mut self,
        shared: &Shared,
        encoder: &mut wgpu::CommandEncoder,
        ri: usize,
        position: Vec3,
        range: (f32, f32),
        size: u32,
        faces: &[wgpu::TextureView],
        kind: Capture,
    ) {
        let device = shared.device();
        let (depth, depth_stencil) =
            self.probes
                .capture_depth(device, size, self.layouts.depth_format);
        while self.captures.len() < 6 {
            self.captures.push(SceneSlot::new(device));
        }
        let mut stats = FrameStats::default();
        for (f, face) in faces.iter().enumerate().take(6) {
            self.captures[f].targets = Some(Targets {
                size: UVec2::splat(size),
                color: face.clone(),
                depth: depth.clone(),
                depth_stencil: depth_stencil.clone(),
            });
            let cam = face_camera(position, f, range.0, range.1);
            let mut v = View::new(&cam, Vec2::splat(size as f32));
            v.capture = Some(kind);
            self.render_view(shared, encoder, ri, SlotId::Capture(f), &v, &mut stats);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use yowie_core::Vec4;

    /// Each face camera sees, at every point of its image, the direction
    /// the capture convention stores there.
    #[test]
    fn face_cameras_store_mirrored_cube_directions() {
        for face in 0..6 {
            let cam = face_camera(Vec3::ZERO, face, 0.1, 100.0);
            let inv = cam.view_proj(1.0).inverse();
            for (u, v) in [(0.0, 0.0), (0.7, 0.0), (0.0, 0.7), (-0.5, 0.9)] {
                // Texture v runs down, clip y up.
                let p = inv * Vec4::new(u, -v, 0.5, 1.0);
                let seen = (p.truncate() / p.w).normalize();
                let want = capture_dir(face, u, v).normalize();
                assert!(
                    seen.dot(want) > 0.9999,
                    "face {face} at {u},{v}: sees {seen}, stores {want}"
                );
            }
        }
    }
}
