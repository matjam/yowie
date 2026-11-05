//! Creating textures from decoded images and replacing their contents in
//! place.

use yowie_core::uvec2;
use yowie_ktx2::{BlockFormat, Ktx2File};

use crate::Graphics;
use crate::error::{GfxError, Result};
use crate::half::f32_to_f16;
use crate::image_data::ImageData;
use crate::pixels;
use crate::shared::Shared;
use crate::texture::{Purpose, Texture, TextureGpu, TextureOptions};

/// The wgpu format for a block format.
fn block_format(block: BlockFormat, srgb: bool) -> wgpu::TextureFormat {
    use wgpu::TextureFormat as F;
    match (block, srgb) {
        (BlockFormat::Bc1, true) => F::Bc1RgbaUnormSrgb,
        (BlockFormat::Bc1, false) => F::Bc1RgbaUnorm,
        (BlockFormat::Bc3, true) => F::Bc3RgbaUnormSrgb,
        (BlockFormat::Bc3, false) => F::Bc3RgbaUnorm,
        (BlockFormat::Bc4, _) => F::Bc4RUnorm,
        (BlockFormat::Bc5, _) => F::Bc5RgUnorm,
        (BlockFormat::Bc7, true) => F::Bc7RgbaUnormSrgb,
        (BlockFormat::Bc7, false) => F::Bc7RgbaUnorm,
    }
}

fn mip_count(width: u32, height: u32) -> u32 {
    32 - width.max(height).max(1).leading_zeros()
}

fn build_rgba8(
    shared: &Shared,
    w: u32,
    h: u32,
    pixels_in: &[u8],
    options: TextureOptions,
) -> Result<TextureGpu> {
    let format = if options.data {
        wgpu::TextureFormat::Rgba8Unorm
    } else {
        wgpu::TextureFormat::Rgba8UnormSrgb
    };
    let mips = if options.mipmaps { mip_count(w, h) } else { 1 };
    let gpu = TextureGpu::new(shared, uvec2(w, h), format, options, Purpose::Sampled, mips)?;
    let mut data = pixels_in.to_vec();
    if !options.premultiplied {
        if options.data {
            pixels::premultiply_linear(&mut data);
        } else {
            pixels::premultiply_srgb(&mut data);
        }
    }
    gpu.write_level(shared, 0, [0, 0], [w, h], &data, 4);
    shared
        .mipmaps
        .generate(shared.device(), shared.queue(), &gpu.texture);
    Ok(gpu)
}

fn build_float(
    shared: &Shared,
    w: u32,
    h: u32,
    pixels_in: &[f32],
    options: TextureOptions,
) -> Result<TextureGpu> {
    let options = TextureOptions {
        data: true,
        premultiplied: true,
        ..options
    };
    let mips = if options.mipmaps { mip_count(w, h) } else { 1 };
    let gpu = TextureGpu::new(
        shared,
        uvec2(w, h),
        wgpu::TextureFormat::Rgba16Float,
        options,
        Purpose::Sampled,
        mips,
    )?;
    let halves: Vec<u8> = pixels_in
        .iter()
        .flat_map(|&v| f32_to_f16(v).to_le_bytes())
        .collect();
    gpu.write_level(shared, 0, [0, 0], [w, h], &halves, 8);
    shared
        .mipmaps
        .generate(shared.device(), shared.queue(), &gpu.texture);
    Ok(gpu)
}

fn build_compressed(
    shared: &Shared,
    file: &Ktx2File,
    options: TextureOptions,
) -> Result<TextureGpu> {
    if !shared
        .device()
        .features()
        .contains(wgpu::Features::TEXTURE_COMPRESSION_BC)
    {
        return Err(GfxError::UnsupportedFormat(
            "this GPU cannot sample BC-compressed textures".into(),
        ));
    }
    let Some((block, srgb)) = file.block_format() else {
        return Err(GfxError::UnsupportedFormat(format!(
            "KTX2 format {:?} is not one of BC1, BC3, BC4, BC5 or BC7",
            file.format
        )));
    };
    let options = TextureOptions {
        mipmaps: file.levels.len() > 1,
        premultiplied: file.premultiplied,
        data: !srgb,
        ..options
    };
    let gpu = TextureGpu::new(
        shared,
        uvec2(file.width, file.height),
        block_format(block, srgb),
        options,
        Purpose::Sampled,
        file.levels.len() as u32,
    )?;
    for (i, level) in file.levels.iter().enumerate() {
        let (w, h) = file.level_size(i as u32);
        let (bx, by) = (w.div_ceil(4), h.div_ceil(4));
        gpu.write_rows(
            shared,
            i as u32,
            [0, 0],
            [bx * 4, by * 4],
            level,
            bx * block.block_bytes() as u32,
            by,
        );
    }
    Ok(gpu)
}

/// Builds GPU objects for `data`, sampling as `options` say.
fn build(shared: &Shared, data: &ImageData, options: TextureOptions) -> Result<TextureGpu> {
    data.check()?;
    match data {
        ImageData::Rgba8 {
            width,
            height,
            pixels,
        } => build_rgba8(shared, *width, *height, pixels, options),
        ImageData::Rgba32F {
            width,
            height,
            pixels,
        } => build_float(shared, *width, *height, pixels, options),
        ImageData::Compressed(file) => build_compressed(shared, file, options),
    }
}

impl Graphics {
    /// Whether textures can be uploaded from BC1, BC3, BC4, BC5 and BC7
    /// compressed images. Desktop GPUs support them; many mobile GPUs do
    /// not.
    pub fn supports_bc(&self) -> bool {
        self.shared
            .device()
            .features()
            .contains(wgpu::Features::TEXTURE_COMPRESSION_BC)
    }
}

impl Texture {
    /// Uploads decoded image data. `options` choose sampling; for
    /// [`ImageData::Compressed`] the file decides mipmaps, alpha and colour
    /// space, and for [`ImageData::Rgba32F`] the texture holds linear data.
    pub fn from_data(gfx: &Graphics, data: &ImageData, options: TextureOptions) -> Result<Texture> {
        Ok(Texture::from_gpu(build(&gfx.shared, data, options)?))
    }

    /// Decodes a file (PNG, JPEG, Radiance HDR or KTX2) and uploads it.
    pub fn from_file_bytes(
        gfx: &Graphics,
        bytes: &[u8],
        options: TextureOptions,
    ) -> Result<Texture> {
        Texture::from_data(gfx, &ImageData::decode(bytes)?, options)
    }

    /// Replaces the image in place. The texture keeps its identity, so
    /// every clone, sprite and queued draw that refers to it shows the new
    /// image, whatever its size. Sampling options (filter and wrap) stay;
    /// the new data decides the rest as in [`Texture::from_data`]. On error
    /// the old image stays.
    ///
    /// The texture must not be a render target.
    pub fn replace_data(&self, gfx: &Graphics, data: &ImageData) -> Result<()> {
        let old = self.gpu();
        let options = TextureOptions {
            filter: old.options.filter,
            wrap: old.options.wrap,
            mipmaps: old.options.mipmaps,
            premultiplied: old.is_rgba8() && old.options.premultiplied,
            data: old.is_rgba8() && old.options.data,
        };
        let new = build(&gfx.shared, data, options)?;
        *self.inner.gpu.write() = std::sync::Arc::new(new);
        Ok(())
    }
}
