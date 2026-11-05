//! Baking an impostor: the model drawn once per view into the cells of an
//! albedo atlas and a normal atlas, in one pass through the scene
//! renderer's preparation and buffers with the bake pipelines.
//!
//! One orthographic camera looks down -Z at the whole atlas. Each view's
//! copy of the model sits at its cell's centre, turned so that the
//! direction the view looks from faces the camera, so the world frame of
//! the bake is each view's own frame.

use yowie_core::{Mat4, Quat, UVec2, Vec3};

use super::{Impostor, ImpostorOptions, MAX_IMPOSTOR_VIEWS, cell_center, cell_size};
use crate::Graphics;
use crate::error::{GfxError, Result};
use crate::scene::billboard::AtlasGrid;
use crate::scene::camera::Camera3D;
use crate::scene::frame_data::fill_frame;
use crate::scene::layouts::DEPTH_FORMAT;
use crate::scene::model::ModelInstance;
use crate::scene::prepare::{Prepared, View, prepare};
use crate::scene::record::SceneRecord;
use crate::scene::render::SceneRenderer;
use crate::scene::slot::{SceneSlot, SlotId};
use crate::scene::submit::Scene;
use crate::shared::Shared;
use crate::texture::{Filter, Purpose, Texture, TextureOptions};

fn atlas_texture(
    shared: &Shared,
    size: UVec2,
    format: wgpu::TextureFormat,
    data: bool,
) -> Result<Texture> {
    Texture::create(
        shared,
        size,
        format,
        TextureOptions {
            filter: Filter::Linear,
            mipmaps: true,
            premultiplied: true,
            data,
            ..TextureOptions::default()
        },
        Purpose::RenderTarget,
    )
}

fn level0(t: &Texture) -> wgpu::TextureView {
    t.gpu().texture.create_view(&wgpu::TextureViewDescriptor {
        label: Some("yowie impostor bake target"),
        base_mip_level: 0,
        mip_level_count: Some(1),
        ..Default::default()
    })
}

pub(super) fn bake(
    gfx: &mut Graphics,
    instance: &ModelInstance,
    o: &ImpostorOptions,
) -> Result<Impostor> {
    let views = o.views.clamp(1, MAX_IMPOSTOR_VIEWS);
    let res = o.resolution.clamp(8, 2048);
    let cols = (views as f32).sqrt().ceil() as u32;
    let rows = views.div_ceil(cols);
    let bounds = instance.model().bounds();
    if instance.model().parts().is_empty() || bounds.is_empty() {
        return Err(GfxError::Terrain(
            "an impostor needs a model with geometry".into(),
        ));
    }
    let size = UVec2::new(cols * res, rows * res);
    let max = gfx.shared.device().limits().max_texture_dimension_2d;
    if size.x > max || size.y > max {
        return Err(GfxError::Terrain(format!(
            "an impostor atlas of {}x{} is larger than the device's {max}",
            size.x, size.y
        )));
    }
    let center = bounds.center();
    let radius = bounds.radius().max(1e-4);
    let cell = cell_size(radius);
    let (w, h) = (cols as f32 * cell, rows as f32 * cell);
    let mut camera = Camera3D::orthographic(
        Vec3::new(w * 0.5, h * 0.5, radius * 4.0),
        Vec3::new(w * 0.5, h * 0.5, 0.0),
        h * 0.5,
    );
    camera.near = radius;
    camera.far = radius * 7.0;
    let Graphics { shared, scenes, .. } = gfx;
    let albedo = atlas_texture(shared, size, wgpu::TextureFormat::Rgba8UnormSrgb, false)?;
    let normal = atlas_texture(shared, size, wgpu::TextureFormat::Rgba8Unorm, true)?;
    let renderer = scenes.get_or_insert_with(|| Box::new(SceneRenderer::new(shared)));
    let mut rec = renderer.take_record();
    rec.reset(usize::MAX, camera, size.as_vec2());
    rec.background = false;
    {
        let mut s = Scene { rec: &mut rec };
        for k in 0..views {
            let theta = std::f32::consts::TAU * k as f32 / views as f32;
            let at = cell_center(k, cols, rows, cell);
            let turn = Quat::from_rotation_x(o.pitch) * Quat::from_rotation_y(-theta);
            let m = Mat4::from_translation(at.extend(0.0))
                * Mat4::from_quat(turn)
                * Mat4::from_translation(-center);
            s.draw_model(instance, m);
        }
    }
    renderer.bake_atlas(shared, rec, size, [&level0(&albedo), &level0(&normal)]);
    for t in [&albedo, &normal] {
        shared
            .mipmaps
            .generate(shared.device(), shared.queue(), &t.gpu().texture);
    }
    Ok(Impostor {
        albedo,
        normal,
        grid: AtlasGrid {
            views,
            cols,
            rows,
            pitch: o.pitch,
        },
        center,
        size: cell,
        roughness: o.roughness,
        distance: 30.0,
        fade: 4.0,
    })
}

