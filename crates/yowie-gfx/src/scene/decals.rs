//! Projected decals: boxes that colour, roughen and bump the surfaces
//! inside them, applied in the surface program before lighting so a decal
//! is lit like the surface it lies on (`shaders/decals.wgsl`).
//!
//! Every decal of a scene is one record in a storage buffer, so decals cost
//! no draw calls. Their images are copied into two atlases (colour and
//! normal), 64 cells of 256 by 256 texels each with five mip levels,
//! kept across frames and refilled least recently used first.

use std::sync::Arc;

use bytemuck::{Pod, Zeroable};
use yowie_core::{Color, Mat4};

use super::submit::{Placement, Scene};
use crate::shared::Shared;
use crate::texture::Texture;
use crate::texture_gpu::TextureGpu;

/// The most decals a scene draws; later ones are dropped.
pub const MAX_DECALS: usize = 256;

impl Scene<'_> {
    /// Projects `decal` onto the surfaces inside the box `placement` makes
    /// of the cube from -0.5 to 0.5. Materials with `receive_decals` off
    /// are left uncovered. A scene draws at most [`MAX_DECALS`] decals.
    pub fn draw_decal(&mut self, decal: &Decal, placement: impl Placement) {
        if self.rec.decals.len() < MAX_DECALS {
            self.rec
                .decals
                .push((decal.clone(), placement.model_matrix()));
        }
    }
}

/// A texture projected onto whatever geometry lies inside a box: bullet
/// holes, blood, footprints, road markings. Draw it with
/// [`Scene::draw_decal`](super::Scene::draw_decal), whose placement maps
/// the cube from -0.5 to 0.5 onto the box. The image projects down the
/// box's Y axis, X running left to right across it and Z from its bottom
/// edge to its top, and fades on surfaces that do not face up the box.
#[derive(Clone, Debug, PartialEq)]
pub struct Decal {
    /// The colour image, sRGB; alpha is how much it covers.
    pub texture: Texture,
    /// Multiplies the image; alpha scales the coverage.
    pub tint: Color,
    /// Replaces the surface's roughness where the decal covers it; `None`
    /// keeps the surface's.
    pub roughness: Option<f32>,
    /// A tangent-space normal map bumping the surface under the decal;
    /// data, not colour.
    pub normal_texture: Option<Texture>,
    /// Scales the normal map's X and Y.
    pub normal_strength: f32,
}

impl Decal {
    /// A decal of `texture`, untinted, keeping the surface's roughness.
    pub fn new(texture: Texture) -> Self {
        Self {
            texture,
            tint: Color::WHITE,
            roughness: None,
            normal_texture: None,
            normal_strength: 1.0,
        }
    }
}

/// One decal as the surface program reads it; matches `Decal` in
/// `shaders/decals.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub(crate) struct GpuDecal {
    inv: [[f32; 4]; 3],
    color_rect: [f32; 4],
    normal_rect: [f32; 4],
    tint: [f32; 4],
    params: [f32; 4],
}

const ATLAS_SIZE: u32 = 2048;
const CELL: u32 = 256;
const CELLS_PER_ROW: u32 = ATLAS_SIZE / CELL;
const CELLS: usize = (CELLS_PER_ROW * CELLS_PER_ROW) as usize;
const LEVELS: u32 = 5;

/// An image held in an atlas cell.
#[derive(Debug)]
struct Cell {
    texture: u64,
    gpu: Arc<TextureGpu>,
    used: u64,
}

/// One atlas: its texture, the view decals sample and the cells.
#[derive(Debug)]
struct Atlas {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    level0: wgpu::TextureView,
    pipeline: wgpu::RenderPipeline,
    cells: Vec<Option<Cell>>,
    /// Cells to fill this frame, with their images.
    pending: Vec<(usize, Arc<TextureGpu>)>,
}

