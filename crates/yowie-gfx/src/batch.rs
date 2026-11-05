//! The 2D draw stream: vertices, indices and the items that order them.
//!
//! Every 2D draw appends triangles to the [`DrawList`] of the current
//! output (the screen or a render texture) as an [`Item`] carrying its
//! layer, sort key and [`DrawState`]. Consecutive draws with the same layer,
//! key and state extend one item, so a run of sprites from one atlas costs
//! one item. At the end of the frame the items are sorted by layer and then
//! sort key, keeping submission order for ties, and neighbouring items with
//! the same state become one draw call.

use std::collections::HashMap;

use yowie_core::{Color, Rect};

use crate::blend::BlendMode;
use crate::custom::CustomEntry;
use crate::pipelines::{Vertex2D, ViewUniform};
use crate::shader::Shader;
use crate::texture::Texture;
use crate::view::ViewMapping;

/// A shader as one draw saw it: its program and uniform values at the
/// moment of the draw.
#[derive(Debug)]
pub(crate) struct ShaderUse {
    pub(crate) id: u64,
    pub(crate) generation: u32,
    pub(crate) module: wgpu::ShaderModule,
    pub(crate) uniforms: Vec<u8>,
}

/// The transform and clipping one stretch of drawing used.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ViewEntry {
    pub(crate) uniform: ViewUniform,
    /// The GPU viewport in output pixels.
    pub(crate) viewport: Rect,
    /// The scissor rectangle in output pixels: x, y, width, height.
    pub(crate) scissor: [u32; 4],
}

/// Everything the frame's draws reference, by slot.
#[derive(Debug, Default)]
pub(crate) struct FrameTables {
    pub(crate) textures: Vec<Texture>,
    texture_slots: HashMap<u64, u32>,
    pub(crate) shaders: Vec<ShaderUse>,
    shader_slots: HashMap<(u64, u32, u32), u32>,
    pub(crate) views: Vec<ViewEntry>,
    pub(crate) customs: Vec<CustomEntry>,
}

impl FrameTables {
    pub(crate) fn clear(&mut self) {
        self.textures.clear();
        self.texture_slots.clear();
        self.shaders.clear();
        self.shader_slots.clear();
        self.views.clear();
        self.customs.clear();
    }

    pub(crate) fn push_custom(&mut self, draw: std::sync::Arc<dyn crate::GpuDraw>) -> u32 {
        self.customs.push(CustomEntry(draw));
        (self.customs.len() - 1) as u32
    }

    pub(crate) fn texture_slot(&mut self, texture: &Texture) -> u32 {
        let next = self.textures.len() as u32;
        let slot = *self.texture_slots.entry(texture.id()).or_insert(next);
        if slot == next {
            self.textures.push(texture.clone());
        }
        slot
    }

    pub(crate) fn shader_slot(&mut self, shader: &Shader) -> u32 {
        let state = shader.inner.state.lock();
        let key = (shader.inner.id, state.generation, state.uniform_version);
        let next = self.shaders.len() as u32;
        let slot = *self.shader_slots.entry(key).or_insert(next);
        if slot == next {
            self.shaders.push(ShaderUse {
                id: shader.inner.id,
                generation: state.generation,
                module: state.module.clone(),
                uniforms: state.uniforms.clone(),
            });
        }
        slot
    }

    pub(crate) fn push_view(&mut self, view: ViewEntry) -> u32 {
        self.views.push(view);
        (self.views.len() - 1) as u32
    }
}

/// The engine's default shader in a [`DrawState`].
pub(crate) const DEFAULT_SHADER: u32 = u32::MAX;
/// The engine's signed-distance-field text shader in a [`DrawState`].
pub(crate) const SDF_SHADER: u32 = u32::MAX - 1;
/// The engine's radial-gradient shader in a [`DrawState`].
pub(crate) const RADIAL_SHADER: u32 = u32::MAX - 2;

/// A custom draw queued with `Graphics::draw_gpu`; the state's texture
/// slot indexes the frame's custom table.
pub(crate) const CUSTOM_SHADER: u32 = u32::MAX - 3;
/// A run of lit sprites; the state's texture slot indexes the frame's lit
/// runs.
pub(crate) const LIT_SHADER: u32 = u32::MAX - 4;

/// Whether `slot` names one of the engine's own shaders rather than an
/// entry of the frame's game shader table.
pub(crate) fn is_engine_shader(slot: u32) -> bool {
    slot >= u32::MAX - 15
}

/// What a draw call binds: texture, shader, blend and view, by slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct DrawState {
    pub(crate) texture: u32,
    pub(crate) shader: u32,
    pub(crate) blend: BlendMode,
    pub(crate) view: u32,
}

/// A run of indices drawn with one state at one layer and sort key.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Item {
    pub(crate) layer: i32,
    pub(crate) key: f32,
    pub(crate) state: DrawState,
    pub(crate) first: u32,
    pub(crate) count: u32,
}

/// Where a draw list renders.
#[derive(Clone, Debug)]
pub(crate) enum ListTarget {
    /// The frame's output: the window or the headless image.
    Screen,
    /// A render texture.
    Texture(Texture),
}

