//! Baking probes: the `Graphics` and `Scene` calls games make, and the
//! captures behind them.

use yowie_core::{Color, Mat4, Quat, Vec2, Vec3};

use super::Capture;
use super::capture::FACES;
use super::grid::LightProbeGrid;
use super::reflection::{ProbeUse, ReflectionProbe};
use super::sh::{self, CELL_FLOATS};
use super::state::{PROBE_MIPS, PROBE_SIZE};
use crate::Graphics;
use crate::error::{GfxError, Result};
use crate::graphics::FrameStats;
use crate::half::f16_to_f32;
use crate::scene::camera::Camera3D;
use crate::scene::ibl::{IblBaker, face_view};
use crate::scene::layouts::HDR_FORMAT;
use crate::scene::material::Material;
use crate::scene::mesh::Mesh;
use crate::scene::render::SceneRenderer;
use crate::scene::submit::{DrawId, Scene};
use crate::shared::Shared;

/// Cells captured before their faces are read back and projected.
const CELLS_PER_READBACK: usize = 64;

fn cube(device: &wgpu::Device, size: u32, mips: u32, usage: wgpu::TextureUsages) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("yowie probe cube"),
        size: wgpu::Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 6,
        },
        mip_level_count: mips,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: HDR_FORMAT,
        usage: usage
            | wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    })
}

fn range(near: f32, far: f32) -> (f32, f32) {
    let near = near.max(1e-4);
    (near, far.max(near * 1.01))
}

