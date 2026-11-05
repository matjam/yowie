//! The images one scene renders into, and the attachment layout every
//! pipeline drawing into them must match.
//!
//! The HDR pass always writes colour (location 0), depth and stencil (in
//! [`SceneLayouts::depth_format`](super::layouts::SceneLayouts)). When the
//! post chain needs them it also writes a surface image (location 1:
//! the octahedral world normal, roughness and the share of the pixel that
//! is ambient light, for ambient occlusion and reflections) and a
//! specular image (location 2: how much of the environment the surface
//! reflects, for reflections). With multisampling the pass draws into
//! multisampled images that resolve into the single-sample ones the post
//! chain reads; depth, which wgpu cannot resolve, is resolved by the
//! chain's first pass, and the stencil is not resolved.
//!
//! Depth images have two views: the depth aspect alone for sampling (the
//! post chain and the static groups' depth pyramid) and both aspects for
//! attaching to passes.
//!
//! A pipeline that draws into the HDR pass takes its colour targets from
//! [`SceneFormat::color_targets`] and its multisample state from
//! [`SceneFormat::multisample`], keyed by the slot's [`SceneFormat`].

use yowie_core::UVec2;

use super::layouts::{HDR_FORMAT, scene_depth_format};

/// Format of the surface image: octahedral normal, roughness, ambient
/// share.
pub(crate) const SURFACE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
/// Format of the specular image: environment reflectance, 1 on surfaces.
pub(crate) const SPECULAR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// Which images the HDR pass writes besides colour and depth.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum Outputs {
    /// Colour alone.
    #[default]
    Color,
    /// Colour and the surface image.
    Surface,
    /// Colour, the surface image and the specular image.
    Specular,
}

/// The attachment layout of a scene's HDR pass.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct SceneFormat {
    /// Samples per pixel: 1, 2, 4 or 8.
    pub(crate) samples: u32,
    pub(crate) outputs: Outputs,
}

impl Default for SceneFormat {
    fn default() -> Self {
        Self {
            samples: 1,
            outputs: Outputs::Color,
        }
    }
}

impl SceneFormat {
    /// The colour targets of a pipeline drawing into the pass: colour with
    /// `blend`, then the surface and specular images, written when
    /// `writes_surface` and otherwise left alone.
    pub(crate) fn color_targets(
        &self,
        blend: Option<wgpu::BlendState>,
        writes_surface: bool,
    ) -> Vec<Option<wgpu::ColorTargetState>> {
        let mask = if writes_surface {
            wgpu::ColorWrites::ALL
        } else {
            wgpu::ColorWrites::empty()
        };
        let extra = |format| {
            Some(wgpu::ColorTargetState {
                format,
                blend: None,
                write_mask: mask,
            })
        };
        let mut targets = vec![Some(wgpu::ColorTargetState {
            format: HDR_FORMAT,
            blend,
            write_mask: wgpu::ColorWrites::ALL,
        })];
        if self.outputs >= Outputs::Surface {
            targets.push(extra(SURFACE_FORMAT));
        }
        if self.outputs >= Outputs::Specular {
            targets.push(extra(SPECULAR_FORMAT));
        }
        targets
    }

    /// The multisample state of a pipeline drawing into the pass.
    pub(crate) fn multisample(&self) -> wgpu::MultisampleState {
        wgpu::MultisampleState {
            count: self.samples,
            ..Default::default()
        }
    }
}

/// The sample counts above 1 that every scene attachment format supports
/// on this device, with resolve for the colour formats.
pub(crate) fn sample_counts(adapter: &wgpu::Adapter, device: &wgpu::Device) -> Vec<u32> {
    let adapter_specific = device
        .features()
        .contains(wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES);
    let features = |f: wgpu::TextureFormat| {
        if adapter_specific {
            adapter.get_texture_format_features(f)
        } else {
            f.guaranteed_format_features(device.features())
        }
    };
    [2, 4, 8]
        .into_iter()
        .filter(|&n| {
            [HDR_FORMAT, SURFACE_FORMAT, SPECULAR_FORMAT]
                .into_iter()
                .all(|f| {
                    let ff = features(f).flags;
                    ff.sample_count_supported(n)
                        && ff.contains(wgpu::TextureFormatFeatureFlags::MULTISAMPLE_RESOLVE)
                })
                && features(scene_depth_format(device))
                    .flags
                    .sample_count_supported(n)
        })
        .collect()
}