/// The 2D drawing for one output in one frame.
#[derive(Debug)]
pub(crate) struct DrawList {
    pub(crate) target: ListTarget,
    pub(crate) mapping: ViewMapping,
    pub(crate) clear: Option<Color>,
    pub(crate) vertices: Vec<Vertex2D>,
    pub(crate) indices: Vec<u32>,
    pub(crate) items: Vec<Item>,
}

/// One draw call after sorting and merging.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct DrawCall {
    pub(crate) state: DrawState,
    pub(crate) first: u32,
    pub(crate) count: u32,
}

impl DrawList {
    pub(crate) fn new(target: ListTarget, mapping: ViewMapping, clear: Option<Color>) -> Self {
        Self {
            target,
            mapping,
            clear,
            vertices: Vec::new(),
            indices: Vec::new(),
            items: Vec::new(),
        }
    }

    /// Appends triangles; `indices` count from the first of `vertices`.
    pub(crate) fn push(
        &mut self,
        state: DrawState,
        layer: i32,
        key: f32,
        vertices: &[Vertex2D],
        indices: &[u32],
    ) {
        if indices.is_empty() {
            return;
        }
        let base = self.vertices.len() as u32;
        self.vertices.extend_from_slice(vertices);
        let first = self.indices.len() as u32;
        self.indices.extend(indices.iter().map(|i| i + base));
        let count = indices.len() as u32;
        if let Some(last) = self.items.last_mut()
            && last.state == state
            && last.layer == layer
            && last.key.to_bits() == key.to_bits()
            && last.first + last.count == first
        {
            last.count += count;
            return;
        }
        self.items.push(Item {
            layer,
            key,
            state,
            first,
            count,
        });
    }

    /// Appends an item with no triangles, drawn by code the renderer calls
    /// out to. Its state must be unique to the item so it never merges.
    pub(crate) fn push_marker(&mut self, state: DrawState, layer: i32, key: f32) {
        self.items.push(Item {
            layer,
            key,
            state,
            first: self.indices.len() as u32,
            count: 0,
        });
    }

    /// Sorts the items and merges them into draw calls, appending the
    /// sorted indices to `out` (offset by `base_vertex`) and returning the
    /// calls with index ranges into `out`.
    pub(crate) fn flatten(&mut self, out: &mut Vec<u32>, base_vertex: u32) -> Vec<DrawCall> {
        let order = |a: &Item, b: &Item| a.layer.cmp(&b.layer).then(a.key.total_cmp(&b.key));
        if !self.items.is_sorted_by(|a, b| order(a, b).is_le()) {
            // A stable sort keeps submission order between equal keys.
            self.items.sort_by(order);
        }
        let mut calls: Vec<DrawCall> = Vec::new();
        for item in &self.items {
            let first = out.len() as u32;
            let range = item.first as usize..(item.first + item.count) as usize;
            out.extend(self.indices[range].iter().map(|i| i + base_vertex));
            match calls.last_mut() {
                Some(c) if c.state == item.state && c.first + c.count == first => {
                    c.count += item.count;
                }
                _ => calls.push(DrawCall {
                    state: item.state,
                    first,
                    count: item.count,
                }),
            }
        }
        calls
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(texture: u32) -> DrawState {
        DrawState {
            texture,
            shader: DEFAULT_SHADER,
            blend: BlendMode::Alpha,
            view: 0,
        }
    }

    fn quad(list: &mut DrawList, texture: u32, layer: i32, key: f32) {
        let v = [Vertex2D::default(); 4];
        list.push(state(texture), layer, key, &v, &[0, 1, 2, 0, 2, 3]);
    }

    #[test]
    fn runs_merge_and_layers_sort() {
        let mut list = DrawList::new(ListTarget::Screen, ViewMapping::default(), None);
        quad(&mut list, 0, 1, 0.0);
        quad(&mut list, 0, 1, 0.0);
        assert_eq!(list.items.len(), 1, "same state extends the item");
        quad(&mut list, 1, 0, 0.0);
        quad(&mut list, 0, 1, 0.0);
        let mut out = Vec::new();
        let calls = list.flatten(&mut out, 0);
        // Layer 0 (texture 1) first, then both layer-1 runs of texture 0 as
        // one call.
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].state.texture, 1);
        assert_eq!(calls[1].count, 18);
        assert_eq!(out.len(), 24);
        assert_eq!(&out[..3], &[8, 9, 10], "the layer-0 quad's vertices");
    }

    #[test]
    fn sort_keys_order_within_a_layer() {
        let mut list = DrawList::new(ListTarget::Screen, ViewMapping::default(), None);
        quad(&mut list, 0, 0, 5.0);
        quad(&mut list, 1, 0, 2.0);
        quad(&mut list, 2, 0, 2.0);
        let mut out = Vec::new();
        let calls = list.flatten(&mut out, 100);
        let textures: Vec<u32> = calls.iter().map(|c| c.state.texture).collect();
        assert_eq!(textures, [1, 2, 0], "ties keep submission order");
        assert_eq!(out[0], 104);
    }
}
