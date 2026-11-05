//! The structures shared with the WGSL of the GPU particle path, and the
//! tables built from an emitter's curves.

use bytemuck::{Pod, Zeroable};
use yowie_core::Color;
use yowie_gfx::SpriteSheet;

use crate::emitter::Emitter;
use crate::spawn::Particle;

/// Samples per curve table; the shaders interpolate between them.
pub(crate) const LUT_SAMPLES: usize = 128;
/// Most sprite sheet frames a GPU emitter can address.
pub(crate) const MAX_FRAMES: usize = 64;
/// Most attractors a GPU emitter applies; extra ones are ignored.
pub(crate) const MAX_ATTRACTORS: usize = 8;

/// One particle as stored on the GPU; matches `Particle` in the shaders.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub(crate) struct GpuParticle {
    pub(crate) pos: [f32; 2],
    pub(crate) vel: [f32; 2],
    pub(crate) origin: [f32; 2],
    pub(crate) age: f32,
    /// Seconds it lives; 0 marks a free slot.
    pub(crate) life: f32,
    pub(crate) size: f32,
    pub(crate) rot: f32,
    pub(crate) spin: f32,
    /// The palette tint as sRGB bytes, red in the low byte.
    pub(crate) tint: u32,
}

const _: () = assert!(std::mem::size_of::<GpuParticle>() == 48);
const _: () = assert!(std::mem::size_of::<StepParams>() <= STEP_STRIDE as usize);

impl From<&Particle> for GpuParticle {
    fn from(p: &Particle) -> Self {
        Self {
            pos: p.position.to_array(),
            vel: p.velocity.to_array(),
            origin: p.origin.to_array(),
            age: p.age,
            life: p.life,
            size: p.size,
            rot: p.rotation,
            spin: p.spin,
            tint: u32::from_le_bytes(p.tint.to_rgba8()),
        }
    }
}

impl GpuParticle {
    /// The particle as the CPU system describes it.
    pub(crate) fn to_particle(self) -> Particle {
        let [r, g, b, a] = self.tint.to_le_bytes();
        Particle {
            position: self.pos.into(),
            velocity: self.vel.into(),
            origin: self.origin.into(),
            age: self.age,
            life: self.life,
            size: self.size,
            rotation: self.rot,
            spin: self.spin,
            tint: Color::rgba8(r, g, b, a),
        }
    }
}

/// The parameters of one simulation step; matches `Params` in `sim.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub(crate) struct StepParams {
    pub(crate) dt: f32,
    pub(crate) birth_first: u32,
    pub(crate) birth_count: u32,
    pub(crate) capacity: u32,
    pub(crate) gravity: [f32; 2],
    pub(crate) drag: f32,
    pub(crate) radial: f32,
    pub(crate) tangential: f32,
    pub(crate) attractor_count: u32,
    pub(crate) pad: [u32; 2],
    pub(crate) attractors: [[f32; 4]; MAX_ATTRACTORS],
}

/// Bytes between steps in the parameter buffer, so each can be bound at a
/// dynamic offset on any device.
pub(crate) const STEP_STRIDE: u64 = 256;

impl StepParams {
    pub(crate) fn new(e: &Emitter, dt: f32, capacity: u32) -> Self {
        let mut attractors = [[0.0; 4]; MAX_ATTRACTORS];
        let n = e.attractors.len().min(MAX_ATTRACTORS);
        for (slot, a) in attractors.iter_mut().zip(&e.attractors) {
            *slot = [a.position.x, a.position.y, a.strength, a.radius];
        }
        Self {
            dt,
            birth_first: 0,
            birth_count: 0,
            capacity,
            gravity: e.gravity.to_array(),
            drag: e.drag,
            radial: e.radial_accel,
            tangential: e.tangential_accel,
            attractor_count: n as u32,
            pad: [0; 2],
            attractors,
        }
    }
}

/// The per-emitter tables and draw settings; matches `Lut` in the shaders.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub(crate) struct DrawBlock {
    pub(crate) offset: [f32; 2],
    pub(crate) aspect: f32,
    pub(crate) frame_count: u32,
    pub(crate) capacity: u32,
    pub(crate) premult: u32,
    pub(crate) pad: [u32; 2],
    pub(crate) plane_origin: [f32; 4],
    pub(crate) plane_x: [f32; 4],
    pub(crate) plane_y: [f32; 4],
    pub(crate) color: [[f32; 4]; LUT_SAMPLES],
    pub(crate) curve: [[f32; 4]; LUT_SAMPLES],
    pub(crate) frames: [[f32; 4]; MAX_FRAMES],
}

impl DrawBlock {
    pub(crate) fn new(capacity: u32) -> Self {
        let mut b = Self::zeroed();
        b.capacity = capacity;
        b.aspect = 1.0;
        b.plane_x = [1.0, 0.0, 0.0, 0.0];
        b.plane_y = [0.0, 1.0, 0.0, 0.0];
        b
    }

    /// Samples the emitter's colour gradient and curves.
    pub(crate) fn fill_curves(&mut self, e: &Emitter) {
        let last = (LUT_SAMPLES - 1) as f32;
        for i in 0..LUT_SAMPLES {
            let t = i as f32 / last;
            self.color[i] = e.color_at(t).to_array();
            let frame = if e.frame_over_life.is_empty() {
                t
            } else {
                e.frame_over_life.at(t)
            };
            self.curve[i] = [
                e.size_over_life.at(t),
                e.speed_over_life.at(t),
                e.alpha_over_life.at(t),
                frame,
            ];
        }
    }

    /// Fills the frame rectangles for a sheet, in the order the emitter's
    /// `frames` list (or the sheet's own order) names them.
    pub(crate) fn fill_frames(&mut self, e: &Emitter, sheet: Option<&SpriteSheet>) {
        self.frame_count = 0;
        let Some(sheet) = sheet else {
            return;
        };
        let n = if e.frames.is_empty() {
            sheet.count() as usize
        } else {
            e.frames.len()
        }
        .min(MAX_FRAMES);
        for slot in 0..n {
            let frame = e.frames.get(slot).copied().unwrap_or(slot as u32);
            let (a, b) = sheet
                .frame_uv(frame)
                .unwrap_or((yowie_core::Vec2::ZERO, yowie_core::Vec2::ONE));
            self.frames[slot] = [a.x, a.y, b.x, b.y];
        }
        self.frame_count = n as u32;
    }
}
