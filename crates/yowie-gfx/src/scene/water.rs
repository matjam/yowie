//! Water: a level plane over a rectangle, coloured by its depth (read from
//! a terrain's heights), reflecting the sky and sun off a surface of
//! moving waves, and fading into the shore.

use std::sync::Arc;

use bytemuck::{Pod, Zeroable};
use parking_lot::Mutex;
use yowie_core::{Color, Vec2, Vec3};

use super::large::{LargeRecord, PrepareContext};
use super::large_pipelines::LargePipelines;
use super::submit::Scene;
use super::terrain::Terrain;
use crate::Graphics;

/// How a [`Water`] surface looks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WaterOptions {
    /// The middle of the surface; its `y` is the water level.
    pub center: Vec3,
    /// The surface's extent along X and Z in world units.
    pub size: Vec2,
    /// The colour of shallow water, sRGB.
    pub shallow_color: Color,
    /// The colour of deep water, sRGB.
    pub deep_color: Color,
    /// The depth in world units at which the water is fully deep in colour
    /// and at its most opaque.
    pub depth_range: f32,
    /// How opaque deep water is, 0 to 1; the surface's reflection adds to
    /// it at grazing angles.
    pub opacity: f32,
    /// The surface's roughness: low values give sharp reflections.
    pub roughness: f32,
    /// How strongly the waves tilt the surface, 0 for a mirror.
    pub waves: f32,
    /// The length of the largest waves in world units.
    pub wave_length: f32,
    /// How fast the waves travel, in world units per second.
    pub wave_speed: f32,
    /// The width in world units of the foam along the shore; zero for
    /// none.
    pub foam: f32,
    /// The depth used where there is no terrain under the water.
    pub floor_depth: f32,
}

impl Default for WaterOptions {
    fn default() -> Self {
        Self {
            center: Vec3::ZERO,
            size: Vec2::splat(100.0),
            shallow_color: Color::rgb8(60, 150, 150),
            deep_color: Color::rgb8(10, 40, 70),
            depth_range: 4.0,
            opacity: 0.96,
            roughness: 0.06,
            waves: 0.35,
            wave_length: 6.0,
            wave_speed: 1.2,
            foam: 0.35,
            floor_depth: 10.0,
        }
    }
}

/// The water block the shaders read; matches `WaterParams` in
/// `shaders/water.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
struct WaterParamsGpu {
    area: [f32; 4],
    /// Level, time, depth range, floor depth.
    level: [f32; 4],
    /// Linear shallow colour and opacity.
    shallow: [f32; 4],
    /// Linear deep colour and roughness.
    deep: [f32; 4],
    /// Strength, one over the wave length, speed, foam width.
    waves: [f32; 4],
    /// Terrain columns, rows, 1 when there is a terrain, 0.
    grid: [u32; 4],
    /// Terrain origin and cell size.
    origin: [f32; 4],
}

#[derive(Debug)]
struct WaterInner {
    options: Mutex<WaterOptions>,
    params: wgpu::Buffer,
    terrain: Option<Terrain>,
    /// Stands in for the heights when there is no terrain.
    no_heights: wgpu::Buffer,
    group: Mutex<Option<wgpu::BindGroup>>,
}

/// A water surface; see the module documentation. Cloning shares it.
#[derive(Clone, Debug)]
pub struct Water {
    inner: Arc<WaterInner>,
}

/// One recorded water draw.
#[derive(Clone, Debug)]
pub(crate) struct WaterDraw {
    water: Water,
}

impl Water {
    /// A water surface over flat ground `floor_depth` below it.
    pub fn new(gfx: &Graphics, options: WaterOptions) -> Water {
        Self::create(gfx, options, None)
    }

    /// A water surface whose depth at each point is the water level minus
    /// the height of `terrain` there, so it is shallow and clear at the
    /// shore and dark in the middle. Edits of the terrain show at once.
    pub fn over_terrain(gfx: &Graphics, terrain: &Terrain, options: WaterOptions) -> Water {
        Self::create(gfx, options, Some(terrain.clone()))
    }

