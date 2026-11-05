//! Copying texture pixels back to the processor.

use image::RgbaImage;

use crate::error::{GfxError, Result};
use crate::pixels;

/// A texture copy recorded into an encoder and waiting to be mapped.
#[derive(Debug)]
pub(crate) struct PendingReadback {
    buffer: wgpu::Buffer,
    width: u32,
    height: u32,
    padded_row: u32,
    bgra: bool,
}

/// Records a copy of level 0 of `texture` into a mappable buffer.
pub(crate) fn record(
    device: &wgpu::Device,
    encoder: &mut wgpu::CommandEncoder,
    texture: &wgpu::Texture,
) -> Result<PendingReadback> {
    let format = texture.format();
    let bgra = match format.remove_srgb_suffix() {
        wgpu::TextureFormat::Rgba8Unorm => false,
        wgpu::TextureFormat::Bgra8Unorm => true,
        other => {
            return Err(GfxError::Readback(format!(
                "texture format {other:?} cannot be read back as RGBA8"
            )));
        }
    };
    let (width, height) = (texture.width(), texture.height());
    let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let padded_row = (width * 4).div_ceil(align) * align;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("yowie readback"),
        size: u64::from(padded_row) * u64::from(height),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded_row),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    Ok(PendingReadback {
        buffer,
        width,
        height,
        padded_row,
        bgra,
    })
}

impl PendingReadback {
    /// Waits for the submitted copy and returns the pixels as stored:
    /// sRGB-encoded and premultiplied. With `straight`, colour is divided
    /// by alpha first.
    pub(crate) fn finish(self, device: &wgpu::Device, straight: bool) -> Result<RgbaImage> {
        let (tx, rx) = std::sync::mpsc::channel();
        self.buffer.map_async(wgpu::MapMode::Read, .., move |r| {
            // The receiver outlives the wait below, so a failed send means
            // nobody is listening any more and the result is not needed.
            let _ = tx.send(r);
        });
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|e| GfxError::Readback(e.to_string()))?;
        rx.recv()
            .map_err(|e| GfxError::Readback(e.to_string()))?
            .map_err(|e| GfxError::Readback(e.to_string()))?;
        let mut pixels = Vec::with_capacity(self.width as usize * self.height as usize * 4);
        {
            let mapped = self
                .buffer
                .get_mapped_range(..)
                .map_err(|e| GfxError::Readback(e.to_string()))?;
            for row in mapped.chunks(self.padded_row as usize) {
                pixels.extend_from_slice(&row[..self.width as usize * 4]);
            }
        }
        self.buffer.unmap();
        if self.bgra {
            for px in pixels.as_chunks_mut::<4>().0 {
                px.swap(0, 2);
            }
        }
        if straight {
            pixels::unpremultiply_srgb(&mut pixels);
        }
        RgbaImage::from_raw(self.width, self.height, pixels)
            .ok_or_else(|| GfxError::Readback("pixel buffer has the wrong length".into()))
    }
}
