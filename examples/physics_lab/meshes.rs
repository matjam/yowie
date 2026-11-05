//! Geometry for the lab: the rolling terrain (render mesh and collision
//! mesh built from the same vertices) and the two convex hulls.

use yowie::gfx::scene::{MeshData, Vertex3D};
use yowie::{Vec3, vec2};

/// Half the terrain's width, and the number of cells along each side.
pub const TERRAIN_HALF: f32 = 20.0;
const TERRAIN_CELLS: usize = 40;

/// The rolling ground the bodies land on.
pub fn terrain_height(x: f32, z: f32) -> f32 {
    1.2 * (x * 0.35).sin() * (z * 0.3).cos() + 0.4 * ((x + z) * 0.9).sin()
}

/// The terrain as a render mesh plus the positions and triangles the
/// collision mesh uses.
pub struct Terrain {
    pub mesh: MeshData,
    pub positions: Vec<Vec3>,
    pub triangles: Vec<[u32; 3]>,
}

/// Builds the terrain. Triangles wind counter-clockwise seen from above.
pub fn terrain() -> Terrain {
    let n = TERRAIN_CELLS + 1;
    let mut vertices = Vec::with_capacity(n * n);
    let mut positions = Vec::with_capacity(n * n);
    for j in 0..n {
        for i in 0..n {
            let x = -TERRAIN_HALF + 2.0 * TERRAIN_HALF * i as f32 / TERRAIN_CELLS as f32;
            let z = -TERRAIN_HALF + 2.0 * TERRAIN_HALF * j as f32 / TERRAIN_CELLS as f32;
            let p = Vec3::new(x, terrain_height(x, z), z);
            let d = 0.1;
            let normal = Vec3::new(
                terrain_height(x - d, z) - terrain_height(x + d, z),
                2.0 * d,
                terrain_height(x, z - d) - terrain_height(x, z + d),
            )
            .normalize();
            let uv = vec2(i as f32 / 4.0, j as f32 / 4.0);
            vertices.push(Vertex3D::new(p, normal, uv));
            positions.push(p);
        }
    }
    let mut triangles = Vec::with_capacity(TERRAIN_CELLS * TERRAIN_CELLS * 2);
    for j in 0..TERRAIN_CELLS {
        for i in 0..TERRAIN_CELLS {
            let a = (j * n + i) as u32;
            let b = a + n as u32;
            triangles.push([a, b, a + 1]);
            triangles.push([a + 1, b, b + 1]);
        }
    }
    let indices = triangles.iter().flatten().copied().collect();
    Terrain {
        mesh: MeshData::new(vertices, indices),
        positions,
        triangles,
    }
}

/// The six points of an octahedron.
pub fn octahedron_points(r: f32) -> Vec<Vec3> {
    vec![
        Vec3::new(r, 0.0, 0.0),
        Vec3::new(-r, 0.0, 0.0),
        Vec3::new(0.0, r, 0.0),
        Vec3::new(0.0, -r, 0.0),
        Vec3::new(0.0, 0.0, r),
        Vec3::new(0.0, 0.0, -r),
    ]
}

/// The faces of [`octahedron_points`] as index lists.
const OCTAHEDRON_FACES: [&[usize]; 8] = [
    &[0, 2, 4],
    &[4, 2, 1],
    &[1, 2, 5],
    &[5, 2, 0],
    &[0, 4, 3],
    &[4, 1, 3],
    &[1, 5, 3],
    &[5, 0, 3],
];

/// The six points of a doorstop with half extents `w`, `h` and `d`.
pub fn wedge_points(w: f32, h: f32, d: f32) -> Vec<Vec3> {
    vec![
        Vec3::new(-w, -h, -d),
        Vec3::new(w, -h, -d),
        Vec3::new(w, -h, d),
        Vec3::new(-w, -h, d),
        Vec3::new(-w, h, -d),
        Vec3::new(-w, h, d),
    ]
}

/// The faces of [`wedge_points`] as index lists.
const WEDGE_FACES: [&[usize]; 5] = [
    &[0, 1, 2, 3],
    &[0, 3, 5, 4],
    &[4, 5, 2, 1],
    &[3, 2, 5],
    &[0, 4, 1],
];

/// A flat-shaded mesh of a convex solid given its points and polygon faces
/// (each a list of coplanar point indices in either winding).
fn flat_mesh(points: &[Vec3], faces: &[&[usize]]) -> MeshData {
    let centre = points.iter().copied().sum::<Vec3>() / points.len() as f32;
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    for face in faces {
        let p: Vec<Vec3> = face.iter().map(|&i| points[i]).collect();
        let mut order: Vec<usize> = (0..p.len()).collect();
        let mut normal = (p[1] - p[0]).cross(p[2] - p[0]).normalize();
        let middle = p.iter().copied().sum::<Vec3>() / p.len() as f32;
        if normal.dot(middle - centre) < 0.0 {
            normal = -normal;
            order.reverse();
        }
        let base = vertices.len() as u32;
        for &k in &order {
            vertices.push(Vertex3D::new(p[k], normal, vec2(0.0, 0.0)));
        }
        for k in 1..order.len() as u32 - 1 {
            indices.extend([base, base + k, base + k + 1]);
        }
    }
    MeshData::new(vertices, indices)
}

/// The octahedron's render mesh.
pub fn octahedron_mesh(r: f32) -> MeshData {
    flat_mesh(&octahedron_points(r), &OCTAHEDRON_FACES)
}

/// The doorstop's render mesh.
pub fn wedge_mesh(w: f32, h: f32, d: f32) -> MeshData {
    flat_mesh(&wedge_points(w, h, d), &WEDGE_FACES)
}