    fn create(gfx: &Graphics, options: WaterOptions, terrain: Option<Terrain>) -> Water {
        let device = gfx.shared.device();
        let params = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("yowie water"),
            size: std::mem::size_of::<WaterParamsGpu>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let no_heights = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("yowie water no heights"),
            size: 16,
            usage: wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        });
        Water {
            inner: Arc::new(WaterInner {
                options: Mutex::new(options),
                params,
                terrain,
                no_heights,
                group: Mutex::new(None),
            }),
        }
    }

    /// The current options.
    pub fn options(&self) -> WaterOptions {
        *self.inner.options.lock()
    }

    /// Changes the options; the next frame draws with them.
    pub fn set_options(&self, options: WaterOptions) {
        *self.inner.options.lock() = options;
    }

    fn params(&self, time: f32) -> WaterParamsGpu {
        let o = self.options();
        let half = o.size * 0.5;
        let lin = |c: Color, w: f32| {
            let l = c.to_linear();
            [l[0], l[1], l[2], w]
        };
        let mut p = WaterParamsGpu {
            area: [
                o.center.x - half.x,
                o.center.z - half.y,
                o.center.x + half.x,
                o.center.z + half.y,
            ],
            level: [o.center.y, time, o.depth_range.max(1e-3), o.floor_depth],
            shallow: lin(o.shallow_color, o.opacity.clamp(0.0, 1.0)),
            deep: lin(o.deep_color, o.roughness.clamp(0.02, 1.0)),
            waves: [
                o.waves.max(0.0),
                1.0 / o.wave_length.max(1e-3),
                o.wave_speed,
                o.foam.max(0.0),
            ],
            ..WaterParamsGpu::default()
        };
        if let Some(t) = &self.inner.terrain {
            let t = &t.inner;
            p.grid = [t.cols, t.rows, 1, 0];
            p.origin = [t.origin.x, t.origin.y, t.origin.z, t.cell];
        }
        p
    }
}

impl Scene<'_> {
    /// Draws a water surface, blended over the scene after the opaque and
    /// blended meshes.
    pub fn draw_water(&mut self, water: &Water) {
        self.rec.large.waters.push(WaterDraw {
            water: water.clone(),
        });
    }
}

/// Writes each recorded water's parameters and makes its bind group.
pub(crate) fn prepare(ctx: &PrepareContext<'_>, rec: &LargeRecord) {
    for d in &rec.waters {
        let w = &d.water.inner;
        let p = d.water.params(rec.time);
        ctx.shared
            .queue()
            .write_buffer(&w.params, 0, bytemuck::bytes_of(&p));
        let mut group = w.group.lock();
        if group.is_none() {
            let heights = w
                .terrain
                .as_ref()
                .map_or(&w.no_heights, |t| &t.inner.gpu.heights);
            *group = Some(
                ctx.shared
                    .device()
                    .create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("yowie water"),
                        layout: &ctx.pipelines.water_layout,
                        entries: &[
                            wgpu::BindGroupEntry {
                                binding: 32,
                                resource: w.params.as_entire_binding(),
                            },
                            wgpu::BindGroupEntry {
                                binding: 33,
                                resource: heights.as_entire_binding(),
                            },
                        ],
                    }),
            );
        }
    }
}

/// Records the recorded water surfaces.
pub(crate) fn encode(
    p: &LargePipelines,
    rec: &LargeRecord,
    pass: &mut wgpu::RenderPass<'_>,
) -> u32 {
    let mut calls = 0;
    for d in &rec.waters {
        let Some(group) = d.water.inner.group.lock().clone() else {
            continue;
        };
        if calls == 0 {
            pass.set_pipeline(&p.water);
        }
        pass.set_bind_group(1, &group, &[]);
        pass.draw(0..6, 0..1);
        calls += 1;
    }
    calls
}