impl SceneRenderer {
    /// Renders `rec` into the bake targets (albedo, normal) of `size`
    /// pixels and submits the work. The record takes the next slot for
    /// the length of the call.
    pub(crate) fn bake_atlas(
        &mut self,
        shared: &Shared,
        rec: SceneRecord,
        size: UVec2,
        targets: [&wgpu::TextureView; 2],
    ) {
        let device = shared.device();
        let (bake_static, bake_skinned) = {
            let l = self.large_renderer(shared);
            (
                l.pipelines.bake_static.clone(),
                l.pipelines.bake_skinned.clone(),
            )
        };
        let ri = self.records.len();
        self.records.push(rec);
        if self.slots.len() <= ri {
            self.slots.resize_with(ri + 1, || SceneSlot::new(device));
        }
        let v = View::new(&self.records[ri].camera, size.as_vec2());
        let mut p = Prepared::default();
        prepare(&self.records[ri], &v, &mut p);
        fill_frame(&self.records[ri], &v, &mut p.frame);
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("yowie impostor bake"),
        });
        let slot = SlotId::Scene(ri);
        let grid = self.write_probes(shared, &mut encoder, ri, slot, None);
        self.upload(shared, ri, slot, &p);
        self.bind_groups(shared, ri, slot, 0, grid, false);
        let rec = &self.records[ri];
        for m in &rec.materials {
            self.textures.prepare(shared, &self.layouts, m);
        }
        for mesh in &rec.meshes {
            if let Some(morph) = &mesh.inner.morph {
                morph.bind_group.get_or_init(|| {
                    device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("yowie morph"),
                        layout: &self.layouts.morph,
                        entries: &[wgpu::BindGroupEntry {
                            binding: 0,
                            resource: morph.buffer.as_entire_binding(),
                        }],
                    })
                });
            }
        }
        let depth = device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("yowie impostor bake depth"),
                size: wgpu::Extent3d {
                    width: size.x,
                    height: size.y,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: DEPTH_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            })
            .create_view(&Default::default());
        if let Some((_, frame_group, _)) = self.slots[ri].groups.clone() {
            let color = |view| {
                Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })
            };
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("yowie impostor bake"),
                color_attachments: &[color(targets[0]), color(targets[1])],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(0.0),
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_bind_group(0, &frame_group, &[]);
            pass.set_vertex_buffer(1, self.slots[ri].instances.buffer.slice(..));
            let mut skinned = None;
            for b in p.opaque.iter().chain(&p.blend) {
                if skinned != Some(b.key.skinned) {
                    pass.set_pipeline(if b.key.skinned {
                        &bake_skinned
                    } else {
                        &bake_static
                    });
                    skinned = Some(b.key.skinned);
                }
                let material = &rec.materials[b.material as usize];
                let Some(group) = self.textures.group(&material.texture_ids()) else {
                    continue;
                };
                pass.set_bind_group(1, group, &[]);
                let mesh = &rec.meshes[b.mesh as usize];
                let gpu = mesh.inner.gpu.lock();
                pass.set_vertex_buffer(0, gpu.vertices.slice(..));
                pass.set_index_buffer(gpu.indices.slice(..), wgpu::IndexFormat::Uint32);
                if let Some(skin) = &gpu.skin {
                    pass.set_vertex_buffer(2, skin.slice(..));
                }
                let morph = mesh
                    .inner
                    .morph
                    .as_ref()
                    .and_then(|m| m.bind_group.get())
                    .unwrap_or(&self.morph_none);
                pass.set_bind_group(2, morph, &[]);
                pass.draw_indexed(0..gpu.index_count, 0, b.first..b.first + b.count);
            }
        }
        shared.queue().submit([encoder.finish()]);
        self.records.pop();
    }
}