impl Graphics {
    /// Captures what `scene` draws into `probe` from its position and
    /// prefilters it; the probe and its clones keep the result. The
    /// scene's lights, sun, sky or environment and light probe grid light
    /// the capture; its camera is ignored. The work is submitted at once
    /// and leaves the frame being drawn alone, so call it at load or
    /// whenever the room changes. Returns what `scene` returns.
    pub fn bake_reflection_probe<R>(
        &mut self,
        probe: &ReflectionProbe,
        scene: impl FnOnce(&mut Scene<'_>) -> R,
    ) -> R {
        let shared = &self.shared;
        let r = self
            .scenes
            .get_or_insert_with(|| Box::new(SceneRenderer::new(shared)));
        let (ri, out) = r.push_bake_record(scene);
        r.bake_probe(shared, ri, probe, None);
        r.pop_bake_record();
        out
    }

    /// Bakes `grid`: captures a small cube of what `scene` draws at each
    /// cell, projects each onto spherical harmonics and keeps them in the
    /// grid. Cells are captured with the scene's lights, sun and sky or
    /// environment, without reflection probes or another grid. Blocks
    /// until the GPU finishes (a grid of a few hundred cells takes a
    /// fraction of a second); save the result with
    /// [`LightProbeGrid::to_bytes`] to skip the bake next time. Fails when
    /// the grid is malformed or too large for the device.
    pub fn bake_light_probes(
        &mut self,
        grid: &mut LightProbeGrid,
        scene: impl FnOnce(&mut Scene<'_>),
    ) -> Result<()> {
        grid.validate(self.shared.device().limits().max_texture_dimension_3d)?;
        let shared = &self.shared;
        let r = self
            .scenes
            .get_or_insert_with(|| Box::new(SceneRenderer::new(shared)));
        let (ri, ()) = r.push_bake_record(scene);
        let result = r.bake_grid(shared, ri, grid);
        r.pop_bake_record();
        result
    }
}

impl Scene<'_> {
    /// Shows a reflection probe: shiny surfaces inside its box reflect its
    /// capture. A probe that holds no capture yet, or was asked to with
    /// [`ReflectionProbe::request_bake`], or whose
    /// [`update`](ReflectionProbe::update) is due, is captured from this
    /// scene's draws before the scene renders. A scene shows up to
    /// [`MAX_REFLECTION_PROBES`](super::MAX_REFLECTION_PROBES) probes (one
    /// on GPUs without cube arrays); the first added are kept.
    pub fn add_reflection_probe(&mut self, probe: &ReflectionProbe) {
        if self.rec.probes.len() < super::MAX_REFLECTION_PROBES {
            self.rec.probes.push(ProbeUse {
                probe: probe.clone(),
            });
        }
    }

    /// Lights the scene's diffuse surfaces with a baked light probe grid
    /// where it reaches; `None` (the default) uses the environment or
    /// ambient colour everywhere. An unbaked grid is ignored.
    pub fn set_light_probes(&mut self, grid: Option<&LightProbeGrid>) {
        self.rec.light_grid = grid.cloned();
    }

    /// Leaves a draw out of probe captures taken from this scene, so a
    /// shiny ball does not capture itself into the probe it reflects.
    pub fn hide_in_probes(&mut self, draw: DrawId) {
        let hidden = &mut self.rec.probe_hidden;
        if let Err(at) = hidden.binary_search(&draw.0) {
            hidden.insert(at, draw.0);
        }
    }

    /// Draws a matte white sphere `radius` world units across at each of
    /// `grid`'s cells, to show where the cells are and, with the grid set
    /// through [`set_light_probes`](Self::set_light_probes), the light each
    /// one holds. `sphere` is a sphere mesh of radius 1, such as
    /// `MeshData::sphere(1.0, 16, 32)`. The spheres are left out of probe
    /// captures.
    pub fn draw_light_probes(&mut self, grid: &LightProbeGrid, sphere: &Mesh, radius: f32) {
        let white = Material::color(Color::WHITE, 0.0, 1.0);
        let scale = Vec3::splat(radius);
        let models: Vec<Mat4> = grid
            .cell_positions()
            .map(|p| Mat4::from_scale_rotation_translation(scale, Quat::IDENTITY, p))
            .collect();
        let id = self.draw_instanced(sphere, &white, &models);
        self.hide_in_probes(id);
    }
}

impl SceneRenderer {
    /// Records `scene` into a record that is not part of the frame and
    /// returns its index; [`pop_bake_record`](Self::pop_bake_record) takes
    /// it out again.
    fn push_bake_record<R>(&mut self, scene: impl FnOnce(&mut Scene<'_>) -> R) -> (usize, R) {
        let mut rec = self.take_record();
        rec.reset(usize::MAX, Camera3D::default(), Vec2::ONE);
        let out = scene(&mut Scene { rec: &mut rec });
        self.push_record(rec);
        (self.records.len() - 1, out)
    }

    fn pop_bake_record(&mut self) {
        if let Some(mut rec) = self.records.pop() {
            rec.statics.clear();
            self.recycle(rec);
        }
    }

    /// Captures the probes of this frame's scenes that are due, each in
    /// its own submission. Runs before the frame records anything.
    pub(crate) fn refresh_probes(&mut self, shared: &Shared, stats: &mut FrameStats) {
        let frame = self.frame;
        for ri in 0..self.records.len() {
            let due: Vec<ReflectionProbe> = self.records[ri]
                .probes
                .iter()
                .filter(|u| u.probe.due(frame))
                .map(|u| u.probe.clone())
                .collect();
            for probe in due {
                self.bake_probe(shared, ri, &probe, Some(frame));
                stats.probe_faces += FACES.len() as u32;
            }
        }
    }

    /// Captures record `ri` into `probe`, prefilters it and submits the
    /// work. `frame` is the renderer frame for captures taken from a
    /// frame's scene.
    fn bake_probe(
        &mut self,
        shared: &Shared,
        ri: usize,
        probe: &ReflectionProbe,
        frame: Option<u64>,
    ) {
        let device = shared.device();
        let size = probe.resolution.clamp(8, 1024).next_power_of_two();
        let mips = size.trailing_zeros() + 1;
        let radiance = cube(device, size, mips, wgpu::TextureUsages::empty());
        let faces: Vec<wgpu::TextureView> = (0..6).map(|f| face_view(&radiance, f, 0)).collect();
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("yowie probe capture"),
        });
        self.prepare_resources(shared, ri);
        self.capture_faces(
            shared,
            &mut encoder,
            ri,
            probe.position,
            range(probe.near, probe.far),
            size,
            &faces,
            Capture::Probe,
        );
        let specular = cube(
            device,
            PROBE_SIZE,
            PROBE_MIPS,
            wgpu::TextureUsages::COPY_SRC,
        );
        let baker = self.baker.get_or_insert_with(|| IblBaker::new(device));
        baker.filter(
            device,
            shared.queue(),
            &mut encoder,
            &self.captures[0].frame,
            &radiance,
            &specular,
        );
        shared.queue().submit([encoder.finish()]);
        probe.store(specular, frame);
    }

