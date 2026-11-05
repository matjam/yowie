//! Particles simulated and drawn on the GPU. [`GpuSystem`] documents how.

mod draw;
mod layout;
mod pool;
mod programs;
#[cfg(test)]
mod tests;

use std::sync::{Arc, Mutex};

use yowie_core::{Vec2, Vec3};
use yowie_gfx::scene::Scene;
use yowie_gfx::{GpuDraw, Graphics, SpriteSheet, Texture};
use yowie_rng::Rng;

use crate::emitter::Emitter;
use crate::error::{ParticleError, Result};
use crate::schedule::Schedule;
use crate::spawn::{Particle, birth};
use draw::Inner;
use layout::{GpuParticle, StepParams};
use pool::{Core, MAX_CAPACITY};
use programs::Programs;

/// Checks that `gfx`'s device can run GPU particles and returns whether
/// it can also draw with GPU-written arguments.
fn check(gfx: &Graphics) -> Result<bool> {
    let caps = gfx.downlevel_capabilities();
    let flags = caps.flags;
    let missing = |what: &str| Err(ParticleError::Unsupported(what.to_owned()));
    if !flags.contains(wgpu::DownlevelFlags::COMPUTE_SHADERS) {
        return missing("the adapter has no compute shaders");
    }
    if !flags.contains(wgpu::DownlevelFlags::VERTEX_STORAGE) {
        return missing("the adapter has no storage buffers in vertex shaders");
    }
    let l = gfx.device().limits();
    if l.max_storage_buffers_per_shader_stage < 4 {
        return missing("the adapter allows fewer than four storage buffers per stage");
    }
    if l.max_compute_workgroup_size_x < 64 || l.max_compute_invocations_per_workgroup < 64 {
        return missing("the adapter's compute workgroups hold fewer than 64 invocations");
    }
    Ok(flags.contains(wgpu::DownlevelFlags::INDIRECT_EXECUTION))
}

/// Whether [`GpuSystem::new`] can succeed on `gfx`'s device.
pub fn gpu_supported(gfx: &Graphics) -> bool {
    check(gfx).is_ok()
}

/// One emitter simulated and drawn on the GPU.
///
/// A `GpuSystem` runs the same [`Emitter`] as the CPU
/// [`System`](crate::System) with the same birth code and the same API
/// shape, but keeps its particles in GPU buffers:
///
/// - Births are made on the CPU with the shared birth code (so shapes,
///   palettes and ranges behave identically) and uploaded once a frame.
/// - A compute pass moves them into free slots taken from a dead stack,
///   then ages, accelerates and kills every slot: gravity, drag, radial
///   and tangential acceleration, up to eight attractors, speed over life.
/// - Size, colour and alpha over life are sampled from 128-entry tables
///   built from the emitter's curves.
/// - One instanced draw per emitter, with the instance count written by
///   the GPU (an indirect draw), expands each live particle into a quad in
///   the vertex shader.
///
/// The simulation advances when the system is drawn: [`GpuSystem::update`]
/// records the step and the frame's compute pass runs it, so a system that
/// is never drawn does not simulate. Live counts come back from the GPU
/// asynchronously, so [`alive`](GpuSystem::alive) lags by a few frames.
///
/// A device that lacks compute shaders, vertex-stage storage buffers or
/// four storage buffers per stage cannot run this path;
/// [`GpuSystem::new`] reports why. [`Particles`](crate::Particles) falls
/// back to the CPU system in that case.
///
/// ```no_run
/// # use yowie_core::vec2;
/// # use yowie_gfx::Graphics;
/// # use yowie_particle::{GpuSystem, presets};
/// # fn f(gfx: &mut Graphics) -> Result<(), yowie_particle::ParticleError> {
/// let mut fire = GpuSystem::at(gfx, presets::fire(), vec2(480.0, 400.0))?;
/// fire.update(1.0 / 60.0);
/// fire.draw(gfx);
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
pub struct GpuSystem {
    inner: Arc<Inner>,
    emitter: Emitter,
    position: Vec2,
    rng: Rng,
    schedule: Schedule,
    texture: Option<Texture>,
    sheet: Option<SpriteSheet>,
}

impl GpuSystem {
    /// Makes a system for `emitter` at the origin and starts it.
    pub fn new(gfx: &Graphics, emitter: Emitter) -> Result<Self> {
        Self::at(gfx, emitter, Vec2::ZERO)
    }

