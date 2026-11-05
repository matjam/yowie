//! A terrain's GPU side: the height buffer, the normal and splat
//! textures, the shading parameters, one index buffer per level, and the
//! bind group the renderer builds over them.

use std::sync::Arc;

use bytemuck::{Pod, Zeroable};
use parking_lot::Mutex;
use yowie_core::UVec2;

use super::heightmap::HeightMap;
use super::{TerrainInner, TerrainState};
use crate::shared::Shared;
use crate::texture::{Filter, Purpose, Texture, TextureOptions, Wrap};
use crate::texture_gpu::TextureGpu;

/// The terrain block the shaders read; matches `TerrainParams` in
/// `shaders/terrain.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub(crate) struct TerrainParamsGpu {
    /// Columns, rows, chunk size, flags (1: triplanar).
    pub(crate) grid: [u32; 4],
    /// World position of sample (0, 0) and the cell size.
    pub(crate) origin: [f32; 4],
    /// One over each layer's repeat size.
    pub(crate) layer_scale: [f32; 4],
    pub(crate) layer_rough: [f32; 4],
    /// Linear tint per layer.
    pub(crate) layer_color: [[f32; 4]; 4],
}

/// What a terrain bind group was built from: the layer images and the
/// state generation.
pub(crate) type GroupKey = ([usize; 4], u64);

#[derive(Debug)]
pub(crate) struct TerrainGpu {
    pub(crate) heights: wgpu::Buffer,
    pub(crate) normals: wgpu::Texture,
    pub(crate) splat: wgpu::Texture,
    pub(crate) params: wgpu::Buffer,
    /// One index buffer and its index count per level.
    pub(crate) indices: Vec<(wgpu::Buffer, u32)>,
    /// Stands in for a layer without a texture.
    pub(crate) white: Texture,
    pub(crate) group: Mutex<Option<(GroupKey, wgpu::BindGroup)>>,
}

fn data_texture(shared: &Shared, label: &str, cols: u32, rows: u32) -> wgpu::Texture {
    shared.device().create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: cols,
            height: rows,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}

/// Writes four bytes per texel of the inclusive rectangle `rect` from
/// `data`, which covers the whole `cols`-wide grid.
fn write_rect(shared: &Shared, tex: &wgpu::Texture, cols: u32, data: &[u8], rect: [u32; 4]) {
    let [x0, z0, x1, z1] = rect;
    let (w, h) = (x1 - x0 + 1, z1 - z0 + 1);
    let mut bytes = Vec::with_capacity((w * h * 4) as usize);
    for z in z0..=z1 {
        let at = ((z * cols + x0) * 4) as usize;
        bytes.extend_from_slice(&data[at..at + (w * 4) as usize]);
    }
    shared.queue().write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: tex,
            mip_level: 0,
            origin: wgpu::Origin3d { x: x0, y: z0, z: 0 },
            aspect: wgpu::TextureAspect::All,
        },
        &bytes,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(w * 4),
            rows_per_image: Some(h),
        },
        wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
    );
}

/// The indices of one chunk at `level`: the grid of `n` by `n` vertices,
/// then a skirt hanging from each edge. Vertex `j * n + i` is grid vertex
/// `(i, j)`; vertex `n * n + e * n + t` is the dropped copy of vertex `t`
/// along edge `e` (0: -Z, 1: +Z, 2: -X, 3: +X). Cells split along the
/// diagonal from their (i, j + 1) corner to their (i + 1, j) corner, and
/// every triangle faces up or outwards.
pub(crate) fn chunk_indices(chunk: u32, level: u32) -> Vec<u32> {
    let n = chunk / (1 << level) + 1;
    let mut idx = Vec::with_capacity(((n - 1) * (n - 1) * 6 + 4 * (n - 1) * 6) as usize);
    for j in 0..n - 1 {
        for i in 0..n - 1 {
            let a = j * n + i;
            let (b, c, e) = (a + 1, a + n, a + n + 1);
            idx.extend_from_slice(&[a, c, b, c, e, b]);
        }
    }
    let border = |edge: u32, t: u32| match edge {
        0 => t,
        1 => (n - 1) * n + t,
        2 => t * n,
        _ => t * n + n - 1,
    };
    for edge in 0..4 {
        let base = n * n + edge * n;
        // The -Z and +X skirts wind one way and the +Z and -X ones the
        // other, so each faces away from the chunk.
        let flip = edge == 1 || edge == 2;
        for t in 0..n - 1 {
            let (a, b) = (border(edge, t), border(edge, t + 1));
            let (c0, c1) = (base + t, base + t + 1);
            if flip {
                idx.extend_from_slice(&[a, c0, b, b, c0, c1]);
            } else {
                idx.extend_from_slice(&[a, b, c0, b, c1, c0]);
            }
        }
    }
    idx
}

