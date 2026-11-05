//! Pass 2 for scenes: each scene rendered into an output this frame runs
//! its chain into its viewport of the output.

use yowie_core::{Color, Vec2};

use super::chain::{Extras, Input, Output};
use super::pipelines::Blend;
use super::uniform::{FLAG_BLOOM, PostUniform};
use super::{PostRenderer, load_op};
use crate::scene::render::{Rendered, SceneRenderer};
use crate::shared::Shared;

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
        let device = shared.device();
        let mut load = load_op(clear);
        let mut draws = 0;
        for item in rendered {
            let ri = item.slot;
            let settings = r.records[ri].post.clone();
            let background = r.records[ri].background_color;
            let slot = &mut r.slots[ri];
            let Some(targets) = slot.targets.as_ref() else {
                continue;
            };
            let size = targets.size;
            let scene_view = targets.color.clone();
            let scene_id = targets.id;
            let mut t = slot
                .post
                .take()
                .unwrap_or_else(|| self.targets(device, size));
            t.resize(size);
            let mut u = PostUniform::new(
                &settings,
                Vec2::new(size.x as f32, size.y as f32),
                background,
                time,
            );
            let previous = slot.previous.unwrap_or(slot.camera.steady_view_proj);
            u.set_camera(&slot.camera, previous);
            if settings.wants_bloom() {
                u.add_flags(FLAG_BLOOM);
                u.bloom[0] /= super::chain::bloom_levels(size) as f32;
            }
            shared
                .queue()
                .write_buffer(&t.uniform, 0, bytemuck::bytes_of(&u));
            let src = Input {
                view: &scene_view,
                id: scene_id,
            };
            draws += self.finish(
                device,
                encoder,
                &mut t,
                &settings,
                src,
                &Extras::default(),
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
