//! The steps of a scene's chain before bloom and the composite, in HDR:
//! the depth copy and pyramid, ambient occlusion, reflections, light
//! shafts, the velocity of moved meshes, temporal anti-aliasing, motion
//! blur, depth of field and auto exposure. Each full-size step reads the
//! image holding the picture and writes the spare one, so nothing is
//! copied.

use super::chain::{Input, cached, mix, tex};
use super::pipelines::{Blend, PipeKey, Program};
use super::settings::PostSettings;
use super::targets::{DEPTH, HDR, Kind, PostTargets};
use super::{PostRenderer, fullscreen};
use crate::scene::record::SceneRecord;
use crate::scene::targets::Targets;

/// A view and the id its bind groups are cached by.
#[derive(Clone, Debug)]
pub(crate) struct Owned {
    pub(crate) view: wgpu::TextureView,
    pub(crate) id: u64,
}

impl Owned {
    pub(crate) fn input(&self) -> Input<'_> {
        Input {
            view: &self.view,
            id: self.id,
        }
    }

    fn of(t: &mut PostTargets, device: &wgpu::Device, kind: Kind) -> Owned {
        let (view, id) = t.view(device, kind);
        Owned { view, id }
    }
}

/// Which steps run this frame. It is settled before the post block is
/// written, since the block carries their flags.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Plan {
    pub(crate) occlusion: bool,
    pub(crate) reflections: bool,
    pub(crate) rays: bool,
    pub(crate) velocity: bool,
    pub(crate) temporal: bool,
    pub(crate) motion: bool,
    pub(crate) focus: bool,
    pub(crate) exposure: bool,
}

impl Plan {
    /// The steps `settings` turn on that the scene's images allow: the
    /// HDR pass wrote the surface and specular images the settings asked
    /// for, unless the frame they changed on has not rendered yet.
    pub(crate) fn new(
        settings: &PostSettings,
        scene: &Targets,
        rec: &SceneRecord,
        sun_on_screen: bool,
        compute: bool,
    ) -> Plan {
        let temporal = settings.temporal_aa;
        let motion = settings.wants_motion_blur();
        Plan {
            occlusion: settings.wants_occlusion() && scene.surface.is_some(),
            reflections: settings.wants_reflections() && scene.specular.is_some(),
            rays: settings.god_rays > 0.0 && sun_on_screen,
            velocity: (temporal || motion) && !rec.moved.is_empty(),
            temporal,
            motion,
            focus: settings.wants_dof(),
            exposure: settings.auto_exposure && compute,
        }
    }

    fn depth(&self) -> bool {
        self.occlusion
            || self.reflections
            || self.rays
            || self.temporal
            || self.motion
            || self.focus
    }
}

/// What the steps leave for bloom and the composite.
#[derive(Debug)]
pub(crate) struct Stepped {
    pub(crate) picture: Owned,
    pub(crate) ao: Option<Owned>,
    pub(crate) rays: Option<Owned>,
    pub(crate) exposure: Option<(wgpu::Buffer, u64)>,
    pub(crate) draws: u32,
}

/// Bind group cache tags of the steps start here, plus the program.
const TAG_BASE: u64 = 100;

