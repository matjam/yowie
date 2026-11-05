//! The scene renderer: owns the scene pipelines, slots and caches, and
//! records Pass 1 (shadow atlas and HDR scene) for each scene of an
//! output.

use yowie_core::{Color, Rect, UVec2};

use super::frame_data::{FrameUniform, fill_frame, sky_key};
use super::ibl::{EnvMaps, IblBaker, cube_view};
use super::layouts::{HDR_FORMAT, SceneLayouts};
use super::pipelines::ScenePipelines;
use super::prepare::{Prepared, View, prepare};
use super::record::SceneRecord;
use super::shadow::ShadowAtlas;
use super::slot::SceneSlot;
use super::textures::MaterialTextures;
use crate::batch::{DrawList, ListTarget};
use crate::graphics::FrameStats;
use crate::shared::Shared;
use crate::view::ViewMapping;

/// Frames a baked sky may go unused before it is dropped.
const SKY_KEEP_FRAMES: u64 = 300;
/// The most baked skies kept at once.
const MAX_SKIES: usize = 4;
/// The cube face size a procedural sky is baked at.
const SKY_BAKE_SIZE: u32 = 64;

/// A baked sky kept for reuse while the sky does not change.
#[derive(Debug)]
pub(super) struct SkyMaps {
    key: u64,
    maps: EnvMaps,
    used: u64,
    id: u64,
}

/// What one scene rendered: its slot and where it lands in the output.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Rendered {
    pub(crate) slot: usize,
    pub(crate) viewport: Rect,
}

/// Everything the 3D renderer keeps between frames.
#[derive(Debug)]
pub(crate) struct SceneRenderer {
    pub(super) layouts: SceneLayouts,
    pub(super) pipelines: ScenePipelines,
    pub(super) textures: MaterialTextures,
    baker: Option<IblBaker>,
    skies: Vec<SkyMaps>,
    no_env: EnvMaps,
    pub(super) atlas: Option<ShadowAtlas>,
    pub(super) empty_atlas: ShadowAtlas,
    pub(super) shadow_sampler: wgpu::Sampler,
    pub(super) linear_sampler: wgpu::Sampler,
    pub(super) morph_none: wgpu::BindGroup,
    free: Vec<SceneRecord>,
    pub(super) records: Vec<SceneRecord>,
    pub(super) slots: Vec<SceneSlot>,
    prepared: Prepared,
    frame: u64,
    /// Bumped whenever a slot buffer is replaced, so bind groups rebuild.
    pub(super) generation: u64,
}

fn black_cube(device: &wgpu::Device) -> wgpu::TextureView {
    let t = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("yowie empty environment"),
        size: wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 6,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: HDR_FORMAT,
        usage: wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    cube_view(&t, 0, None)
}

