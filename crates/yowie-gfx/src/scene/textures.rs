//! Material bind groups, cached across frames by their texture ids, and
//! the stand-in textures for slots a material leaves empty.

use std::collections::HashMap;
use std::sync::Arc;

use yowie_core::UVec2;

use super::layouts::{MATERIAL_TEXTURES, SceneLayouts};
use super::material::Material;
use crate::error::Result;
use crate::shared::Shared;
use crate::texture::{Filter, Purpose, Texture, TextureOptions, Wrap};
use crate::texture_gpu::TextureGpu;

/// Frames a bind group may go unused before it is dropped.
const KEEP_FRAMES: u64 = 120;

#[derive(Debug)]
struct Entry {
    group: wgpu::BindGroup,
    used: u64,
    /// The texture images the group names, to notice a replaced texture.
    gpus: [Arc<TextureGpu>; MATERIAL_TEXTURES as usize],
}

/// The material bind group cache.
#[derive(Debug)]
pub(crate) struct MaterialTextures {
    /// White colour, white data and a flat normal, in slot order.
    defaults: [Texture; MATERIAL_TEXTURES as usize],
    groups: HashMap<[u64; 5], Entry>,
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
        Ok(Self {
            defaults: [white.clone(), white_data.clone(), normal, white_data, white],
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
        let gpus: [Arc<TextureGpu>; MATERIAL_TEXTURES as usize] =
            std::array::from_fn(|i| set[i].unwrap_or(&self.defaults[i]).gpu());
        if let Some(e) = self.groups.get_mut(&key)
            && e.gpus.iter().zip(&gpus).all(|(a, b)| Arc::ptr_eq(a, b))
        {
            e.used = frame;
            return;
        }
        let mut entries = Vec::with_capacity(gpus.len() * 2);
        for (i, g) in gpus.iter().enumerate() {
            entries.push(wgpu::BindGroupEntry {
                binding: i as u32,
                resource: wgpu::BindingResource::TextureView(&g.view),
            });
        }
        for (i, g) in gpus.iter().enumerate() {
            entries.push(wgpu::BindGroupEntry {
                binding: MATERIAL_TEXTURES + i as u32,
                resource: wgpu::BindingResource::Sampler(shared.samplers.get(&g.options)),
            });
        }
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
    pub(crate) fn group(&self, key: &[u64; 5]) -> Option<&wgpu::BindGroup> {
        self.groups.get(key).map(|e| &e.group)
    }
}