impl TerrainGpu {
    pub(crate) fn new(
        shared: &Shared,
        map: &HeightMap,
        cell: f32,
        chunk: u32,
        levels: u32,
        splat: &[u8],
    ) -> Self {
        let device = shared.device();
        let (cols, rows) = (map.cols(), map.rows());
        let heights = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("yowie terrain heights"),
            size: (map.heights().len() * 4) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        shared
            .queue()
            .write_buffer(&heights, 0, bytemuck::cast_slice(map.heights()));
        let normals = data_texture(shared, "yowie terrain normals", cols, rows);
        let splat_tex = data_texture(shared, "yowie terrain splat", cols, rows);
        let params = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("yowie terrain params"),
            size: std::mem::size_of::<TerrainParamsGpu>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let indices = (0..levels)
            .map(|l| {
                let idx = chunk_indices(chunk, l);
                let buf = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("yowie terrain indices"),
                    size: (idx.len() * 4) as u64,
                    usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                shared
                    .queue()
                    .write_buffer(&buf, 0, bytemuck::cast_slice(&idx));
                (buf, idx.len() as u32)
            })
            .collect();
        let white_opts = TextureOptions {
            filter: Filter::Linear,
            wrap: Wrap::Repeat,
            premultiplied: true,
            ..TextureOptions::default()
        };
        // A 1x1 texture is within every device's limits.
        let white = match Texture::create(
            shared,
            UVec2::ONE,
            wgpu::TextureFormat::Rgba8UnormSrgb,
            white_opts,
            Purpose::Sampled,
        ) {
            Ok(t) => t,
            Err(e) => unreachable!("1x1 texture: {e}"),
        };
        white.write_level0(shared, 0, 0, 1, 1, &[255; 4], 4);
        let gpu = Self {
            heights,
            normals,
            splat: splat_tex,
            params,
            indices,
            white,
            group: Mutex::new(None),
        };
        let all = [0, 0, cols - 1, rows - 1];
        gpu.write_normals(shared, map, cell, all);
        gpu.write_splat(shared, cols, splat, all);
        gpu
    }

    /// Uploads rows `z0..=z1` of the heights.
    pub(crate) fn write_heights(&self, shared: &Shared, map: &HeightMap, z0: u32, z1: u32) {
        let cols = map.cols() as usize;
        let start = z0 as usize * cols;
        let end = (z1 as usize + 1) * cols;
        shared.queue().write_buffer(
            &self.heights,
            (start * 4) as u64,
            bytemuck::cast_slice(&map.heights()[start..end]),
        );
    }

    /// Recomputes and uploads the normals of the inclusive rectangle.
    pub(crate) fn write_normals(
        &self,
        shared: &Shared,
        map: &HeightMap,
        cell: f32,
        rect: [u32; 4],
    ) {
        let cols = map.cols();
        let mut data = vec![0u8; (cols * map.rows() * 4) as usize];
        for z in rect[1]..=rect[3] {
            for x in rect[0]..=rect[2] {
                let n = map.normal(i64::from(x), i64::from(z), cell);
                let at = ((z * cols + x) * 4) as usize;
                for (c, v) in [n.x, n.y, n.z].into_iter().enumerate() {
                    data[at + c] = ((v * 0.5 + 0.5) * 255.0 + 0.5) as u8;
                }
                data[at + 3] = 255;
            }
        }
        write_rect(shared, &self.normals, cols, &data, rect);
    }

    /// Uploads the splat weights of the inclusive rectangle.
    pub(crate) fn write_splat(&self, shared: &Shared, cols: u32, splat: &[u8], rect: [u32; 4]) {
        write_rect(shared, &self.splat, cols, splat, rect);
    }

    /// Writes the shading parameters from the terrain's layout and
    /// layers.
    pub(crate) fn write_params(&self, shared: &Shared, t: &TerrainInner, state: &TerrainState) {
        let mut p = TerrainParamsGpu {
            grid: [t.cols, t.rows, t.chunk, u32::from(t.triplanar)],
            origin: [t.origin.x, t.origin.y, t.origin.z, t.cell],
            ..TerrainParamsGpu::default()
        };
        for (i, l) in state.layers.iter().enumerate() {
            p.layer_scale[i] = 1.0 / l.scale.max(1e-3);
            p.layer_rough[i] = l.roughness.clamp(0.03, 1.0);
            p.layer_color[i] = l.color.to_linear();
        }
        shared
            .queue()
            .write_buffer(&self.params, 0, bytemuck::bytes_of(&p));
    }

    /// The images of the four layers, a missing one replaced by white.
    pub(crate) fn layer_images(&self, state: &TerrainState) -> [Arc<TextureGpu>; 4] {
        std::array::from_fn(|i| {
            state.layers[i]
                .texture
                .as_ref()
                .unwrap_or(&self.white)
                .gpu()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunk_triangles_face_up_and_skirts_face_out() {
        // Build the positions the vertex program builds for a flat chunk
        // of 4 samples at level 1 (3 by 3 vertices) and check every
        // triangle's facing.
        let (chunk, level) = (4, 1);
        let n = chunk / (1 << level) + 1;
        let pos = |v: u32| -> yowie_core::Vec3 {
            if v < n * n {
                yowie_core::Vec3::new((v % n) as f32, 0.0, (v / n) as f32)
            } else {
                let k = v - n * n;
                let (edge, t) = (k / n, k % n);
                let (i, j) = match edge {
                    0 => (t, 0),
                    1 => (t, n - 1),
                    2 => (0, t),
                    _ => (n - 1, t),
                };
                yowie_core::Vec3::new(i as f32, -1.0, j as f32)
            }
        };
        let idx = chunk_indices(chunk, level);
        assert_eq!(idx.len() as u32, (n - 1) * (n - 1) * 6 + 4 * (n - 1) * 6);
        let centre = yowie_core::Vec3::new((n - 1) as f32 * 0.5, 0.0, (n - 1) as f32 * 0.5);
        for (k, t) in idx.as_chunks::<3>().0.iter().enumerate() {
            let [a, b, c] = t.map(pos);
            let normal = (b - a).cross(c - a);
            if k < ((n - 1) * (n - 1) * 2) as usize {
                assert!(normal.y > 0.0, "grid triangle {k} faces down");
            } else {
                let mid = (a + b + c) / 3.0 - centre;
                let out = yowie_core::Vec3::new(mid.x, 0.0, mid.z);
                assert!(normal.dot(out) > 0.0, "skirt triangle {k} faces in");
            }
        }
    }
}
