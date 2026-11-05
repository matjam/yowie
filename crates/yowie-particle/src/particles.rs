//! One handle for an effect that runs on either the CPU or the GPU.

use yowie_core::Vec2;
use yowie_gfx::scene::Scene;
use yowie_gfx::{Graphics, SpriteSheet, Texture};

use crate::emitter::Emitter;
use crate::gpu::GpuSystem;
use crate::system::System;

/// Where a [`Particles`] effect simulates and draws.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Backend {
    /// The CPU [`System`]: exact live counts, per-particle access, a few
    /// thousand particles per effect.
    #[default]
    Cpu,
    /// The compute-shader [`GpuSystem`]: hundreds of thousands of
    /// particles, live counts a few frames late.
    Gpu,
}

#[derive(Debug)]
enum Kind {
    Cpu(Box<System>),
    Gpu(Box<GpuSystem>),
}

/// An effect that a game can move between the CPU and GPU without
/// changing the code that drives it.
///
/// The methods match [`System`] and [`GpuSystem`]. Asking for
/// [`Backend::Gpu`] on a device that cannot run it falls back to the CPU,
/// and [`backend`](Self::backend) says which one is running.
///
/// ```no_run
/// # use yowie_core::vec2;
/// # use yowie_gfx::Graphics;
/// # use yowie_particle::{Backend, Particles, presets};
/// # fn f(gfx: &mut Graphics) {
/// let mut fire = Particles::at(gfx, presets::fire(), vec2(480.0, 400.0), Backend::Cpu);
/// fire.update(1.0 / 60.0);
/// fire.draw(gfx);
/// // Later, with a bigger rate:
/// fire.switch_to(gfx, Backend::Gpu);
/// # }
/// ```
#[derive(Debug)]
pub struct Particles {
    kind: Kind,
    texture: Option<Texture>,
    sheet: Option<SpriteSheet>,
}

impl Particles {
    /// Makes an effect for `emitter` at the origin and starts it.
    pub fn new(gfx: &Graphics, emitter: Emitter, backend: Backend) -> Self {
        Self::at(gfx, emitter, Vec2::ZERO, backend)
    }

    /// Makes an effect for `emitter` at `position` and starts it.
    pub fn at(gfx: &Graphics, emitter: Emitter, position: Vec2, backend: Backend) -> Self {
        Self {
            kind: Self::build(gfx, emitter, position, backend),
            texture: None,
            sheet: None,
        }
    }

    fn build(gfx: &Graphics, emitter: Emitter, position: Vec2, backend: Backend) -> Kind {
        if backend == Backend::Gpu {
            match GpuSystem::at(gfx, emitter.clone(), position) {
                Ok(s) => return Kind::Gpu(Box::new(s)),
                Err(e) => log::warn!("particles fall back to the CPU: {e}"),
            }
        }
        Kind::Cpu(Box::new(System::at(emitter, position)))
    }

    /// The backend that is running.
    pub fn backend(&self) -> Backend {
        match self.kind {
            Kind::Cpu(_) => Backend::Cpu,
            Kind::Gpu(_) => Backend::Gpu,
        }
    }

    /// Moves the effect to `backend`, keeping its emitter, position and
    /// texture. Live particles are dropped and the emitter's start burst
    /// and prewarm do not run again; emission continues from time zero.
    /// Does nothing when `backend` is already running.
    pub fn switch_to(&mut self, gfx: &Graphics, backend: Backend) {
        if self.backend() == backend {
            return;
        }
        let emitter = self.emitter().clone();
        let quiet = Emitter {
            burst: 0,
            prewarm: 0.0,
            ..emitter.clone()
        };
        self.kind = Self::build(gfx, quiet, self.position(), backend);
        self.set_emitter(emitter);
        match (self.texture.clone(), self.sheet.clone()) {
            (Some(t), _) => self.set_texture(t),
            (None, Some(s)) => self.set_sheet(s),
            _ => {}
        }
    }

    /// Draws particles with `texture`; without one they are solid squares.
    pub fn set_texture(&mut self, texture: Texture) {
        self.sheet = None;
        self.texture = Some(texture.clone());
        match &mut self.kind {
            Kind::Cpu(s) => s.set_texture(texture),
            Kind::Gpu(s) => s.set_texture(texture),
        }
    }

