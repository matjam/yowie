//! Post-processing: Pass 2 of the frame, from each scene's HDR image (or
//! a 2D frame) to the output.
//!
//! A scene's chain runs, in this order and only the steps its
//! [`PostSettings`] turn on (`steps.rs`): the depth copy (resolving
//! multisampled depth) and its nearest-depth pyramid, ambient occlusion,
//! reflections, both applied to the picture, light shafts, the motion of
//! moved meshes, temporal anti-aliasing, motion blur, depth of field and
//! automatic exposure; then bloom and the composite (exposure, tone
//! curve, grade and lens effects) into the scene's viewport, and FXAA
//! after it (`chain.rs`). Each full-size step reads the image holding the
//! picture and writes a spare one, so nothing is copied. With the default
//! settings the chain is the composite alone, which is the resolve the
//! renderer had before post-processing: exposure, the tone curve and
//! composition over the background.
//!
//! Images are made the first time a setting asks for them and kept at the
//! scene's size; bind groups and pipelines are cached, so a frame records
//! passes and nothing else.
//!
//! A 2D frame ([`Graphics::set_frame_post`](crate::Graphics::set_frame_post))
//! draws its scenes and the game's 2D layers below
//! [`layers::UI`](crate::layers::UI) into an HDR image, runs bloom, the
//! grade, the lens effects and FXAA over it into the output, and draws the
//! interface layers on top.

mod chain;
mod exposure;
mod frame2d;
#[cfg(test)]
mod gpu_tests;
mod lut;
mod pipelines;
mod scene_chain;
mod settings;
mod steps;
mod targets;
mod tonemap;
mod uniform;
mod velocity;

pub use lut::Lut;
pub use settings::PostSettings;

pub(crate) use frame2d::FramePost;
pub(crate) use targets::{HDR, PostTargets};
pub(crate) use uniform::PostCamera;

use pipelines::PostPipelines;

/// Stand-ins bound where a pass's optional input is off.
#[derive(Debug)]
struct Dummies {
    id: u64,
    black: wgpu::TextureView,
    white: wgpu::TextureView,
    lut: wgpu::TextureView,
    exposure: wgpu::Buffer,
}

impl Dummies {
    fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Dummies {
        let tex = |label, format, dimension, depth, bytes: &[u8]| {
            let size = wgpu::Extent3d {
                width: depth,
                height: depth,
                depth_or_array_layers: if dimension == wgpu::TextureDimension::D3 {
                    depth
                } else {
                    1
                },
            };
            let t = device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size,
                mip_level_count: 1,
                sample_count: 1,
                dimension,
                format,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            let bpp = bytes.len() as u32 / (size.width * size.height * size.depth_or_array_layers);
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &t,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                bytes,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(size.width * bpp),
                    rows_per_image: Some(size.height),
                },
                size,
            );
            t.create_view(&Default::default())
        };
        let d2 = wgpu::TextureDimension::D2;
        let black = tex("yowie post black", HDR, d2, 1, &[0; 8]);
        let white = tex(
            "yowie post white",
            wgpu::TextureFormat::R8Unorm,
            d2,
            1,
            &[255],
        );
        let mut lut = Vec::new();
        for b in [0u8, 255] {
            for g in [0u8, 255] {
                for r in [0u8, 255] {
                    lut.extend_from_slice(&[r, g, b, 255]);
                }
            }
        }
        let lut = tex(
            "yowie post identity lut",
            wgpu::TextureFormat::Rgba8Unorm,
            wgpu::TextureDimension::D3,
            2,
            &lut,
        );
        let exposure = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("yowie post exposure"),
            size: 16,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&exposure, 0, bytemuck::cast_slice(&[1.0f32, 1.0, 0.0, 0.0]));
        Dummies {
            id: crate::texture::next_id(),
            black,
            white,
            lut,
            exposure,
        }
    }
}

/// The post passes' pipelines and the state they share, kept by
/// [`Graphics`](crate::Graphics) once a scene or a 2D frame first needs
/// them.
#[derive(Debug)]
pub(crate) struct PostRenderer {
    pipes: PostPipelines,
    linear: wgpu::Sampler,
    dummies: Dummies,
    /// The 2D frame post state of each output, by output key.
    pub(crate) frames: FramePost,
    velocity: Option<velocity::VelocityPipeline>,
    exposure: Option<exposure::ExposurePipelines>,
    /// The device runs the auto exposure compute passes.
    compute: bool,
}

impl PostRenderer {
    pub(crate) fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        Self {
            velocity: None,
            exposure: None,
            compute: exposure::supported(device),
            pipes: PostPipelines::new(device),
            linear: device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("yowie post linear"),
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                ..Default::default()
            }),
            dummies: Dummies::new(device, queue),
            frames: FramePost::default(),
        }
    }

    /// New post targets for a picture `size` pixels large.
    pub(crate) fn targets(&self, device: &wgpu::Device, size: yowie_core::UVec2) -> PostTargets {
        PostTargets::new(device, &self.pipes.common, &self.linear, size)
    }
}

/// Records one full-screen triangle into `target` with `pipeline` and the
/// two bind groups, in `viewport` (output pixels) or the whole target.
#[allow(clippy::too_many_arguments)]
pub(crate) fn fullscreen(
    encoder: &mut wgpu::CommandEncoder,
    label: &str,
    target: &wgpu::TextureView,
    load: wgpu::LoadOp<wgpu::Color>,
    pipeline: &wgpu::RenderPipeline,
    groups: [&wgpu::BindGroup; 2],
    viewport: Option<yowie_core::Rect>,
    timestamps: Option<wgpu::RenderPassTimestampWrites<'_>>,
) {
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some(label),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: target,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load,
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: timestamps,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    if let Some(v) = viewport {
        pass.set_viewport(v.x, v.y, v.w, v.h, 0.0, 1.0);
    }
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, groups[0], &[]);
    pass.set_bind_group(1, groups[1], &[]);
    pass.draw(0..3, 0..1);
}

/// The load operation that clears to `color` (premultiplied linear from
/// sRGB), or keeps what the target holds.
pub(crate) fn load_op(color: Option<yowie_core::Color>) -> wgpu::LoadOp<wgpu::Color> {
    match color {
        Some(c) => {
            let [r, g, b, a] = c.to_linear_premultiplied();
            wgpu::LoadOp::Clear(wgpu::Color {
                r: f64::from(r),
                g: f64::from(g),
                b: f64::from(b),
                a: f64::from(a),
            })
        }
        None => wgpu::LoadOp::Load,
    }
}