/// The largest supported count at or below `requested`, or 1.
pub(crate) fn pick_samples(supported: &[u32], requested: u32) -> u32 {
    supported
        .iter()
        .copied()
        .filter(|&n| n <= requested)
        .max()
        .unwrap_or(1)
}

fn image(
    device: &wgpu::Device,
    label: &str,
    size: UVec2,
    format: wgpu::TextureFormat,
    samples: u32,
    sampled: bool,
) -> wgpu::Texture {
    let mut usage = wgpu::TextureUsages::RENDER_ATTACHMENT;
    if sampled {
        usage |= wgpu::TextureUsages::TEXTURE_BINDING;
    }
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: size.x,
            height: size.y,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: samples,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage,
        view_formats: &[],
    })
}

fn texture(
    device: &wgpu::Device,
    label: &str,
    size: UVec2,
    format: wgpu::TextureFormat,
    samples: u32,
    sampled: bool,
) -> wgpu::TextureView {
    image(device, label, size, format, samples, sampled).create_view(&Default::default())
}

/// A depth and stencil image of `format` with `samples` per pixel: its
/// depth-only view for sampling and its view of both aspects for
/// attaching.
pub(crate) fn depth_views(
    device: &wgpu::Device,
    label: &str,
    size: UVec2,
    format: wgpu::TextureFormat,
    samples: u32,
) -> (wgpu::TextureView, wgpu::TextureView) {
    let t = image(device, label, size, format, samples, true);
    let depth = t.create_view(&wgpu::TextureViewDescriptor {
        label: Some(label),
        aspect: wgpu::TextureAspect::DepthOnly,
        ..Default::default()
    });
    (depth, t.create_view(&Default::default()))
}

/// The multisampled images the pass draws into before resolving.
#[derive(Debug)]
pub(crate) struct Multisampled {
    pub(crate) color: wgpu::TextureView,
    /// The depth aspect, which the post chain resolves and the static
    /// groups' depth pyramid reads.
    pub(crate) depth: wgpu::TextureView,
    /// Depth and stencil, attached to the pass.
    pub(crate) depth_stencil: wgpu::TextureView,
    pub(crate) surface: Option<wgpu::TextureView>,
    pub(crate) specular: Option<wgpu::TextureView>,
}

/// The images of one scene. The single-sample ones are what the post
/// chain reads.
#[derive(Debug)]
pub(crate) struct Targets {
    /// Tells target sets apart for the post chain's bind group cache.
    pub(crate) id: u64,
    pub(crate) size: UVec2,
    pub(crate) format: SceneFormat,
    pub(crate) color: wgpu::TextureView,
    /// The depth aspect of the single-sample depth image.
    pub(crate) depth: wgpu::TextureView,
    /// Depth and stencil of the single-sample depth image, attached when
    /// the pass is not multisampled.
    pub(crate) depth_stencil: wgpu::TextureView,
    pub(crate) surface: Option<wgpu::TextureView>,
    pub(crate) specular: Option<wgpu::TextureView>,
    pub(crate) msaa: Option<Multisampled>,
}

impl Targets {
    /// The images of a scene `size` pixels large in `format`, with depth
    /// and stencil in `depth_format`.
    pub(crate) fn new(
        device: &wgpu::Device,
        size: UVec2,
        format: SceneFormat,
        depth_format: wgpu::TextureFormat,
    ) -> Self {
        let n = format.samples;
        let surface = format.outputs >= Outputs::Surface;
        let specular = format.outputs >= Outputs::Specular;
        let msaa = (n > 1).then(|| {
            let (depth, depth_stencil) =
                depth_views(device, "yowie scene depth msaa", size, depth_format, n);
            Multisampled {
                color: texture(
                    device,
                    "yowie scene colour msaa",
                    size,
                    HDR_FORMAT,
                    n,
                    false,
                ),
                depth,
                depth_stencil,
                surface: surface.then(|| {
                    texture(
                        device,
                        "yowie scene surface msaa",
                        size,
                        SURFACE_FORMAT,
                        n,
                        false,
                    )
                }),
                specular: specular.then(|| {
                    texture(
                        device,
                        "yowie scene specular msaa",
                        size,
                        SPECULAR_FORMAT,
                        n,
                        false,
                    )
                }),
            }
        });
        let (depth, depth_stencil) =
            depth_views(device, "yowie scene depth", size, depth_format, 1);
        Self {
            id: crate::texture::next_id(),
            size,
            format,
            color: texture(device, "yowie scene colour", size, HDR_FORMAT, 1, true),
            depth,
            depth_stencil,
            surface: surface
                .then(|| texture(device, "yowie scene surface", size, SURFACE_FORMAT, 1, true)),
            specular: specular.then(|| {
                texture(
                    device,
                    "yowie scene specular",
                    size,
                    SPECULAR_FORMAT,
                    1,
                    true,
                )
            }),
            msaa,
        }
    }