impl SceneRenderer {
    pub(crate) fn new(shared: &Shared) -> Self {
        let device = shared.device();
        let layouts = SceneLayouts::new(device);
        let mut encoder = device.create_command_encoder(&Default::default());
        let empty_atlas = ShadowAtlas::empty(device, &mut encoder);
        shared.queue().submit([encoder.finish()]);
        let morph_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("yowie no morph"),
            size: 32,
            usage: wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        });
        let morph_none = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("yowie no morph"),
            layout: &layouts.morph,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: morph_buf.as_entire_binding(),
            }],
        });
        let textures = match MaterialTextures::new(shared) {
            Ok(t) => t,
            // The stand-ins are 1x1 textures, which every device can make.
            Err(e) => unreachable!("1x1 material textures: {e}"),
        };
        Self {
            pipelines: ScenePipelines::new(device),
            textures,
            baker: None,
            skies: Vec::new(),
            no_env: EnvMaps {
                specular: black_cube(device),
                irradiance: black_cube(device),
                specular_mips: 1,
            },
            atlas: None,
            empty_atlas,
            shadow_sampler: device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("yowie shadow"),
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                compare: Some(wgpu::CompareFunction::LessEqual),
                ..Default::default()
            }),
            linear_sampler: device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("yowie scene linear"),
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                mipmap_filter: wgpu::MipmapFilterMode::Linear,
                ..Default::default()
            }),
            morph_none,
            layouts,
            free: Vec::new(),
            records: Vec::new(),
            slots: Vec::new(),
            prepared: Prepared::default(),
            frame: 0,
            generation: 0,
        }
    }

    pub(crate) fn begin_frame(&mut self) {
        self.free.append(&mut self.records);
        self.textures.begin_frame();
        self.frame += 1;
        let frame = self.frame;
        self.skies.retain(|s| frame - s.used < SKY_KEEP_FRAMES);
    }

    pub(crate) fn take_record(&mut self) -> SceneRecord {
        self.free.pop().unwrap_or_default()
    }

    pub(crate) fn push_record(&mut self, rec: SceneRecord) {
        self.records.push(rec);
    }

    /// Prepares the draw lists of outputs that have scenes: the scene
    /// resolve takes over clearing, so the 2D pass loads instead, and the
    /// letterbox backdrop the 2D list starts with is dropped because the
    /// resolve composes the clear colour behind the scene itself.
    pub(crate) fn prepare_lists(&mut self, lists: &mut [DrawList], clear_color: Color) {
        let mut done = vec![false; lists.len()];
        for rec in &mut self.records {
            let Some(list) = lists.get_mut(rec.list) else {
                continue;
            };
            let letterboxed =
                matches!(list.target, ListTarget::Screen) && list.mapping.letterboxed();
            rec.background_color = if letterboxed {
                clear_color.to_linear_premultiplied()
            } else {
                [0.0; 4]
            };
            if !done[rec.list] {
                done[rec.list] = true;
                rec.output_clear = list.clear.take();
                if letterboxed && list.items.first().is_some_and(|i| i.layer == i32::MIN) {
                    list.items.remove(0);
                }
            } else {
                rec.output_clear = None;
            }
        }
    }

    /// Whether any scene renders into list `li`.
    pub(crate) fn has_scenes(&self, li: usize) -> bool {
        self.records.iter().any(|r| r.list == li)
    }

    /// The clear colour output `li` gave up to its scenes.
    pub(crate) fn output_clear(&self, li: usize) -> Option<Color> {
        self.records
            .iter()
            .find(|r| r.list == li)
            .and_then(|r| r.output_clear)
    }

    /// Pass 1 for output `li`: renders each of its scenes into its slot's
    /// HDR targets and returns where each lands in the output.
    pub(crate) fn render_list(
        &mut self,
        shared: &Shared,
        encoder: &mut wgpu::CommandEncoder,
        li: usize,
        mapping: &ViewMapping,
        stats: &mut FrameStats,
    ) -> Vec<Rendered> {
        let mut out = Vec::new();
        for ri in 0..self.records.len() {
            if self.records[ri].list != li {
                continue;
            }
            self.ensure_slot(shared, ri);
            if let Some(viewport) = self.render_scene(shared, encoder, ri, mapping, stats) {
                out.push(Rendered { slot: ri, viewport });
            }
        }
        out
    }

    fn ensure_slot(&mut self, shared: &Shared, ri: usize) {
        if self.slots.len() <= ri {
            self.slots
                .resize_with(ri + 1, || SceneSlot::new(shared.device()));
        }
    }

    fn render_scene(
        &mut self,
        shared: &Shared,
        encoder: &mut wgpu::CommandEncoder,
        ri: usize,
        mapping: &ViewMapping,
        stats: &mut FrameStats,
    ) -> Option<Rect> {
        let device = shared.device();
        let rec = &self.records[ri];
        let out_size = mapping.output_size.as_vec2();
        let vp = mapping
            .rect_to_pixels(rec.camera.viewport_rect(mapping.view_size))
            .intersection(&Rect::new(0.0, 0.0, out_size.x, out_size.y))?;
        let vp = Rect::new(vp.x.floor(), vp.y.floor(), vp.w.round(), vp.h.round());
        if vp.w < 1.0 || vp.h < 1.0 {
            return None;
        }
        let size = UVec2::new(vp.w as u32, vp.h as u32);
        let v = View::new(&rec.camera, size.as_vec2());
        let mut prepared = std::mem::take(&mut self.prepared);
        prepare(rec, &v, &mut prepared);
        fill_frame(rec, &v, &mut prepared.frame);
        let env = self.choose_env(shared, encoder, ri, &mut prepared.frame);
        self.slots[ri].ensure_targets(device, size);
        self.upload(shared, ri, &prepared);
        let any_shadows = !prepared.shadows.is_empty();
        if any_shadows && self.atlas.is_none() {
            self.atlas = Some(ShadowAtlas::new(device));
        }
        self.bind_groups(shared, ri, env, any_shadows);
        let rec = &self.records[ri];
        for m in &rec.materials {
            self.textures.prepare(shared, &self.layouts, m);
        }
        for mesh in &rec.meshes {
            if let Some(morph) = &mesh.inner.morph {
                morph.bind_group.get_or_init(|| {
                    device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("yowie morph"),
                        layout: &self.layouts.morph,
                        entries: &[wgpu::BindGroupEntry {
                            binding: 0,
                            resource: morph.buffer.as_entire_binding(),
                        }],
                    })
                });
            }
        }
        let draws = rec.draws.len() as u32;
        let calls = super::passes::encode_scene(self, shared, encoder, ri, &prepared);
        stats.draw_calls_3d += calls;
        stats.draws_3d += draws;
        stats.culled_3d += prepared.culled;
        stats.lights += prepared.lights.len() as u32;
        stats.lights_dropped += prepared.lights_dropped;
        stats.shadow_instances += prepared.shadow_instances;
        self.prepared = prepared;
        Some(vp)
    }

    /// Picks the maps the scene lights with and records a sky bake when
    /// the sky changed. Returns the environment's id (0 for none).
    fn choose_env(
        &mut self,
        shared: &Shared,
        encoder: &mut wgpu::CommandEncoder,
        ri: usize,
        f: &mut FrameUniform,
    ) -> u64 {
        let rec = &self.records[ri];
        let background = rec.background;
        if let Some(env) = &rec.environment {
            f.env = [
                env.inner.intensity,
                env.inner.maps.specular_mips as f32,
                1.0,
                if background { 2.0 } else { 0.0 },
            ];
            return env.inner.id;
        }
        if rec.sky.is_none() {
            return 0;
        }
        f.env[3] = if background { 1.0 } else { 0.0 };
        let key = sky_key(f);
        let frame = self.frame;
        if let Some(s) = self.skies.iter_mut().find(|s| s.key == key) {
            s.used = frame;
            f.env[0] = 1.0;
            f.env[1] = s.maps.specular_mips as f32;
            f.env[2] = 1.0;
            return s.id;
        }
        // The bake reads the slot's frame block, so write it first.
        self.ensure_slot(shared, ri);
        let slot = &self.slots[ri];
        shared
            .queue()
            .write_buffer(&slot.frame, 0, bytemuck::bytes_of(f));
        let baker = self
            .baker
            .get_or_insert_with(|| IblBaker::new(shared.device()));
        let maps = baker.bake(
            shared.device(),
            shared.queue(),
            encoder,
            &slot.frame,
            None,
            SKY_BAKE_SIZE,
        );
        f.env[0] = 1.0;
        f.env[1] = maps.specular_mips as f32;
        f.env[2] = 1.0;
        let id = crate::texture::next_id();
        if self.skies.len() >= MAX_SKIES {
            self.skies.remove(0);
        }
        self.skies.push(SkyMaps {
            key,
            maps,
            used: frame,
            id,
        });
        id
    }

    /// The environment maps of slot `ri`'s scene, by the id `choose_env`
    /// returned.
    pub(super) fn env_maps(&self, ri: usize, env: u64) -> &EnvMaps {
        if let Some(e) = &self.records[ri].environment {
            return &e.inner.maps;
        }
        self.skies
            .iter()
            .find(|s| s.id == env)
            .map_or(&self.no_env, |s| &s.maps)
    }

    pub(super) fn atlas_target(&self) -> Option<&wgpu::TextureView> {
        self.atlas.as_ref().map(|a| &a.target_view)
    }
}
