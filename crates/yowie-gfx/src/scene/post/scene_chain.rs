//! Pass 2 for scenes: each scene rendered into an output this frame runs
//! its chain into its viewport of the output.

use yowie_core::{Color, Vec2, Vec3};

use super::chain::{Extras, Output};
use super::pipelines::Blend;
use super::steps::{Owned, Plan};
use super::uniform::{
    FLAG_AO, FLAG_AUTO_EXPOSURE, FLAG_BLOOM, FLAG_RAYS, FLAG_SHOW_AO, PostCamera, PostUniform,
};
use super::{PostRenderer, load_op};
use crate::scene::record::SceneRecord;
use crate::scene::render::{Rendered, SceneRenderer};
use crate::shared::Shared;

/// Where the sun is on screen, in texture coordinates, and its colour
/// scaled to a brightest channel of 1; `None` when it is behind the eye
/// or far outside the frame.
fn sun_on_screen(rec: &SceneRecord, camera: &PostCamera) -> Option<(Vec2, Vec3)> {
    let sun = rec.sun.as_ref()?;
    let to_sun = -sun.direction.normalize_or_zero();
    if to_sun == Vec3::ZERO {
        return None;
    }
    let clip = camera.steady_view_proj * to_sun.extend(0.0);
    if clip.w <= 1e-6 {
        return None;
    }
    let uv = Vec2::new(clip.x / clip.w * 0.5 + 0.5, 0.5 - clip.y / clip.w * 0.5);
    if uv.x < -1.0 || uv.x > 2.0 || uv.y < -1.0 || uv.y > 2.0 {
        return None;
    }
    let [r, g, b, _] = sun.color.to_linear_premultiplied();
    let c = Vec3::new(r, g, b);
    let m = c.max_element();
    Some((uv, if m > 0.0 { c / m } else { Vec3::ONE }))
}

impl PostRenderer {
    /// Records the post chain of every scene in `rendered` into `view`
    /// (an output of `format`). `clear` is the output's clear colour,
    /// applied by the first scene's last pass, or by a pass of its own
    /// when no scene drew. `time` is the game time in seconds. Returns
    /// the number of draws.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn run_scenes(
        &mut self,
        r: &mut SceneRenderer,
        shared: &Shared,
        encoder: &mut wgpu::CommandEncoder,
        rendered: &[Rendered],
        view: &wgpu::TextureView,
        format: wgpu::TextureFormat,
        clear: Option<Color>,
        time: f32,
    ) -> u32 {
        let (device, queue) = (shared.device(), shared.queue());
        let mut load = load_op(clear);
        let mut draws = 0;
        for item in rendered {
            let ri = item.slot;
            let rec = &r.records[ri];
            let settings = &rec.post;
            let slot = &mut r.slots[ri];
            let Some(scene) = slot.targets.as_ref() else {
                continue;
            };
            let size = scene.size;
            let mut t = slot
                .post
                .take()
                .unwrap_or_else(|| self.targets(device, size));
            t.resize(size);
            let sun = sun_on_screen(rec, &slot.camera);
            let plan = Plan::new(settings, scene, rec, sun.is_some(), self.compute);
            let mut u = PostUniform::new(
                settings,
                Vec2::new(size.x as f32, size.y as f32),
                rec.background_color,
                time,
            );
            let previous = slot.previous.unwrap_or(slot.camera.steady_view_proj);
            u.set_camera(&slot.camera, previous);
            if let Some((uv, color)) = sun {
                u.set_sun(uv, color);
            }
            if settings.wants_bloom() {
                u.add_flags(FLAG_BLOOM);
                u.bloom[0] /= super::chain::bloom_levels(size) as f32;
            }
            for (on, flag) in [
                (plan.occlusion, FLAG_AO),
                (plan.occlusion && settings.show_occlusion, FLAG_SHOW_AO),
                (plan.rays, FLAG_RAYS),
                (plan.exposure, FLAG_AUTO_EXPOSURE),
            ] {
                if on {
                    u.add_flags(flag);
                }
            }
            // History from before a cut, a resize or a frame without the
            // temporal pass is not blended in.
            let history = t.history_valid && slot.previous.is_some();
            u.temporal[3] = if history { 1.0 } else { 0.0 };
            let dt = (time - t.last_time.unwrap_or(time)).clamp(0.0, 1.0);
            u.exposure[1] = 1.0 - (-dt * settings.adaptation_speed.max(0.0)).exp();
            t.last_time = Some(time);
            queue.write_buffer(&t.uniform, 0, bytemuck::bytes_of(&u));
            let stepped = self.hdr_steps(device, queue, encoder, &mut t, &plan, scene, rec);
            draws += stepped.draws;
            let extras = Extras {
                ao: stepped.ao.as_ref().map(Owned::input),
                rays: stepped.rays.as_ref().map(Owned::input),
                exposure: stepped.exposure.as_ref().map(|(b, id)| (b, *id)),
            };
            draws += self.finish(
                device,
                encoder,
                &mut t,
                settings,
                stepped.picture.input(),
                &extras,
                Output {
                    view,
                    format,
                    viewport: Some(item.viewport),
                    load,
                    blend: Blend::Premultiplied,
                },
            );
            load = wgpu::LoadOp::Load;
            slot.previous = Some(slot.camera.steady_view_proj);
            slot.post = Some(t);
        }
        if let wgpu::LoadOp::Clear(_) = load {
            // No scene drew; the output still takes its clear colour.
            let _ = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("yowie post clear"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
        }
        draws
    }
}
