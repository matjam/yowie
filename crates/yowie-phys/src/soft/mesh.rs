//! Triangle meshes that follow soft particles, for rendering.

use yowie_core::Vec3;

/// A triangle mesh whose vertices are a soft body's particles.
///
/// Keep one next to a [`Cloth`](super::Cloth) or [`SoftBody`](super::SoftBody)
/// and call `update_mesh` after each step. Upload `positions` and `normals`
/// to the renderer; `indices` never change.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SurfaceMesh {
    /// Vertex positions in world space, one per particle.
    pub positions: Vec<Vec3>,
    /// Smooth vertex normals, unit length, one per particle.
    pub normals: Vec<Vec3>,
    /// Triangles as counter-clockwise index triples into `positions`.
    pub indices: Vec<u32>,
}

impl SurfaceMesh {
    /// Creates a mesh with the given triangle indices and `vertex_count`
    /// vertices at the origin. Call [`update`](Self::update) to fill it.
    pub fn new(indices: Vec<u32>, vertex_count: usize) -> Self {
        Self {
            positions: vec![Vec3::ZERO; vertex_count],
            normals: vec![Vec3::Y; vertex_count],
            indices,
        }
    }

    /// Copies `particles` into `positions` and recomputes `normals`. Does
    /// nothing if the particle count differs from the vertex count.
    pub fn update(&mut self, particles: &[Vec3]) {
        if particles.len() != self.positions.len() {
            return;
        }
        self.positions.copy_from_slice(particles);
        compute_normals(&self.positions, &self.indices, &mut self.normals);
    }
}

/// Computes area-weighted smooth vertex normals for a triangle list.
///
/// `out` is resized to `positions.len()`. Vertices used by no triangle, or
/// whose triangles cancel out, get `Vec3::Y`. Indices out of range are
/// skipped.
pub fn compute_normals(positions: &[Vec3], indices: &[u32], out: &mut Vec<Vec3>) {
    out.clear();
    out.resize(positions.len(), Vec3::ZERO);
    for tri in indices.as_chunks::<3>().0 {
        let [a, b, c] = [tri[0] as usize, tri[1] as usize, tri[2] as usize];
        if a.max(b).max(c) >= positions.len() {
            continue;
        }
        let n = (positions[b] - positions[a]).cross(positions[c] - positions[a]);
        out[a] += n;
        out[b] += n;
        out[c] += n;
    }
    for n in out.iter_mut() {
        *n = n.try_normalize().unwrap_or(Vec3::Y);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flat_triangle_has_up_normals() {
        let mut mesh = SurfaceMesh::new(vec![0, 2, 1], 3);
        mesh.update(&[Vec3::ZERO, Vec3::X, Vec3::Z]);
        for n in &mesh.normals {
            assert!((*n - Vec3::Y).length() < 1e-5, "{n:?}");
        }
        assert_eq!(mesh.positions[1], Vec3::X);
    }

    #[test]
    fn mismatched_update_is_ignored() {
        let mut mesh = SurfaceMesh::new(vec![0, 1, 2], 3);
        mesh.update(&[Vec3::X]);
        assert_eq!(mesh.positions[0], Vec3::ZERO);
    }
}
