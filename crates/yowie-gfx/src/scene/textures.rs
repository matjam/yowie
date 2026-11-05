//! Material bind groups, cached across frames by their texture ids, and
//! the stand-in textures for slots a material leaves empty.

use std::collections::HashMap;
use std::sync::Arc;

use yowie_core::UVec2;

use super::layouts::{
    CORE_TEXTURES, LAYER_BINDING, LAYER_SAMPLER_BINDING, MATERIAL_TEXTURES, SceneLayouts,
};
use super::material::Material;
use crate::error::Result;
use crate::shared::Shared;
use crate::texture::{Filter, Purpose, Texture, TextureOptions, Wrap};
use crate::texture_gpu::TextureGpu;

/// Frames a bind group may go unused before it is dropped.
const KEEP_FRAMES: u64 = 120;

/// The texture ids of a material's bind group, in binding order.
pub(crate) type TextureKey = [u64; MATERIAL_TEXTURES as usize];

const N: usize = MATERIAL_TEXTURES as usize;

#[derive(Debug)]
struct Entry {
    group: wgpu::BindGroup,
    used: u64,
    /// The texture images the group names, to notice a replaced texture.
    gpus: [Arc<TextureGpu>; N],
}

/// The material bind group cache.
#[derive(Debug)]
pub(crate) struct MaterialTextures {
    /// The stand-in for each slot: white colour or data, or a flat normal.
    defaults: [Texture; N],
    groups: HashMap<TextureKey, Entry>,
    frame: u64,
}

fn solid(shared: &Shared, texel: [u8; 4], format: wgpu::TextureFormat) -> Result<Texture> {
    let opts = TextureOptions {
        filter: Filter::Linear,
        wrap: Wrap::Repeat,
        premultiplied: true,
        ..TextureOptions::default()
    };
    let t = Texture::create(shared, UVec2::ONE, format, opts, Purpose::Sampled)?;
    t.write_level0(shared, 0, 0, 1, 1, &texel, 4);
    Ok(t)
}

impl MaterialTextures {
    pub(crate) fn new(shared: &Shared) -> Result<Self> {
        let srgb = wgpu::TextureFormat::Rgba8UnormSrgb;
        let data = wgpu::TextureFormat::Rgba8Unorm;
        let white = solid(shared, [255; 4], srgb)?;
        let white_data = solid(shared, [255; 4], data)?;
        let normal = solid(shared, [128, 128, 255, 255], data)?;
        let core = [
            white.clone(),
            white_data.clone(),
            normal.clone(),
            white_data.clone(),
            white.clone(),
        ];
        // Clearcoat, coat normal, sheen, specular, transmission,
        // thickness, iridescence, anisotropy, fur.
        let layers = [
            white_data.clone(),
            normal,
            white.clone(),
            white,
            white_data.clone(),
            white_data.clone(),
            white_data.clone(),
            white_data.clone(),
            white_data,
        ];
        let defaults = std::array::from_fn(|i| {
            if i < CORE_TEXTURES as usize {
                core[i].clone()
            } else {
                layers[i - CORE_TEXTURES as usize].clone()
            }
        });
        Ok(Self {
            defaults,
            groups: HashMap::new(),
            frame: 0,
        })
    }

    /// Drops groups that went unused for a while. Called once a frame.
    pub(crate) fn begin_frame(&mut self) {
        self.frame += 1;
        if self.frame.is_multiple_of(60) {
            let frame = self.frame;
            self.groups.retain(|_, e| frame - e.used <= KEEP_FRAMES);
        }
    }

    /// Makes sure the bind group for `material`'s textures exists and
    /// names their current images: a texture replaced in place since the
    /// group was made gets a new group.
    pub(crate) fn prepare(&mut self, shared: &Shared, layouts: &SceneLayouts, material: &Material) {
        let key = material.texture_ids();
        let frame = self.frame;
        let set = material.textures();
        let gpus: [Arc<TextureGpu>; N] =
            std::array::from_fn(|i| set[i].unwrap_or(&self.defaults[i]).gpu());
        if let Some(e) = self.groups.get_mut(&key)
            && e.gpus.iter().zip(&gpus).all(|(a, b)| Arc::ptr_eq(a, b))
        {
            e.used = frame;
            return;
        }
        let core = CORE_TEXTURES as usize;
        let mut entries = Vec::with_capacity(N + core + 1);
        for (i, g) in gpus.iter().enumerate() {
            let binding = if i < core {
                i as u32
            } else {
                LAYER_BINDING + (i - core) as u32
            };
            entries.push(wgpu::BindGroupEntry {
                binding,
                resource: wgpu::BindingResource::TextureView(&g.view),
            });
        }
        for (i, g) in gpus.iter().take(core).enumerate() {
            entries.push(wgpu::BindGroupEntry {
                binding: CORE_TEXTURES + i as u32,
                resource: wgpu::BindingResource::Sampler(shared.samplers.get(&g.options)),
            });
        }
        // The feature maps share one linear, repeating, mipmapped sampler.
        let layer_options = TextureOptions {
            filter: Filter::Linear,
            wrap: Wrap::Repeat,
            mipmaps: true,
            ..TextureOptions::default()
        };
        entries.push(wgpu::BindGroupEntry {
            binding: LAYER_SAMPLER_BINDING,
            resource: wgpu::BindingResource::Sampler(shared.samplers.get(&layer_options)),
        });
        let group = shared
            .device()
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("yowie material"),
                layout: &layouts.material,
                entries: &entries,
            });
        self.groups.insert(
            key,
            Entry {
                group,
                used: frame,
                gpus,
            },
        );
    }

    /// The bind group for a texture key prepared this frame.
    pub(crate) fn group(&self, key: &TextureKey) -> Option<&wgpu::BindGroup> {
        self.groups.get(key).map(|e| &e.group)
    }
}
