//! The finishing steps every chain ends with, for scenes and 2D frames
//! alike: bloom, the composite and FXAA, and the helpers every step
//! uses (inputs, bind group caching).

use yowie_core::Rect;

use super::pipelines::{Blend, PipeKey, Program};
use super::settings::PostSettings;
use super::targets::{HDR, LDR, PostTargets};
use super::{PostRenderer, fullscreen};

/// A texture view with the id its bind groups are cached by.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Input<'a> {
    pub(crate) view: &'a wgpu::TextureView,
    pub(crate) id: u64,
}

/// What the composite reads besides the picture; `None` binds a
/// stand-in that changes nothing.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Extras<'a> {
    /// The occlusion image, shown in place of the picture when asked.
    pub(crate) ao: Option<Input<'a>>,
    pub(crate) rays: Option<Input<'a>>,
    pub(crate) exposure: Option<(&'a wgpu::Buffer, u64)>,
}

/// Where the finished picture goes.
#[derive(Debug)]
pub(crate) struct Output<'a> {
    pub(crate) view: &'a wgpu::TextureView,
    pub(crate) format: wgpu::TextureFormat,
    /// The rectangle of the output in pixels; `None` for all of it.
    pub(crate) viewport: Option<Rect>,
    pub(crate) load: wgpu::LoadOp<wgpu::Color>,
    /// How the picture combines with the output: premultiplied over it
    /// for a scene, replacing it for a 2D frame.
    pub(crate) blend: Blend,
}

/// Bind group cache tags, the first word of a key.
pub(crate) mod tag {
    pub(crate) const COMPOSITE: u64 = 1;
    pub(crate) const PREFILTER: u64 = 2;
    pub(crate) const DOWN: u64 = 3;
    pub(crate) const UP: u64 = 4;
    pub(crate) const FXAA: u64 = 5;
}

/// Folds ids into one cache word.
pub(crate) fn mix(ids: &[u64]) -> u64 {
    ids.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, v| {
        (h ^ v).wrapping_mul(0x0000_0100_0000_01b3).rotate_left(17)
    })
}

/// The bind group under `key`, made by `make` the first time.
pub(crate) fn cached(
    t: &mut PostTargets,
    key: [u64; 4],
    make: impl FnOnce(&PostTargets) -> wgpu::BindGroup,
) -> wgpu::BindGroup {
    if let Some(g) = t.groups.get(&key) {
        return g.clone();
    }
    let g = make(t);
    t.groups.insert(key, g.clone());
    g
}

pub(crate) fn tex(binding: u32, view: &wgpu::TextureView) -> wgpu::BindGroupEntry<'_> {
    wgpu::BindGroupEntry {
        binding,
        resource: wgpu::BindingResource::TextureView(view),
    }
}

pub(crate) fn buf(binding: u32, buffer: &wgpu::Buffer) -> wgpu::BindGroupEntry<'_> {
    wgpu::BindGroupEntry {
        binding,
        resource: buffer.as_entire_binding(),
    }
}

/// The number of bloom mips for a picture of `size`, as `BloomChain`
/// makes them.
pub(crate) fn bloom_levels(size: yowie_core::UVec2) -> u32 {
    let half = (size / 2).max(yowie_core::UVec2::ONE);
    let short = half.x.min(half.y).max(1);
    (32 - short.leading_zeros()).saturating_sub(3).clamp(1, 6)
}