    /// Draws particles with the frames of `sheet`.
    pub fn set_sheet(&mut self, sheet: SpriteSheet) {
        self.texture = None;
        self.sheet = Some(sheet.clone());
        match &mut self.kind {
            Kind::Cpu(s) => s.set_sheet(sheet),
            Kind::Gpu(s) => s.set_sheet(sheet),
        }
    }

    /// The emitter the effect runs.
    pub fn emitter(&self) -> &Emitter {
        match &self.kind {
            Kind::Cpu(s) => s.emitter(),
            Kind::Gpu(s) => s.emitter(),
        }
    }

    /// Replaces the emitter without disturbing live particles, except that
    /// a GPU effect whose `max` changes drops them.
    pub fn set_emitter(&mut self, emitter: Emitter) {
        match &mut self.kind {
            Kind::Cpu(s) => s.set_emitter(emitter),
            Kind::Gpu(s) => s.set_emitter(emitter),
        }
    }

    /// Moves the effect.
    pub fn set_position(&mut self, position: Vec2) {
        match &mut self.kind {
            Kind::Cpu(s) => s.set_position(position),
            Kind::Gpu(s) => s.set_position(position),
        }
    }

    /// Where the effect is.
    pub fn position(&self) -> Vec2 {
        match &self.kind {
            Kind::Cpu(s) => s.position(),
            Kind::Gpu(s) => s.position(),
        }
    }

    /// Begins emitting from time zero.
    pub fn start(&mut self) {
        match &mut self.kind {
            Kind::Cpu(s) => s.start(),
            Kind::Gpu(s) => s.start(),
        }
    }

    /// Ends emission over time; live particles die out.
    pub fn stop(&mut self) {
        match &mut self.kind {
            Kind::Cpu(s) => s.stop(),
            Kind::Gpu(s) => s.stop(),
        }
    }

    /// Kills every live particle.
    pub fn clear(&mut self) {
        match &mut self.kind {
            Kind::Cpu(s) => s.clear(),
            Kind::Gpu(s) => s.clear(),
        }
    }

    /// Births `n` particles now.
    pub fn burst(&mut self, n: usize) {
        match &mut self.kind {
            Kind::Cpu(s) => s.burst(n),
            Kind::Gpu(s) => s.burst(n),
        }
    }

    /// The live particle count; a few frames old on the GPU.
    pub fn alive(&self) -> usize {
        match &self.kind {
            Kind::Cpu(s) => s.alive(),
            Kind::Gpu(s) => s.alive(),
        }
    }

    /// The exact live count. On the GPU backend this waits for the GPU, so
    /// use it for headless checks and tools, not every frame of a game.
    pub fn read_alive(&self) -> usize {
        match &self.kind {
            Kind::Cpu(s) => s.alive(),
            Kind::Gpu(s) => s.read_alive(),
        }
    }

    /// Whether the effect still births particles over time.
    pub fn emitting(&self) -> bool {
        match &self.kind {
            Kind::Cpu(s) => s.emitting(),
            Kind::Gpu(s) => s.emitting(),
        }
    }

    /// Whether the effect has stopped emitting and every particle died.
    pub fn finished(&self) -> bool {
        match &self.kind {
            Kind::Cpu(s) => s.finished(),
            Kind::Gpu(s) => s.finished(),
        }
    }

    /// Advances the effect by `dt` seconds.
    pub fn update(&mut self, dt: f32) {
        match &mut self.kind {
            Kind::Cpu(s) => s.update(dt),
            Kind::Gpu(s) => s.update(dt),
        }
    }

    /// Queues the effect into the 2D frame.
    pub fn draw(&self, gfx: &mut Graphics) {
        match &self.kind {
            Kind::Cpu(s) => s.draw(gfx),
            Kind::Gpu(s) => s.draw(gfx),
        }
    }

    /// Queues the effect into a 3D scene on the plane set with
    /// [`GpuSystem::set_plane`]. Only the GPU backend draws in 3D; the CPU
    /// backend draws nothing.
    pub fn draw_3d(&self, scene: &mut Scene<'_>) {
        if let Kind::Gpu(s) = &self.kind {
            s.draw_3d(scene);
        }
    }

    /// The GPU system when the GPU backend is running, for settings only it
    /// has such as the 3D plane.
    pub fn gpu_mut(&mut self) -> Option<&mut GpuSystem> {
        match &mut self.kind {
            Kind::Gpu(s) => Some(s),
            Kind::Cpu(_) => None,
        }
    }
}
