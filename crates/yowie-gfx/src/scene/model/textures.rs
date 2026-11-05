//! glTF images as textures: colour images in sRGB, data images (normal,
//! metallic-roughness, occlusion) linear, each uploaded once per use.

use std::collections::HashMap;

use crate::Graphics;
use crate::error::{GfxError, Result};
use crate::texture::{Filter, Texture, TextureOptions, Wrap};

pub(super) struct TextureCache<'a> {
    images: &'a [gltf::image::Data],
    made: HashMap<(usize, bool), Texture>,
}

/// Expands a glTF image to RGBA8.
fn rgba8(img: &gltf::image::Data) -> Option<Vec<u8>> {
    use gltf::image::Format as F;
    let px = &img.pixels;
    let n = img.width as usize * img.height as usize;
    let out: Vec<u8> = match img.format {
        F::R8G8B8A8 => px.clone(),
        F::R8G8B8 => px
            .as_chunks::<3>()
            .0
            .iter()
            .flat_map(|c| [c[0], c[1], c[2], 255])
            .collect(),
        F::R8G8 => px
            .as_chunks::<2>()
            .0
            .iter()
            .flat_map(|c| [c[0], c[1], 0, 255])
            .collect(),
        F::R8 => px.iter().flat_map(|c| [*c, *c, *c, 255]).collect(),
        F::R16G16B16A16 => px.as_chunks::<2>().0.iter().map(|c| c[1]).collect(),
        F::R16G16B16 => px
            .as_chunks::<6>()
            .0
            .iter()
            .flat_map(|c| [c[1], c[3], c[5], 255])
            .collect(),
        F::R16G16 => px
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|c| [c[1], c[3], 0, 255])
            .collect(),
        F::R16 => px
            .as_chunks::<2>()
            .0
            .iter()
            .flat_map(|c| [c[1], c[1], c[1], 255])
            .collect(),
        F::R32G32B32FLOAT | F::R32G32B32A32FLOAT => {
            let k = if img.format == F::R32G32B32FLOAT {
                3
            } else {
                4
            };
            px.chunks_exact(4 * k)
                .flat_map(|c| {
                    let f = |i: usize| {
                        let v = f32::from_le_bytes([
                            c[i * 4],
                            c[i * 4 + 1],
                            c[i * 4 + 2],
                            c[i * 4 + 3],
                        ]);
                        (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
                    };
                    [f(0), f(1), f(2), if k == 4 { f(3) } else { 255 }]
                })
                .collect()
        }
    };
    (out.len() == n * 4).then_some(out)
}

impl<'a> TextureCache<'a> {
    pub(super) fn new(images: &'a [gltf::image::Data]) -> Self {
        Self {
            images,
            made: HashMap::new(),
        }
    }

    /// The texture for a glTF texture, as colour (`srgb`) or data.
    pub(super) fn get(
        &mut self,
        gfx: &Graphics,
        tex: &gltf::Texture,
        srgb: bool,
    ) -> Result<Texture> {
        let source = tex.source().index();
        if let Some(t) = self.made.get(&(source, srgb)) {
            return Ok(t.clone());
        }
        let img = self
            .images
            .get(source)
            .ok_or_else(|| GfxError::Gltf(format!("image {source} is missing")))?;
        let mut px =
            rgba8(img).ok_or_else(|| GfxError::Gltf(format!("image {source} is malformed")))?;
        if !srgb {
            // Data maps leave alpha unused; keeping it opaque stops the
            // upload's premultiplication from changing the data.
            for p in px.as_chunks_mut::<4>().0 {
                p[3] = 255;
            }
        }
        let sampler = tex.sampler();
        let wrap = match sampler.wrap_s() {
            gltf::texture::WrappingMode::ClampToEdge => Wrap::Clamp,
            gltf::texture::WrappingMode::MirroredRepeat => Wrap::Mirror,
            gltf::texture::WrappingMode::Repeat => Wrap::Repeat,
        };
        let filter = match sampler.mag_filter() {
            Some(gltf::texture::MagFilter::Nearest) => Filter::Nearest,
            _ => Filter::Linear,
        };
        let opts = TextureOptions {
            filter,
            wrap,
            mipmaps: true,
            premultiplied: false,
            data: !srgb,
        };
        let t = Texture::from_pixels(gfx, img.width, img.height, &px, opts)?;
        self.made.insert((source, srgb), t.clone());
        Ok(t)
    }
}
