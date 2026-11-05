//! The frame's output: a window surface or a headless image.

use yowie_core::{UVec2, uvec2};

use crate::device::Gpu;
use crate::error::{GfxError, Result};

/// The format headless frames and render textures use.
pub(crate) const OFFSCREEN_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

#[derive(Debug)]
pub(crate) enum Output {
    Surface {
        surface: wgpu::Surface<'static>,
        config: wgpu::SurfaceConfiguration,
        /// The sRGB format the frame is rendered through.
        view_format: wgpu::TextureFormat,
        can_copy: bool,
    },
    Headless {
        texture: wgpu::Texture,
    },
}

/// An acquired output image for one frame.
#[derive(Debug)]
pub(crate) struct OutputFrame {
    pub(crate) surface_texture: Option<wgpu::SurfaceTexture>,
    pub(crate) texture: wgpu::Texture,
    pub(crate) view: wgpu::TextureView,
}

fn headless_texture(device: &wgpu::Device, size: UVec2) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("yowie headless frame"),
        size: wgpu::Extent3d {
            width: size.x.max(1),
            height: size.y.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: OFFSCREEN_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    })
}

impl Output {
    pub(crate) fn headless(gpu: &Gpu, size: UVec2) -> Output {
        Output::Headless {
            texture: headless_texture(&gpu.device, size),
        }
    }

    pub(crate) fn surface(
        gpu: &Gpu,
        surface: wgpu::Surface<'static>,
        size: UVec2,
        vsync: bool,
    ) -> Result<Output> {
        let caps = surface.get_capabilities(&gpu.adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .or_else(|| caps.formats.first().copied())
            .ok_or(GfxError::SurfaceUnsupported)?;
        let view_format = format.add_srgb_suffix();
        let can_copy = caps.usages.contains(wgpu::TextureUsages::COPY_SRC);
        let mut usage = wgpu::TextureUsages::RENDER_ATTACHMENT;
        if can_copy {
            usage |= wgpu::TextureUsages::COPY_SRC;
        }
        let config = wgpu::SurfaceConfiguration {
            usage,
            format,
            color_space: wgpu::SurfaceColorSpace::Auto,
            width: size.x.max(1),
            height: size.y.max(1),
            present_mode: if vsync {
                wgpu::PresentMode::AutoVsync
            } else {
                wgpu::PresentMode::AutoNoVsync
            },
            desired_maximum_frame_latency: 2,
            alpha_mode: caps
                .alpha_modes
                .first()
                .copied()
                .unwrap_or(wgpu::CompositeAlphaMode::Auto),
            view_formats: if view_format == format {
                vec![]
            } else {
                vec![view_format]
            },
        };
        surface.configure(&gpu.device, &config);
        log::info!("yowie-gfx: surface {format:?} viewed as {view_format:?}");
        Ok(Output::Surface {
            surface,
            config,
            view_format,
            can_copy,
        })
    }

    /// The format the frame is rendered in.
    pub(crate) fn format(&self) -> wgpu::TextureFormat {
        match self {
            Output::Surface { view_format, .. } => *view_format,
            Output::Headless { .. } => OFFSCREEN_FORMAT,
        }
    }

    /// The output size in pixels.
    pub(crate) fn size(&self) -> UVec2 {
        match self {
            Output::Surface { config, .. } => uvec2(config.width, config.height),
            Output::Headless { texture } => uvec2(texture.width(), texture.height()),
        }
    }

    pub(crate) fn resize(&mut self, gpu: &Gpu, size: UVec2) {
        let size = size.max(UVec2::ONE);
        match self {
            Output::Surface {
                surface, config, ..
            } => {
                config.width = size.x;
                config.height = size.y;
                surface.configure(&gpu.device, config);
            }
            Output::Headless { texture } => *texture = headless_texture(&gpu.device, size),
        }
    }

    pub(crate) fn set_vsync(&mut self, gpu: &Gpu, vsync: bool) {
        if let Output::Surface {
            surface, config, ..
        } = self
        {
            config.present_mode = if vsync {
                wgpu::PresentMode::AutoVsync
            } else {
                wgpu::PresentMode::AutoNoVsync
            };
            surface.configure(&gpu.device, config);
        }
    }

    /// Whether frames can be copied back.
    pub(crate) fn can_capture(&self) -> bool {
        match self {
            Output::Surface { can_copy, .. } => *can_copy,
            Output::Headless { .. } => true,
        }
    }

    /// Acquires the image to render into, or `None` when the window cannot
    /// take a frame now (minimised, or the surface was just reconfigured).
    pub(crate) fn acquire(&mut self, gpu: &Gpu) -> Option<OutputFrame> {
        match self {
            Output::Headless { texture } => Some(OutputFrame {
                surface_texture: None,
                view: texture.create_view(&wgpu::TextureViewDescriptor::default()),
                texture: texture.clone(),
            }),
            Output::Surface {
                surface,
                config,
                view_format,
                ..
            } => {
                let st = match surface.get_current_texture() {
                    wgpu::CurrentSurfaceTexture::Success(t) => t,
                    wgpu::CurrentSurfaceTexture::Suboptimal(t) => {
                        // Present this one; the next resize event reconfigures.
                        t
                    }
                    wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                        surface.configure(&gpu.device, config);
                        return None;
                    }
                    other => {
                        log::debug!("yowie-gfx: skipping frame: {other:?}");
                        return None;
                    }
                };
                let view = st.texture.create_view(&wgpu::TextureViewDescriptor {
                    format: Some(*view_format),
                    ..Default::default()
                });
                Some(OutputFrame {
                    texture: st.texture.clone(),
                    surface_texture: Some(st),
                    view,
                })
            }
        }
    }
}
