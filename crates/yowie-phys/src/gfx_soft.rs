//! Turning soft-body surfaces into renderer geometry.

use yowie_core::Vec2;
use yowie_gfx::scene::{MeshData, Vertex3D};

use crate::soft::SurfaceMesh;

impl SurfaceMesh {
    /// Builds renderer geometry with one vertex per particle. Texture
    /// coordinates are zero and vertex colours white; set them on the
    /// result if the material uses them, and call
    /// [`write_to`](Self::write_to) each frame to follow the particles.
    pub fn to_mesh_data(&self) -> MeshData {
        let vertices = self
            .positions
            .iter()
            .zip(&self.normals)
            .map(|(p, n)| Vertex3D::new(*p, *n, Vec2::ZERO))
            .collect();
        MeshData::new(vertices, self.indices.clone())
    }

    /// Copies the positions and normals into `data`, keeping its texture
    /// coordinates, colours and tangents. Does nothing if `data` has a
    /// different vertex count. Pass the result to `Mesh::update` to upload
    /// it.
    pub fn write_to(&self, data: &mut MeshData) {
        if data.vertices.len() != self.positions.len() {
            return;
        }
        for ((v, p), n) in data
            .vertices
            .iter_mut()
            .zip(&self.positions)
            .zip(&self.normals)
        {
            v.position = *p;
            v.normal = *n;
        }
    }
}

#[cfg(test)]
mod tests {
    use yowie_core::Color;

    use crate::soft::{Cloth, ClothDesc, SoftSettings};

    #[test]
    fn write_to_follows_particles_and_keeps_colours() {
        let desc = ClothDesc {
            cols: 4,
            rows: 4,
            pinned: vec![0],
            ..ClothDesc::default()
        };
        let mut cloth = Cloth::new(&desc).unwrap();
        let mut surface = cloth.mesh();
        let mut data = surface.to_mesh_data();
        assert_eq!(data.vertices.len(), 16);
        data.vertices[5].color = Color::RED;
        for _ in 0..30 {
            cloth.step(1.0 / 60.0, &SoftSettings::default(), &());
        }
        cloth.update_mesh(&mut surface);
        surface.write_to(&mut data);
        assert_eq!(data.vertices[5].color, Color::RED);
        assert_eq!(data.vertices[5].position, cloth.positions()[5]);
        assert!(data.vertices[15].position.y < -0.3);
        assert!((data.vertices[10].normal.length() - 1.0).abs() < 1e-3);
    }
}