impl PostRenderer {
    /// Records one full-screen pass of `program`'s `entry`, reading
    /// `inputs` (group 1 in binding order) and writing all of `target`.
    #[allow(clippy::too_many_arguments)]
    fn step(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        t: &mut PostTargets,
        program: Program,
        entry: &'static str,
        inputs: &[Input<'_>],
        target: &wgpu::TextureView,
        format: wgpu::TextureFormat,
    ) -> u32 {
        let ids: Vec<u64> = inputs.iter().map(|i| i.id).collect();
        let layout = self.pipes.group(program).clone();
        let group = cached(t, [TAG_BASE + program as u64, mix(&ids), 0, 0], |_| {
            let entries: Vec<_> = inputs
                .iter()
                .enumerate()
                .map(|(i, input)| tex(i as u32, input.view))
                .collect();
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("yowie post step"),
                layout: &layout,
                entries: &entries,
            })
        });
        let pipe = self.pipes.get(
            device,
            PipeKey {
                program,
                entry,
                format,
                blend: Blend::Replace,
            },
        );
        let clear = wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT);
        fullscreen(
            encoder,
            entry,
            target,
            clear,
            &pipe,
            [&t.common, &group],
            None,
            None,
        );
        1
    }

    /// Records the steps `plan` turns on for the scene in `scene`.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn hdr_steps(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        t: &mut PostTargets,
        plan: &Plan,
        scene: &Targets,
        rec: &SceneRecord,
    ) -> Stepped {
        let mut draws = 0;
        let mut ping = 0u8;
        let mut next = |t: &mut PostTargets| {
            let o = Owned::of(t, device, Kind::Ping(ping));
            ping ^= 1;
            o
        };
        let black = Owned {
            view: self.dummies.black.clone(),
            id: mix(&[self.dummies.id, 1]),
        };
        let white = Owned {
            view: self.dummies.white.clone(),
            id: mix(&[self.dummies.id, 2]),
        };
        let mut picture = Owned {
            view: scene.color.clone(),
            id: scene.id,
        };
        let extra = |view: &Option<wgpu::TextureView>, n: u64| {
            view.as_ref().map(|v| Owned {
                view: v.clone(),
                id: mix(&[scene.id, n]),
            })
        };
        let surface = extra(&scene.surface, 1);
        let specular = extra(&scene.specular, 2);
        if !plan.depth() {
            let exposure = self.exposure_step(device, queue, encoder, t, plan, &picture);
            return Stepped {
                picture,
                ao: None,
                rays: None,
                exposure,
                draws,
            };
        }

        // The depth copy, resolving multisampled depth, and its pyramid.
        let mips = t.depth_mips(device);
        let (src, program, entry) = match &scene.msaa {
            Some(m) => (&m.depth, Program::DepthCopyMs, "fs_depth_ms"),
            None => (&scene.depth, Program::DepthCopy, "fs_depth"),
        };
        let src = Input {
            view: src,
            id: mix(&[scene.id, 3]),
        };
        draws += self.step(
            device,
            encoder,
            t,
            program,
            entry,
            &[src],
            &mips[0].0,
            DEPTH,
        );
        if plan.reflections {
            for l in 1..mips.len() {
                let finer = Input {
                    view: &mips[l - 1].0,
                    id: mips[l - 1].1,
                };
                draws += self.step(
                    device,
                    encoder,
                    t,
                    Program::DepthMip,
                    "fs_depth_mip",
                    &[finer],
                    &mips[l].0,
                    DEPTH,
                );
            }
        }
        let depth = Owned::of(t, device, Kind::Depth);
        let d = depth.input();

        // Ambient occlusion and its blur.
        let ao = match (&surface, plan.occlusion) {
            (Some(surface), true) => {
                let a = Owned::of(t, device, Kind::Occlusion(0));
                let b = Owned::of(t, device, Kind::Occlusion(1));
                let r8 = wgpu::TextureFormat::R8Unorm;
                draws += self.step(
                    device,
                    encoder,
                    t,
                    Program::Gtao,
                    "fs_gtao",
                    &[d, surface.input()],
                    &a.view,
                    r8,
                );
                draws += self.step(
                    device,
                    encoder,
                    t,
                    Program::AoBlur,
                    "fs_blur_x",
                    &[a.input(), d],
                    &b.view,
                    r8,
                );
                draws += self.step(
                    device,
                    encoder,
                    t,
                    Program::AoBlur,
                    "fs_blur_y",
                    &[b.input(), d],
                    &a.view,
                    r8,
                );
                Some(a)
            }
            _ => None,
        };

        // Reflections, then both applied to the picture.
        let reflection = match (&surface, plan.reflections) {
            (Some(surface), true) => {
                let r = Owned::of(t, device, Kind::Reflection);
                let inputs = [d, surface.input(), picture.input()];
                draws += self.step(
                    device,
                    encoder,
                    t,
                    Program::Ssr,
                    "fs_ssr",
                    &inputs,
                    &r.view,
                    HDR,
                );
                Some(r)
            }
            _ => None,
        };
        if ao.is_some() || reflection.is_some() {
            let out = next(t);
            let inputs = [
                picture.input(),
                ao.as_ref().unwrap_or(&white).input(),
                surface.as_ref().unwrap_or(&black).input(),
                specular.as_ref().unwrap_or(&black).input(),
                reflection.as_ref().unwrap_or(&black).input(),
            ];
            draws += self.step(
                device,
                encoder,
                t,
                Program::Apply,
                "fs_apply",
                &inputs,
                &out.view,
                HDR,
            );
            picture = out;
        }

        // Light shafts, which the composite adds.
        let rays = plan.rays.then(|| Owned::of(t, device, Kind::Rays));
        if let Some(r) = &rays {
            draws += self.step(
                device,
                encoder,
                t,
                Program::Rays,
                "fs_rays",
                &[d],
                &r.view,
                HDR,
            );
        }

        // Motion of moved meshes, for the temporal steps.
        let velocity = if plan.velocity && self.velocity(device, queue, encoder, t, rec, d) {
            draws += 1;
            Owned::of(t, device, Kind::Velocity)
        } else {
            black.clone()
        };
        if plan.temporal {
            let previous = Owned::of(t, device, Kind::History(t.history_next ^ 1));
            let out = Owned::of(t, device, Kind::History(t.history_next));
            let inputs = [picture.input(), previous.input(), d, velocity.input()];
            draws += self.step(
                device,
                encoder,
                t,
                Program::Taa,
                "fs_taa",
                &inputs,
                &out.view,
                HDR,
            );
            t.history_next ^= 1;
            t.history_valid = true;
            picture = out;
        } else {
            t.history_valid = false;
        }
        if plan.motion {
            let out = next(t);
            let inputs = [picture.input(), d, velocity.input()];
            draws += self.step(
                device,
                encoder,
                t,
                Program::Motion,
                "fs_motion",
                &inputs,
                &out.view,
                HDR,
            );
            picture = out;
        }

        // Depth of field: the half-size gather, then the combine.
        if plan.focus {
            let half = Owned::of(t, device, Kind::Focus);
            let inputs = [picture.input(), d];
            draws += self.step(
                device,
                encoder,
                t,
                Program::Focus,
                "fs_focus",
                &inputs,
                &half.view,
                HDR,
            );
            let out = next(t);
            let inputs = [picture.input(), half.input(), d];
            draws += self.step(
                device,
                encoder,
                t,
                Program::FocusCombine,
                "fs_focus_combine",
                &inputs,
                &out.view,
                HDR,
            );
            picture = out;
        }
        let exposure = self.exposure_step(device, queue, encoder, t, plan, &picture);
        Stepped {
            picture,
            ao,
            rays,
            exposure,
            draws,
        }
    }

    fn exposure_step(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        t: &mut PostTargets,
        plan: &Plan,
        picture: &Owned,
    ) -> Option<(wgpu::Buffer, u64)> {
        if !plan.exposure {
            return None;
        }
        self.auto_exposure(device, queue, encoder, t, picture.input())
    }
}
