//! Indirect light from the scene itself: reflection probes and the light
//! probe grid.
//!
//! Both capture the scene into small cube maps through the ordinary scene
//! passes (`capture.rs`): six 90 degree views from a point into a capture
//! slot each, with draws hidden in probes left out.
//!
//! - A [`ReflectionProbe`] keeps one capture, prefiltered like the image
//!   environment (`ibl.rs`): one mip per roughness step. Scenes copy the
//!   probes they show into a cube array (`state.rs`); the surface program
//!   blends the probes covering a fragment, smaller boxes first, looks each
//!   up with box projection, and gives what they leave to the environment.
//! - A [`LightProbeGrid`] captures a small cube at each cell and projects
//!   it on the processor to nine spherical harmonic coefficients (`sh.rs`),
//!   convolved with the cosine lobe, so evaluating them gives irradiance
//!   over pi. They upload to a 3D texture, seven slabs deep, which the
//!   surface program filters trilinearly for indirect diffuse light. The
//!   coefficients save to bytes and load back (`file.rs`).
//!
//! Captures render linear HDR light with the scene's lights, shadows, sky
//! or environment, and the light probe grid when baking a reflection probe
//! (never other probes, so a capture never feeds on itself).

mod bake;
mod capture;
mod file;
mod grid;
mod reflection;
mod sh;
mod state;

#[cfg(test)]
mod gpu_tests;

use bytemuck::{Pod, Zeroable};

pub use grid::{LightProbeGrid, MAX_LIGHT_PROBES};
pub use reflection::{ProbeUpdate, ReflectionProbe};

pub(crate) use grid::GridUse;
pub(crate) use reflection::ProbeUse;
pub(crate) use state::ProbeState;

use super::prepare::View;
use super::record::SceneRecord;
use crate::shared::Shared;

/// The most reflection probes one scene shows. Further probes are ignored
/// (the first added are kept).
pub const MAX_REFLECTION_PROBES: usize = 8;

/// What a capture view renders for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Capture {
    /// A reflection probe face: lit by the grid, not by other probes.
    Probe,
    /// A light probe grid cell face: lit by neither.
    Grid,
}

/// One reflection probe as the surface program reads it; matches
/// `ProbeGpu` in `probes.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub(crate) struct ProbeGpu {
    /// Capture position; w is the intensity.
    pub(crate) center: [f32; 4],
    /// Influence box minimum; w is the blend distance.
    pub(crate) box_min: [f32; 4],
    /// Influence box maximum.
    pub(crate) box_max: [f32; 4],
    /// Cube array layer, 1 when box projected, 0, 0.
    pub(crate) params: [f32; 4],
}

/// The probe and grid block at group 0, binding 20; matches `Probes` in
/// `probes.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub(crate) struct ProbeBlock {
    /// Probe count, specular mip count, 0, 0.
    pub(crate) info: [f32; 4],
    /// Cell (0, 0, 0)'s position; w is the intensity, 0 for no grid.
    pub(crate) grid_origin: [f32; 4],
    /// Distance between cells.
    pub(crate) grid_spacing: [f32; 4],
    /// Cells along each axis.
    pub(crate) grid_counts: [f32; 4],
    pub(crate) list: [ProbeGpu; MAX_REFLECTION_PROBES],
}

/// Whether reflection probes bind as a cube array, which allows several
/// per scene. Adapters without cube arrays (some mobile Vulkan drivers)
/// bind one cube and show one probe per scene.
pub(crate) fn cube_arrays(shared: &Shared) -> bool {
    shared
        .gpu
        .adapter
        .get_downlevel_capabilities()
        .flags
        .contains(wgpu::DownlevelFlags::CUBE_ARRAY_TEXTURES)
}

/// The probe declarations and lookups the mesh module includes.
pub(crate) fn shader_source(cube_array: bool) -> String {
    let sample = if cube_array {
        include_str!("probe_array.wgsl")
    } else {
        include_str!("probe_single.wgsl")
    };
    [sample, include_str!("probes.wgsl")].join("\n")
}

/// For a capture view, which of `rec`'s draws to skip (by draw index);
/// empty for any other view.
pub(crate) fn hidden_mask(rec: &SceneRecord, v: &View) -> Vec<bool> {
    if v.capture.is_none() || rec.probe_hidden.is_empty() {
        return Vec::new();
    }
    let mut mask = vec![false; rec.draws.len()];
    for &i in &rec.probe_hidden {
        if let Some(m) = mask.get_mut(i as usize) {
            *m = true;
        }
    }
    mask
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_matches_the_shader_layout() {
        assert_eq!(std::mem::size_of::<ProbeGpu>(), 64);
        assert_eq!(
            std::mem::size_of::<ProbeBlock>(),
            64 + 64 * MAX_REFLECTION_PROBES
        );
    }
}
