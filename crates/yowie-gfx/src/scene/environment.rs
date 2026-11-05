//! Image-based lighting from a panorama: prefiltered specular reflections
//! and diffuse irradiance.

use std::sync::Arc;

use super::frame_data::FrameUniform;
use super::ibl::{EnvMaps, IblBaker};
use super::panorama::Panorama;
use crate::Graphics;
use crate::error::{GfxError, Result};
use crate::texture::next_id;

/// Options for [`Environment::new`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EnvironmentOptions {
    /// The radiance cube's face size in texels, rounded to a power of two;
    /// it is also what the background shows. 256 by default.
    pub size: u32,
    /// Multiplies the environment's light.
    pub intensity: f32,
}

impl Default for EnvironmentOptions {
    fn default() -> Self {
        Self {
            size: 256,
            intensity: 1.0,
        }
    }
}

#[derive(Debug)]
pub(crate) struct EnvInner {
    pub(crate) id: u64,
    pub(crate) maps: EnvMaps,
    pub(crate) intensity: f32,
}

/// An environment that lights a scene and draws behind it, made from an
/// equirectangular panorama. Set it with
/// [`Scene::set_environment`](super::Scene::set_environment). Cloning
/// shares the GPU maps.
#[derive(Clone, Debug)]
pub struct Environment {
    pub(crate) inner: Arc<EnvInner>,
}

impl Environment {
    /// Bakes a panorama into a radiance cube, a prefiltered specular cube
    /// and an irradiance cube. Blocks while the GPU runs the bake.
    pub fn new(gfx: &Graphics, panorama: &Panorama, options: EnvironmentOptions) -> Result<Self> {
        let device = gfx.shared.device();
        let queue = gfx.shared.queue();
        let max = device.limits().max_texture_dimension_2d;
        let (w, h) = (panorama.width(), panorama.height());
        if w > max || h > max {
            return Err(GfxError::InvalidSize {
                width: w,
                height: h,
                max,
            });
        }
        let pano = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("yowie panorama"),
            size: wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba16Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let texels = panorama.to_rgba16f();
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &pano,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            bytemuck::cast_slice(&texels),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(w * 8),
                rows_per_image: Some(h),
            },
            pano.size(),
        );
        let frame = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("yowie bake frame"),
            size: std::mem::size_of::<FrameUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let baker = IblBaker::new(device);
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("yowie environment bake"),
        });
        let view = pano.create_view(&Default::default());
        let maps = baker.bake(
            device,
            queue,
            &mut encoder,
            &frame,
            Some(&view),
            options.size,
        );
        queue.submit([encoder.finish()]);
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|e| GfxError::Panorama(e.to_string()))?;
        Ok(Self {
            inner: Arc::new(EnvInner {
                id: next_id(),
                maps,
                intensity: options.intensity,
            }),
        })
    }

    /// Decodes a panorama (OpenEXR, Radiance HDR, PNG or JPEG) and bakes it;
    /// see [`Panorama::decode`] and [`Environment::new`].
    pub fn from_bytes(gfx: &Graphics, bytes: &[u8], options: EnvironmentOptions) -> Result<Self> {
        Environment::new(gfx, &Panorama::decode(bytes)?, options)
    }

    /// The intensity it was made with.
    pub fn intensity(&self) -> f32 {
        self.inner.intensity
    }
}
