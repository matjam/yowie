//! The reflection probe a game places and the capture it keeps.

use std::sync::Arc;

use parking_lot::Mutex;
use yowie_core::Vec3;

use crate::scene::bounds::Aabb;
use crate::texture::next_id;

/// When a [`ReflectionProbe`] captures the scene again.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ProbeUpdate {
    /// Captured once: by
    /// [`Graphics::bake_reflection_probe`](crate::Graphics::bake_reflection_probe),
    /// or from the first scene that shows the probe unbaked; and again
    /// from the next scene that shows it after
    /// [`ReflectionProbe::request_bake`].
    #[default]
    OnRequest,
    /// Captured again from the scene that shows it every `n` frames (1 is
    /// every frame), for rooms whose contents move. Each capture renders
    /// the scene six times.
    EveryFrames(u32),
}

/// A point that captures the scene around it into a cube map, so shiny
/// surfaces inside its box reflect the room rather than the sky.
///
/// Show it with [`Scene::add_reflection_probe`](crate::scene::Scene::add_reflection_probe).
/// The capture is prefiltered like an [`Environment`](super::super::Environment)
/// (rougher surfaces see blurrier mips) and lights specular reflections
/// only; diffuse light comes from a [`LightProbeGrid`](super::LightProbeGrid)
/// or the environment. Where probes overlap, smaller boxes take
/// precedence and blend over larger ones across their blend distance;
/// what no probe covers falls back to the environment, sky or ambient
/// colour. Cloning shares the capture.
///
/// ```no_run
/// # use yowie_gfx::{Graphics, scene::*};
/// # use yowie_core::{Color, Vec3};
/// # fn f(gfx: &mut Graphics, room: &Mesh, ball: &Mesh) {
/// let bounds = Aabb::new(Vec3::new(-9.0, 0.0, -9.0), Vec3::new(9.0, 5.0, 9.0));
/// let probe = ReflectionProbe::new(Vec3::new(0.0, 1.8, 0.0), bounds);
/// gfx.bake_reflection_probe(&probe, |s| {
///     s.draw(room, &Material::color(Color::RED, 0.0, 0.8), Vec3::ZERO);
/// });
/// # let cam = Camera3D::default();
/// gfx.scene(&cam, |s| {
///     s.add_reflection_probe(&probe);
///     s.draw(room, &Material::color(Color::RED, 0.0, 0.8), Vec3::ZERO);
///     s.draw(ball, &Material::color(Color::WHITE, 1.0, 0.05), Vec3::Y);
/// });
/// # }
/// ```
#[derive(Clone, Debug)]
pub struct ReflectionProbe {
    /// Where the cube is captured from, in world units.
    pub position: Vec3,
    /// The box the probe lights, and the box reflections are projected
    /// onto when `box_projection` is on: usually the room's walls.
    pub bounds: Aabb,
    /// How far outside its box the probe fades out, in world units.
    /// Surfaces inside the box and on its faces get the whole probe; 0
    /// ends it sharply at the faces.
    pub blend_distance: f32,
    /// Multiplies the probe's light.
    pub intensity: f32,
    /// Corrects reflections for parallax by projecting them onto
    /// `bounds`, so a wall reflects where it stands seen from anywhere in
    /// the box. Right for rooms whose walls match the box (on by
    /// default); turn it off for open spaces.
    pub box_projection: bool,
    /// The capture's face size in texels, rounded to a power of two; 128
    /// by default. The prefiltered result is 128 texels a side whatever
    /// this is.
    pub resolution: u32,
    /// The capture cameras' near distance in world units.
    pub near: f32,
    /// The capture cameras' far distance in world units.
    pub far: f32,
    /// When the probe captures again.
    pub update: ProbeUpdate,
    pub(crate) shared: Arc<ProbeShared>,
}

/// The capture a probe and its clones share.
#[derive(Debug)]
pub(crate) struct ProbeShared {
    pub(crate) state: Mutex<ProbeBaked>,
}

#[derive(Debug, Default)]
pub(crate) struct ProbeBaked {
    pub(crate) maps: Option<Arc<ProbeMaps>>,
    pub(crate) requested: bool,
    /// The renderer frame of the last capture taken from a scene.
    pub(crate) frame: Option<u64>,
}

/// One prefiltered capture: a cube of `PROBE_SIZE` texels with
/// `PROBE_MIPS` mips. `generation` names this capture, so a scene notices
/// a new one.
#[derive(Debug)]
pub(crate) struct ProbeMaps {
    pub(crate) texture: wgpu::Texture,
    pub(crate) generation: u64,
}