impl Atlas {
    fn new(shared: &Shared, format: wgpu::TextureFormat, label: &'static str) -> Self {
        let device = shared.device();
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width: ATLAS_SIZE,
                height: ATLAS_SIZE,
                depth_or_array_layers: 1,
            },
            mip_level_count: LEVELS,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let module = crate::pipelines::trusted_module(
            device,
            "yowie decal blit",
            include_str!("../shaders/blit.wgsl"),
        );
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("yowie decal blit"),
            bind_group_layouts: &[Some(&shared.layouts.texture)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("yowie decal blit"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(format.into())],
            }),
            multiview_mask: None,
            cache: None,
        });
        Self {
            view: texture.create_view(&Default::default()),
            level0: texture.create_view(&wgpu::TextureViewDescriptor {
                label: Some("yowie decal atlas level 0"),
                mip_level_count: Some(1),
                ..Default::default()
            }),
            texture,
            pipeline,
            cells: (0..CELLS).map(|_| None).collect(),
            pending: Vec::new(),
        }
    }

    /// The atlas rectangle (offset and size in texture coordinates) of
    /// `tex`, placing it in a cell when it has none. `None` when every
    /// cell is taken this frame.
    fn place(&mut self, tex: &Texture, frame: u64) -> Option<[f32; 4]> {
        let id = tex.id();
        let gpu = tex.gpu();
        let found = self.cells.iter().position(|c| {
            c.as_ref()
                .is_some_and(|c| c.texture == id && Arc::ptr_eq(&c.gpu, &gpu))
        });
        let index = match found {
            Some(i) => i,
            None => {
                let free = self.cells.iter().position(Option::is_none).or_else(|| {
                    self.cells
                        .iter()
                        .enumerate()
                        .filter(|(_, c)| c.as_ref().is_some_and(|c| c.used < frame))
                        .min_by_key(|(_, c)| c.as_ref().map_or(0, |c| c.used))
                        .map(|(i, _)| i)
                })?;
                self.pending.push((free, gpu.clone()));
                free
            }
        };
        self.cells[index] = Some(Cell {
            texture: id,
            gpu,
            used: frame,
        });
        let (x, y) = (index as u32 % CELLS_PER_ROW, index as u32 / CELLS_PER_ROW);
        let s = CELL as f32 / ATLAS_SIZE as f32;
        Some([x as f32 * s, y as f32 * s, s, s])
    }

    /// Copies the pending images into their cells and rebuilds the mips.
    fn flush(&mut self, shared: &Shared) {
        if self.pending.is_empty() {
            return;
        }
        let device = shared.device();
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("yowie decal atlas"),
        });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("yowie decal atlas"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.level0,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.pipeline);
            for (index, gpu) in &self.pending {
                let (x, y) = (
                    *index as u32 % CELLS_PER_ROW * CELL,
                    *index as u32 / CELLS_PER_ROW * CELL,
                );
                pass.set_viewport(x as f32, y as f32, CELL as f32, CELL as f32, 0.0, 1.0);
                pass.set_scissor_rect(x, y, CELL, CELL);
                pass.set_bind_group(0, &gpu.bind_group, &[]);
                pass.draw(0..3, 0..1);
            }
        }
        shared.queue().submit([encoder.finish()]);
        shared
            .mipmaps
            .generate(device, shared.queue(), &self.texture);
        self.pending.clear();
    }
}

/// The colour and normal atlases every scene's decals share.
#[derive(Debug)]
pub(crate) struct DecalAtlases {
    color: Atlas,
    normal: Atlas,
}

impl DecalAtlases {
    pub(crate) fn new(shared: &Shared) -> Self {
        Self {
            color: Atlas::new(
                shared,
                wgpu::TextureFormat::Rgba8UnormSrgb,
                "yowie decal colour atlas",
            ),
            normal: Atlas::new(
                shared,
                wgpu::TextureFormat::Rgba8Unorm,
                "yowie decal normal atlas",
            ),
        }
    }

    pub(crate) fn color_view(&self) -> &wgpu::TextureView {
        &self.color.view
    }

    pub(crate) fn normal_view(&self) -> &wgpu::TextureView {
        &self.normal.view
    }

    /// The records of `decals`, placing their images in the atlases and
    /// copying new ones in. Decals that find no cell are left out.
    pub(crate) fn records(
        &mut self,
        shared: &Shared,
        decals: &[(Decal, Mat4)],
        frame: u64,
        out: &mut Vec<GpuDecal>,
    ) {
        out.clear();
        for (d, model) in decals.iter().take(MAX_DECALS) {
            let Some(color_rect) = self.color.place(&d.texture, frame) else {
                continue;
            };
            let normal_rect = match &d.normal_texture {
                Some(t) => self.normal.place(t, frame).unwrap_or_default(),
                None => [0.0; 4],
            };
            let inv = model.inverse().transpose();
            out.push(GpuDecal {
                inv: [
                    inv.x_axis.to_array(),
                    inv.y_axis.to_array(),
                    inv.z_axis.to_array(),
                ],
                color_rect,
                normal_rect,
                tint: d.tint.to_linear(),
                params: [
                    d.roughness.unwrap_or(0.0).clamp(0.0, 1.0),
                    if d.roughness.is_some() { 1.0 } else { 0.0 },
                    d.normal_strength,
                    0.0,
                ],
            });
        }
        self.color.flush(shared);
        self.normal.flush(shared);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_matches_the_shader() {
        assert_eq!(std::mem::size_of::<GpuDecal>(), 7 * 16);
    }
}
