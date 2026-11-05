//! glTF images as textures: colour images in sRGB, data images (normal,
//! metallic-roughness, occlusion) linear, each uploaded once per use.

use std::collections::HashMap;

use crate::Graphics;
use crate::error::{GfxError, Result};
use crate::texture::{Filter, Texture, TextureOptions, Wrap};

/// Which images and channels a packed texture was made from.
type PackedKey = (Vec<(usize, [bool; 4])>, bool);

pub(super) struct TextureCache<'a> {
    images: &'a [gltf::image::Data],
    made: HashMap<(usize, bool), Texture>,
    packed: HashMap<PackedKey, Texture>,
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
            packed: HashMap::new(),
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
        let opts = options(tex, srgb, false);
        let t = Texture::from_pixels(gfx, img.width, img.height, &px, opts)?;
        self.made.insert((source, srgb), t.clone());
        Ok(t)
    }

    /// One texture holding channels of several glTF textures, for the
    /// extensions that keep two factors in separate images (clearcoat
    /// strength and roughness, sheen colour and roughness, specular colour
    /// and strength, iridescence strength and thickness). Each present
    /// part supplies the channels its mask selects, resampled to the first
    /// part's size; the rest stay white. Alpha is kept as data. `None`
    /// when no part is present.
    pub(super) fn packed(
        &mut self,
        gfx: &Graphics,
        parts: &[(Option<gltf::Texture>, [bool; 4])],
        srgb: bool,
    ) -> Result<Option<Texture>> {
        let parts: Vec<_> = parts
            .iter()
            .filter_map(|(t, mask)| t.as_ref().map(|t| (t, *mask)))
            .collect();
        let Some((first, _)) = parts.first() else {
            return Ok(None);
        };
        let key: Vec<(usize, [bool; 4])> = parts
            .iter()
            .map(|(t, mask)| (t.source().index(), *mask))
            .collect();
        if let Some(t) = self.packed.get(&(key.clone(), srgb)) {
            return Ok(Some(t.clone()));
        }
        let image = |source: usize| {
            let img = self
                .images
                .get(source)
                .ok_or_else(|| GfxError::Gltf(format!("image {source} is missing")))?;
            let px =
                rgba8(img).ok_or_else(|| GfxError::Gltf(format!("image {source} is malformed")))?;
            Ok::<_, GfxError>((img.width as usize, img.height as usize, px))
        };
        let (w, h, _) = image(first.source().index())?;
        let mut out = vec![255u8; w * h * 4];
        for (tex, mask) in &parts {
            let (sw, sh, px) = image(tex.source().index())?;
            for y in 0..h {
                for x in 0..w {
                    let s = ((y * sh / h) * sw + x * sw / w) * 4;
                    let d = (y * w + x) * 4;
                    for c in 0..4 {
                        if mask[c] {
                            out[d + c] = px[s + c];
                        }
                    }
                }
            }
        }
        let opts = options(first, srgb, true);
        let t = Texture::from_pixels(gfx, w as u32, h as u32, &out, opts)?;
        self.packed.insert((key, srgb), t.clone());
        Ok(Some(t))
    }
}

/// Texture options from a glTF texture's sampler.
fn options(tex: &gltf::Texture, srgb: bool, premultiplied: bool) -> TextureOptions {
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
    TextureOptions {
        filter,
        wrap,
        mipmaps: true,
        premultiplied,
        data: !srgb,
    }
}