    /// Captures every cell of `grid` from record `ri` and keeps the
    /// projected coefficients in it.
    fn bake_grid(&mut self, shared: &Shared, ri: usize, grid: &mut LightProbeGrid) -> Result<()> {
        let (device, queue) = (shared.device(), shared.queue());
        let size = grid.resolution.clamp(4, 128);
        let faces_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("yowie light probe capture"),
            size: wgpu::Extent3d {
                width: size,
                height: size,
                depth_or_array_layers: 6,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: HDR_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let faces: Vec<wgpu::TextureView> = (0..6).map(|f| face_view(&faces_tex, f, 0)).collect();
        let row = (size * 8).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
            * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let cell_bytes = u64::from(row * size * 6);
        let positions: Vec<Vec3> = grid.cell_positions().collect();
        let range = range(grid.near, grid.far);
        self.prepare_resources(shared, ri);
        let mut cells: Vec<[f32; CELL_FLOATS]> = Vec::with_capacity(positions.len());
        for chunk in positions.chunks(CELLS_PER_READBACK) {
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("yowie light probe readback"),
                size: cell_bytes * chunk.len() as u64,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            for (k, p) in chunk.iter().enumerate() {
                // One submission per cell: the capture slots are rewritten
                // for each.
                let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("yowie light probe capture"),
                });
                self.capture_faces(
                    shared,
                    &mut encoder,
                    ri,
                    *p,
                    range,
                    size,
                    &faces,
                    Capture::Grid,
                );
                encoder.copy_texture_to_buffer(
                    wgpu::TexelCopyTextureInfo {
                        texture: &faces_tex,
                        mip_level: 0,
                        origin: wgpu::Origin3d::ZERO,
                        aspect: wgpu::TextureAspect::All,
                    },
                    wgpu::TexelCopyBufferInfo {
                        buffer: &buffer,
                        layout: wgpu::TexelCopyBufferLayout {
                            offset: cell_bytes * k as u64,
                            bytes_per_row: Some(row),
                            rows_per_image: Some(size),
                        },
                    },
                    faces_tex.size(),
                );
                queue.submit([encoder.finish()]);
            }
            read_cells(
                device,
                &buffer,
                chunk.len(),
                size as usize,
                row as usize,
                &mut cells,
            )?;
        }
        grid.set_cells(cells);
        Ok(())
    }
}

/// Maps `buffer` (`count` cells of six faces, `size` texels square, rows
/// `row` bytes apart), projects each cell and appends it to `out`.
fn read_cells(
    device: &wgpu::Device,
    buffer: &wgpu::Buffer,
    count: usize,
    size: usize,
    row: usize,
    out: &mut Vec<[f32; CELL_FLOATS]>,
) -> Result<()> {
    let fail = |e: String| GfxError::Probes(format!("cannot read the capture back: {e}"));
    let (tx, rx) = std::sync::mpsc::channel();
    buffer.map_async(wgpu::MapMode::Read, .., move |r| {
        // The receiver waits below; a failed send means nobody listens.
        let _ = tx.send(r);
    });
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .map_err(|e| fail(e.to_string()))?;
    rx.recv()
        .map_err(|e| fail(e.to_string()))?
        .map_err(|e| fail(e.to_string()))?;
    {
        let mapped = buffer
            .get_mapped_range(..)
            .map_err(|e| fail(e.to_string()))?;
        let mut texels = Vec::with_capacity(6 * size * size);
        for cell in mapped.chunks(row * size * 6).take(count) {
            texels.clear();
            for line in cell.chunks(row) {
                for px in line[..size * 8].as_chunks::<8>().0 {
                    let h = |i: usize| f16_to_f32(u16::from_le_bytes([px[i], px[i + 1]]));
                    texels.push([h(0), h(2), h(4)]);
                }
            }
            out.push(sh::project(&texels, size));
        }
    }
    buffer.unmap();
    Ok(())
}
