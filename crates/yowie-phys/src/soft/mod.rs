//! Cloth, volumetric soft bodies and 2D fluids, simulated with extended
//! position-based dynamics (XPBD).
//!
//! The solver is Yowie's own code. Each `step` splits the time into
//! substeps; a substep predicts positions from velocities, projects the
//! constraints several times, pushes particles out of colliders and reads
//! the new velocities back from how far each particle moved. Compliance is
//! in metres per newton: zero is rigid and larger values are softer.
//!
//! Soft objects collide with the static and kinematic colliders of a
//! [`World3D`](crate::World3D) (cloth and soft bodies) or
//! [`World2D`](crate::World2D) (fluids), found through rapier's query
//! pipeline, so call the world's `step` or `refresh_queries` first. They do
//! not push rigid bodies back, and sensors are ignored. Pass `&()` where a
//! world is expected to simulate without colliders.
//!
//! Enable the `rayon` feature to split large per-particle passes across
//! threads. Results are identical with and without it.
//!
//! ```
//! use yowie_core::Vec3;
//! use yowie_phys::soft::{Cloth, ClothDesc, SoftSettings};
//!
//! let desc = ClothDesc { cols: 8, rows: 8, pinned: vec![0, 7], ..ClothDesc::default() };
//! let mut cloth = Cloth::new(&desc).unwrap();
//! let settings = SoftSettings::default();
//! for _ in 0..60 {
//!     cloth.step(1.0 / 60.0, &settings, &());
//! }
//! assert_eq!(cloth.positions()[0], Vec3::ZERO);
//! ```

mod body;
mod cloth;
mod collide;
mod constraints;
mod fluid;
mod fluid_grid;
mod mesh;
mod parallel;
mod solver;

#[cfg(test)]
mod tests;

pub use body::{SoftBody, SoftBodyDesc};
pub use cloth::{Cloth, ClothDesc};
pub use collide::{Colliders2, Colliders3, Contact2, Contact3, Ground, QUERY_REACH};
pub use constraints::{
    Link, Pressure, ShapeMatch, Tet, enclosed_volume, project_link, project_pressure, project_tet,
    tet_volume,
};
pub use fluid::{Fluid2D, FluidDesc2D};
pub use mesh::{SurfaceMesh, compute_normals};
pub use solver::{SoftSettings, Wind};
