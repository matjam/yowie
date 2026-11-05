//! Pass 2: resolving each scene's HDR image into the output.
//!
//! [`resolve_output`] is the one function the post-processing chain
//! replaces. It receives every scene rendered into an output this frame
//! (each slot holds the HDR colour and depth in `Targets`) and writes the
//! output: today exposure, a tone curve (ACES or AgX, `shaders/resolve.wgsl`)
//! and composition over the background colour, in each scene's viewport.
//! A chain of effects reads the same targets and ends by writing the same
//! viewports; the 2D pass that follows loads the output either way.

use yowie_core::Color;

use super::render::{Rendered, SceneRenderer};
use crate::shared::Shared;

/// Records the resolve of every scene in `rendered` into `view` (an output
/// of `format`). `clear` is the output's clear colour, applied first.
pub(crate) fn resolve_output(
    r: &mut SceneRenderer,
    shared: &Shared,
    encoder: &mut wgpu::CommandEncoder,
    rendered: &[Rendered],
    view: &wgpu::TextureView,
    format: wgpu::TextureFormat,
    clear: Option<Color>,
) -> u32 {
    let device = shared.device();
    let pipeline = r.pipelines.resolve(device, &r.layouts, format);
    for item in rendered {
        let slot = &mut r.slots[item.slot];
        if slot.resolve_group.is_none()
            && let Some(t) = &slot.targets
        {
            slot.resolve_group = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("yowie resolve"),
                layout: &r.layouts.resolve,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&t.color),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&r.linear_sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: slot.resolve.as_entire_binding(),
                    },
                ],
            }));
        }
    }
    let load = match clear {
        Some(c) => {
            let [r, g, b, a] = c.to_linear_premultiplied();
            wgpu::LoadOp::Clear(wgpu::Color {
                r: f64::from(r),
                g: f64::from(g),
                b: f64::from(b),
                a: f64::from(a),
            })
        }
        None => wgpu::LoadOp::Load,
    };
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("yowie resolve"),
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
    pass.set_pipeline(&pipeline);
    let mut calls = 0;
    for item in rendered {
        let Some(group) = &r.slots[item.slot].resolve_group else {
            continue;
        };
        let v = item.viewport;
        pass.set_viewport(v.x, v.y, v.w, v.h, 0.0, 1.0);
        pass.set_bind_group(0, group, &[]);
        pass.draw(0..3, 0..1);
        calls += 1;
    }
    calls
}