    /// Makes a system for `emitter` at `position` and starts it: the
    /// emitter's `burst` is born there and its `prewarm` runs.
    pub fn at(gfx: &Graphics, emitter: Emitter, position: Vec2) -> Result<Self> {
        let indirect = check(gfx)?;
        let device = gfx.device();
        let limit = device.limits().max_storage_buffer_binding_size / 48;
        let capacity = (emitter.max as u64).clamp(1, limit.min(u64::from(MAX_CAPACITY))) as u32;
        let core = Core::new(device, gfx.queue(), Programs::get(gfx), capacity);
        let mut s = Self {
            inner: Arc::new(Inner {
                core: Mutex::new(core),
                indirect,
            }),
            rng: Rng::new(emitter.seed),
            emitter,
            position,
            schedule: Schedule::default(),
            texture: None,
            sheet: None,
        };
        s.refresh();
        s.start();
        Ok(s)
    }

    /// Draws particles with `texture`; without one they are solid squares.
    pub fn set_texture(&mut self, texture: Texture) {
        let smooth = texture.options().filter == yowie_gfx::Filter::Linear;
        self.inner
            .lock()
            .set_view(Some(texture.wgpu_view()), Some(smooth));
        self.texture = Some(texture);
        self.sheet = None;
        self.refresh();
    }

    /// Draws particles with the frames of `sheet`, chosen over each
    /// particle's life by the emitter's `frames` and `frame_over_life`.
    /// At most 64 frames are addressable.
    pub fn set_sheet(&mut self, sheet: SpriteSheet) {
        let tex = sheet.texture();
        let smooth = tex.options().filter == yowie_gfx::Filter::Linear;
        self.inner
            .lock()
            .set_view(Some(tex.wgpu_view()), Some(smooth));
        self.sheet = Some(sheet);
        self.texture = None;
        self.refresh();
    }

    /// The emitter the system runs.
    pub fn emitter(&self) -> &Emitter {
        &self.emitter
    }

    /// Replaces the emitter without disturbing live particles. Changing
    /// `max` resizes the particle pool and drops the live particles.
    pub fn set_emitter(&mut self, emitter: Emitter) {
        let capacity = (emitter.max as u64).clamp(1, u64::from(MAX_CAPACITY)) as u32;
        {
            let mut core = self.inner.lock();
            if capacity != core.capacity && emitter.max != self.emitter.max {
                core.resize(capacity);
            }
        }
        self.emitter = emitter;
        self.refresh();
    }

    /// Moves the system. In a world-space system live particles stay where
    /// they are; otherwise they move with it.
    pub fn set_position(&mut self, position: Vec2) {
        if position != self.position {
            self.position = position;
            self.refresh();
        }
    }

    /// Where the system is.
    pub fn position(&self) -> Vec2 {
        self.position
    }

    /// Places the plane the particles live on in a 3D world: particle
    /// position `(x, y)` lands at `origin + x * x_axis + y * y_axis`.
    /// Used by [`draw_3d`](Self::draw_3d); the default is the XY plane at
    /// the world origin.
    pub fn set_plane(&mut self, origin: Vec3, x_axis: Vec3, y_axis: Vec3) {
        let mut core = self.inner.lock();
        core.block.plane_origin = origin.extend(0.0).to_array();
        core.block.plane_x = x_axis.extend(0.0).to_array();
        core.block.plane_y = y_axis.extend(0.0).to_array();
        core.touch_block();
    }

    /// Begins emitting from time zero: births the emitter's `burst` and
    /// runs its `prewarm`.
    pub fn start(&mut self) {
        self.schedule.start(&self.emitter);
        self.burst(self.emitter.burst as usize);
        let step = 1.0 / 60.0;
        let mut t = 0.0;
        while t < self.emitter.prewarm {
            self.update(step);
            t += step;
        }
    }

    /// Ends emission over time. Live particles die out on their own.
    pub fn stop(&mut self) {
        self.schedule.stop();
    }

    /// Kills every live particle.
    pub fn clear(&mut self) {
        self.inner.lock().clear();
    }

    /// Births `n` particles now, whether or not the system is emitting, up
    /// to the pool's capacity.
    pub fn burst(&mut self, n: usize) {
        if n == 0 {
            return;
        }
        let inner = self.inner.clone();
        let mut core = inner.lock();
        let births = self.make_births(n.min(core.capacity as usize));
        let step = StepParams::new(&self.emitter, 0.0, core.capacity);
        core.queue_step(step, &births);
    }

    /// The live particle count as of a few frames ago.
    pub fn alive(&self) -> usize {
        self.inner.lock().alive_seen as usize
    }

    /// The most live particles the pool holds.
    pub fn capacity(&self) -> usize {
        self.inner.lock().capacity as usize
    }

