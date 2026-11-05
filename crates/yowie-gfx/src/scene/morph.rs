//! Morph targets: per-vertex offsets the vertex program blends by weight.
//!
//! A morphed mesh uploads every target's position and normal deltas into
//! one storage buffer (six floats per vertex per target, after a
//! four-word header holding the vertex and target counts), bound as group
//! 2 of the mesh pipelines. A draw names up to [`MAX_MORPH_WEIGHTS`]
//! targets with their weights in the frame's weight buffer; the instance
//! stream carries the first weight and the count.

use std::sync::OnceLock;

use yowie_core::Vec3;

use super::bounds::Aabb;
use super::vertex::MeshData;
use crate::error::{GfxError, Result};
use crate::shared::Shared;

/// The most morph targets one draw blends; further non-zero weights are
/// dropped, smallest first.
pub const MAX_MORPH_WEIGHTS: usize = 8;

/// One morph target: offsets added to each vertex's position and normal
/// at full weight. Both lists have one entry per vertex; `normals` may be
/// empty.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MorphTarget {
    /// Position offsets in mesh space.
    pub positions: Vec<Vec3>,
    /// Normal offsets, or empty for none.
    pub normals: Vec<Vec3>,
}

#[derive(Debug)]
pub(crate) struct MorphBuffer {
    pub(crate) buffer: wgpu::Buffer,
    pub(crate) vertex_count: usize,
    pub(crate) target_count: usize,
    /// The largest offset each target moves any vertex, per axis.
    reach: Vec3,
    pub(crate) bind_group: OnceLock<wgpu::BindGroup>,
}

impl MorphBuffer {
    pub(crate) fn new(
        shared: &Shared,
        vertex_count: usize,
        targets: &[MorphTarget],
    ) -> Result<Self> {
        let mut data: Vec<f32> = Vec::with_capacity(4 + targets.len() * vertex_count * 6);
        data.extend_from_slice(&[
            f32::from_bits(vertex_count as u32),
            f32::from_bits(targets.len() as u32),
            0.0,
            0.0,
        ]);
        let mut reach = Vec3::ZERO;
        for (i, t) in targets.iter().enumerate() {
            if t.positions.len() != vertex_count
                || !(t.normals.is_empty() || t.normals.len() == vertex_count)
            {
                return Err(GfxError::Mesh(format!(
                    "morph target {i} has {} offsets for {vertex_count} vertices",
                    t.positions.len()
                )));
            }
            let mut target_reach = Vec3::ZERO;
            for v in 0..vertex_count {
                let p = t.positions[v];
                let n = t.normals.get(v).copied().unwrap_or(Vec3::ZERO);
                target_reach = target_reach.max(p.abs());
                data.extend_from_slice(&[p.x, p.y, p.z, n.x, n.y, n.z]);
            }
            reach += target_reach;
        }
        let buffer = shared.device().create_buffer(&wgpu::BufferDescriptor {
            label: Some("yowie morph targets"),
            size: (data.len() * 4) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        shared
            .queue()
            .write_buffer(&buffer, 0, bytemuck::cast_slice(&data));
        Ok(Self {
            buffer,
            vertex_count,
            target_count: targets.len(),
            reach,
            bind_group: OnceLock::new(),
        })
    }

    /// The rest bounds grown by every target at full weight at once, which
    /// bounds any blend with weights up to one.
    pub(crate) fn reach(&self, data: &MeshData) -> Aabb {
        let b = data.bounds();
        Aabb::new(b.min - self.reach, b.max + self.reach)
    }
}

/// One weight in the frame's weight buffer: a target index and its weight.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct GpuMorphWeight {
    pub(crate) target: u32,
    pub(crate) weight: f32,
}

/// Picks the largest non-zero weights, at most [`MAX_MORPH_WEIGHTS`] of
/// them, appending them to `out`. Returns how many were appended.
pub(crate) fn pick_weights(weights: &[f32], targets: usize, out: &mut Vec<GpuMorphWeight>) -> u32 {
    let mut picked: Vec<GpuMorphWeight> = weights
        .iter()
        .take(targets)
        .enumerate()
        .filter(|(_, w)| w.abs() > 1e-4)
        .map(|(i, w)| GpuMorphWeight {
            target: i as u32,
            weight: *w,
        })
        .collect();
    if picked.len() > MAX_MORPH_WEIGHTS {
        picked.sort_by(|a, b| b.weight.abs().total_cmp(&a.weight.abs()));
        picked.truncate(MAX_MORPH_WEIGHTS);
    }
    out.extend_from_slice(&picked);
    picked.len() as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weights_keep_the_largest() {
        let mut out = Vec::new();
        let w = [0.0, 0.5, 0.1, 0.9, 0.2, 0.3, 0.4, 0.6, 0.7, 0.05, 0.8];
        let n = pick_weights(&w, w.len(), &mut out);
        assert_eq!(n as usize, MAX_MORPH_WEIGHTS);
        assert!(out.iter().all(|m| m.weight >= 0.2));
        let mut out = Vec::new();
        assert_eq!(
            pick_weights(&[0.0, 1.0], 1, &mut out),
            0,
            "past the target count"
        );
    }
}