/// A probe a scene shows.
#[derive(Clone, Debug)]
pub(crate) struct ProbeUse {
    pub(crate) probe: ReflectionProbe,
}

impl ReflectionProbe {
    /// A probe capturing from `position` that lights `bounds`, with box
    /// projection on, a blend distance of 1 and 128 texel faces.
    pub fn new(position: Vec3, bounds: Aabb) -> Self {
        Self {
            position,
            bounds,
            blend_distance: 1.0,
            intensity: 1.0,
            box_projection: true,
            resolution: 128,
            near: 0.05,
            far: 1000.0,
            update: ProbeUpdate::OnRequest,
            shared: Arc::new(ProbeShared {
                state: Mutex::new(ProbeBaked::default()),
            }),
        }
    }

    /// The same probe capturing again as `update` says.
    pub fn with_update(self, update: ProbeUpdate) -> Self {
        Self { update, ..self }
    }

    /// Whether the probe holds a capture.
    pub fn is_baked(&self) -> bool {
        self.shared.state.lock().maps.is_some()
    }

    /// Asks the next scene that shows the probe to capture it again from
    /// that scene's draws.
    pub fn request_bake(&self) {
        self.shared.state.lock().requested = true;
    }

    /// The capture, if any.
    pub(crate) fn maps(&self) -> Option<Arc<ProbeMaps>> {
        self.shared.state.lock().maps.clone()
    }

    /// Whether a scene rendering in renderer frame `frame` should capture
    /// the probe first.
    pub(crate) fn due(&self, frame: u64) -> bool {
        let st = self.shared.state.lock();
        due_at(
            self.update,
            st.maps.is_some(),
            st.requested,
            st.frame,
            frame,
        )
    }

    /// Keeps a new capture taken in renderer frame `frame` (`None` for a
    /// bake outside the frame).
    pub(crate) fn store(&self, texture: wgpu::Texture, frame: Option<u64>) {
        let mut st = self.shared.state.lock();
        st.maps = Some(Arc::new(ProbeMaps {
            texture,
            generation: next_id(),
        }));
        st.requested = false;
        if frame.is_some() {
            st.frame = frame;
        }
    }

    /// The volume of the probe's box, which orders overlapping probes.
    pub(crate) fn volume(&self) -> f32 {
        let s = (self.bounds.max - self.bounds.min).max(Vec3::ZERO);
        s.x * s.y * s.z
    }
}

/// Whether a probe updating by `update` captures in frame `frame`, given
/// whether it holds a capture, whether one was requested, and the frame
/// of its last capture from a scene. A probe captures at most once a
/// frame.
fn due_at(
    update: ProbeUpdate,
    baked: bool,
    requested: bool,
    last: Option<u64>,
    frame: u64,
) -> bool {
    if last == Some(frame) {
        return false;
    }
    if !baked || requested {
        return true;
    }
    match update {
        ProbeUpdate::OnRequest => false,
        ProbeUpdate::EveryFrames(n) => {
            last.is_none_or(|f| frame.saturating_sub(f) >= u64::from(n.max(1)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn due_follows_the_update_mode() {
        use ProbeUpdate::{EveryFrames, OnRequest};
        // (update, baked, requested, last capture, frame, due)
        let cases = [
            (OnRequest, false, false, None, 1, true),
            (OnRequest, true, false, Some(1), 2, false),
            (OnRequest, true, false, None, 9, false),
            (OnRequest, true, true, Some(1), 2, true),
            (OnRequest, false, false, Some(4), 4, false),
            (EveryFrames(3), true, false, Some(1), 3, false),
            (EveryFrames(3), true, false, Some(1), 4, true),
            (EveryFrames(1), true, false, Some(4), 5, true),
            (EveryFrames(0), true, false, Some(4), 5, true),
            (EveryFrames(2), true, false, None, 1, true),
        ];
        for (update, baked, requested, last, frame, want) in cases {
            assert_eq!(
                due_at(update, baked, requested, last, frame),
                want,
                "{update:?} baked {baked} requested {requested} last {last:?} frame {frame}"
            );
        }
        let b = Aabb::new(Vec3::splat(-1.0), Vec3::splat(1.0));
        assert_eq!(ReflectionProbe::new(Vec3::ZERO, b).volume(), 8.0);
    }
}