    /// Whether the system still births particles over time.
    pub fn emitting(&self) -> bool {
        self.schedule.emitting(&self.emitter)
    }

    /// Whether the system has stopped emitting and every particle has
    /// died, so a one-shot effect can be dropped. It becomes true a few
    /// frames after the last particle dies.
    pub fn finished(&self) -> bool {
        !self.emitting() && self.inner.lock().settled()
    }

    /// Records a simulation step of `dt` seconds; the frame's compute pass
    /// runs it when the system is drawn. A step of zero or less does
    /// nothing.
    pub fn update(&mut self, dt: f32) {
        if dt <= 0.0 {
            return;
        }
        let due = self.schedule.advance(&self.emitter, dt);
        let inner = self.inner.clone();
        let mut core = inner.lock();
        if due == 0 && core.settled() {
            return;
        }
        let births = self.make_births(due.min(core.capacity as usize));
        let step = StepParams::new(&self.emitter, dt, core.capacity);
        core.queue_step(step, &births);
    }

    /// Queues the system into the 2D pass at the current layer (the
    /// emitter's `layer` applies) and sort key.
    pub fn draw(&self, gfx: &mut Graphics) {
        let draw: Arc<dyn GpuDraw> = self.inner.clone();
        gfx.with_layer(self.emitter.layer, |g| g.draw_gpu(draw));
    }

    /// Queues the system into a 3D scene's HDR pass as camera-facing
    /// quads on the plane set with [`set_plane`](Self::set_plane), after
    /// the blended meshes and tested against the scene's depth.
    pub fn draw_3d(&self, scene: &mut Scene<'_>) {
        let draw: Arc<dyn GpuDraw> = self.inner.clone();
        scene.draw_gpu(draw);
    }

    /// The live count read back now, waiting for the GPU. For tests and
    /// tools; it stalls the frame.
    pub fn read_alive(&self) -> usize {
        let core = self.inner.lock();
        let bytes = core.read_blocking(&core.header, 4, 4);
        bytes
            .first_chunk::<4>()
            .map_or(0, |b| u32::from_le_bytes(*b) as usize)
    }

    /// Every live particle read back now, waiting for the GPU, in slot
    /// order. For tests and tools; it stalls the frame.
    pub fn snapshot(&self) -> Vec<Particle> {
        let core = self.inner.lock();
        let bytes = core.read_blocking(&core.particles, 0, u64::from(core.capacity) * 48);
        bytes
            .as_chunks::<48>()
            .0
            .iter()
            .map(|c| bytemuck::pod_read_unaligned::<GpuParticle>(c))
            .filter(|p| p.life > 0.0)
            .map(GpuParticle::to_particle)
            .collect()
    }

    fn make_births(&mut self, n: usize) -> Vec<GpuParticle> {
        let origin = if self.emitter.world_space {
            self.position
        } else {
            Vec2::ZERO
        };
        let width = self.default_width();
        (0..n)
            .map(|_| GpuParticle::from(&birth(&self.emitter, &mut self.rng, origin, width)))
            .collect()
    }

    /// The width a particle takes when the emitter's size is 0.
    fn default_width(&self) -> f32 {
        let w = match (&self.sheet, &self.texture) {
            (Some(s), _) => s.frame_size().x as f32,
            (None, Some(t)) => t.width() as f32,
            _ => 0.0,
        };
        if w > 0.0 { w } else { 8.0 }
    }

    /// Height over width for a particle.
    fn aspect(&self) -> f32 {
        if self.emitter.aspect > 0.0 {
            return self.emitter.aspect;
        }
        match (&self.sheet, &self.texture) {
            (Some(s), _) => s.frame_size().y as f32 / (s.frame_size().x as f32).max(1.0),
            (None, Some(t)) => t.height() as f32 / (t.width() as f32).max(1.0),
            _ => 1.0,
        }
    }

    /// Rebuilds the tables and draw settings from the emitter.
    fn refresh(&mut self) {
        let blend: yowie_gfx::BlendMode = self.emitter.blend.into();
        let aspect = self.aspect();
        let mut core = self.inner.lock();
        core.blend = blend;
        let b = &mut core.block;
        b.fill_curves(&self.emitter);
        b.fill_frames(&self.emitter, self.sheet.as_ref());
        b.aspect = aspect;
        b.premult = u32::from(blend.premultiplies_tint());
        b.offset = if self.emitter.world_space {
            [0.0; 2]
        } else {
            self.position.to_array()
        };
        core.touch_block();
    }
}