impl PostRenderer {
    /// Records bloom (when on), the composite and FXAA (when on), from the
    /// picture `src` into `out`. The post block must already hold this
    /// frame's values. Returns the number of draws.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn finish(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        t: &mut PostTargets,
        settings: &PostSettings,
        src: Input<'_>,
        extras: &Extras<'_>,
        out: Output<'_>,
    ) -> u32 {
        let mut draws = 0;
        let exposure_buffer = extras
            .exposure
            .map_or(self.dummies.exposure.clone(), |e| e.0.clone());
        let exposure = (
            &exposure_buffer,
            extras.exposure.map_or(self.dummies.id, |e| e.1),
        );
        let bloom = if settings.wants_bloom() {
            draws += self.bloom(device, encoder, t, src, exposure);
            t.bloom.as_ref().map(|b| (b.mips[0].clone(), b.image.id))
        } else {
            None
        };
        let lut = settings
            .lut
            .as_ref()
            .filter(|_| settings.grades() && settings.lut_strength > 0.0);
        let d = &self.dummies;
        let pick = |i: Option<Input<'_>>, dummy: u64| i.map_or(dummy, |i| i.id);
        let key = [
            tag::COMPOSITE,
            src.id,
            mix(&[
                bloom.as_ref().map_or(0, |b| b.1),
                pick(extras.ao, 1),
                pick(extras.rays, 2),
            ]),
            mix(&[lut.map_or(3, |l| l.id()), exposure.1]),
        ];
        let layout = self.pipes.group(Program::Composite).clone();
        let group = cached(t, key, |_| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("yowie composite"),
                layout: &layout,
                entries: &[
                    tex(0, src.view),
                    tex(1, bloom.as_ref().map_or(&d.black, |b| &b.0)),
                    tex(2, extras.ao.map_or(&d.white, |i| i.view)),
                    tex(3, extras.rays.map_or(&d.black, |i| i.view)),
                    tex(4, lut.map_or(&d.lut, |l| l.view())),
                    buf(5, exposure.0),
                ],
            })
        });
        if settings.fxaa {
            let ldr_id = t.ldr(device).id;
            let pipe = self.pipes.get(
                device,
                PipeKey {
                    program: Program::Composite,
                    entry: "fs_composite",
                    format: LDR,
                    blend: Blend::Replace,
                },
            );
            let ldr = t.ldr.as_ref().map(|i| i.view.clone());
            let Some(ldr) = ldr else {
                return draws;
            };
            fullscreen(
                encoder,
                "yowie composite",
                &ldr,
                wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                &pipe,
                [&t.common, &group],
                None,
                None,
            );
            let layout = self.pipes.group(Program::Fxaa).clone();
            let fxaa_group = cached(t, [tag::FXAA, ldr_id, 0, 0], |_| {
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("yowie fxaa"),
                    layout: &layout,
                    entries: &[tex(0, &ldr)],
                })
            });
            let pipe = self.pipes.get(
                device,
                PipeKey {
                    program: Program::Fxaa,
                    entry: "fs_fxaa",
                    format: out.format,
                    blend: out.blend,
                },
            );
            fullscreen(
                encoder,
                "yowie fxaa",
                out.view,
                out.load,
                &pipe,
                [&t.common, &fxaa_group],
                out.viewport,
                None,
            );
            draws += 2;
        } else {
            let pipe = self.pipes.get(
                device,
                PipeKey {
                    program: Program::Composite,
                    entry: "fs_composite",
                    format: out.format,
                    blend: out.blend,
                },
            );
            fullscreen(
                encoder,
                "yowie composite",
                out.view,
                out.load,
                &pipe,
                [&t.common, &group],
                out.viewport,
                None,
            );
            draws += 1;
        }
        draws
    }

    /// The bloom chain from `src` into the bloom mips; returns the draws.
    fn bloom(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        t: &mut PostTargets,
        src: Input<'_>,
        exposure: (&wgpu::Buffer, u64),
    ) -> u32 {
        let chain_id = t.bloom(device).image.id;
        let Some(mips) = t.bloom.as_ref().map(|b| b.mips.clone()) else {
            return 0;
        };
        let layout = self.pipes.group(Program::Bloom).clone();
        let make = |view: &wgpu::TextureView| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("yowie bloom"),
                layout: &layout,
                entries: &[tex(0, view), buf(1, exposure.0)],
            })
        };
        let key = |pass: &'static str, format| PipeKey {
            program: Program::Bloom,
            entry: pass,
            format,
            blend: Blend::Replace,
        };
        let prefilter = self.pipes.get(device, key("fs_prefilter", HDR));
        let down = self.pipes.get(device, key("fs_down", HDR));
        let up = self.pipes.get(
            device,
            PipeKey {
                blend: Blend::Additive,
                ..key("fs_up", HDR)
            },
        );
        let clear = wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT);
        let g = cached(t, [tag::PREFILTER, src.id, exposure.1, chain_id], |_| {
            make(src.view)
        });
        fullscreen(
            encoder,
            "yowie bloom prefilter",
            &mips[0],
            clear,
            &prefilter,
            [&t.common, &g],
            None,
            None,
        );
        let mut draws = 1;
        for l in 1..mips.len() {
            let g = cached(t, [tag::DOWN, chain_id, l as u64, exposure.1], |_| {
                make(&mips[l - 1])
            });
            fullscreen(
                encoder,
                "yowie bloom down",
                &mips[l],
                clear,
                &down,
                [&t.common, &g],
                None,
                None,
            );
            draws += 1;
        }
        for l in (0..mips.len().saturating_sub(1)).rev() {
            let g = cached(t, [tag::UP, chain_id, l as u64, exposure.1], |_| {
                make(&mips[l + 1])
            });
            fullscreen(
                encoder,
                "yowie bloom up",
                &mips[l],
                wgpu::LoadOp::Load,
                &up,
                [&t.common, &g],
                None,
                None,
            );
            draws += 1;
        }
        draws
    }
}