    /// Targets over images made elsewhere (a probe capture's cube face and
    /// its depth image's two views): colour, depth and stencil alone,
    /// single-sample, so the pass draws with the default format and no
    /// post chain reads it.
    pub(crate) fn wrap(
        size: UVec2,
        color: wgpu::TextureView,
        depth: wgpu::TextureView,
        depth_stencil: wgpu::TextureView,
    ) -> Self {
        Self {
            id: crate::texture::next_id(),
            size,
            format: SceneFormat::default(),
            color,
            depth,
            depth_stencil,
            surface: None,
            specular: None,
            msaa: None,
        }
    }

    /// The pass's colour attachments: cleared when `first`, otherwise
    /// loaded (a pass split in two); multisampled images resolve into the
    /// single-sample ones, and are kept only when `keep` (another pass
    /// follows).
    pub(crate) fn color_attachments(
        &self,
        first: bool,
        keep: bool,
    ) -> Vec<Option<wgpu::RenderPassColorAttachment<'_>>> {
        let ops = wgpu::Operations {
            load: if first {
                wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT)
            } else {
                wgpu::LoadOp::Load
            },
            store: wgpu::StoreOp::Store,
        };
        let m = self.msaa.as_ref();
        let pairs = [
            (Some(&self.color), m.map(|m| &m.color)),
            (self.surface.as_ref(), m.and_then(|m| m.surface.as_ref())),
            (self.specular.as_ref(), m.and_then(|m| m.specular.as_ref())),
        ];
        let mut out = Vec::with_capacity(3);
        for (single, multi) in pairs {
            let Some(single) = single else {
                continue;
            };
            out.push(Some(match multi {
                Some(multi) => wgpu::RenderPassColorAttachment {
                    view: multi,
                    depth_slice: None,
                    resolve_target: Some(single),
                    ops: wgpu::Operations {
                        store: if keep {
                            wgpu::StoreOp::Store
                        } else {
                            wgpu::StoreOp::Discard
                        },
                        ..ops
                    },
                },
                None => wgpu::RenderPassColorAttachment {
                    view: single,
                    depth_slice: None,
                    resolve_target: None,
                    ops,
                },
            }));
        }
        out
    }

    /// The depth and stencil view the pass attaches: the multisampled one
    /// when there is one (the chain resolves its depth).
    pub(crate) fn attached_depth(&self) -> &wgpu::TextureView {
        self.msaa
            .as_ref()
            .map_or(&self.depth_stencil, |m| &m.depth_stencil)
    }

    /// The pass's depth and stencil attachment, cleared to the far plane
    /// (0, reversed) and stencil 0 when `first`, loaded otherwise.
    pub(crate) fn depth_attachment(
        &self,
        first: bool,
    ) -> wgpu::RenderPassDepthStencilAttachment<'_> {
        let load = |clear| {
            if first {
                wgpu::LoadOp::Clear(clear)
            } else {
                wgpu::LoadOp::Load
            }
        };
        wgpu::RenderPassDepthStencilAttachment {
            view: self.attached_depth(),
            depth_ops: Some(wgpu::Operations {
                load: load(0.0),
                store: wgpu::StoreOp::Store,
            }),
            stencil_ops: Some(wgpu::Operations {
                load: if first {
                    wgpu::LoadOp::Clear(0)
                } else {
                    wgpu::LoadOp::Load
                },
                store: wgpu::StoreOp::Store,
            }),
        }
    }
}
