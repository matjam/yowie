//! The 3D vertex format, geometry on the processor, and the GPU vertex
//! streams it is packed into.

use bytemuck::{Pod, Zeroable};
use yowie_core::{Color, Mat3, Mat4, Vec2, Vec3, Vec4};

use super::bounds::Aabb;

/// One vertex of a 3D mesh.
///
/// `normal` and `tangent` are unit vectors in mesh space; `tangent.w` is
/// the handedness of the bitangent (+1 or -1), as glTF stores it. `uv` is
/// the first texture coordinate set and `uv2` the second (lightmaps,
/// occlusion). `color` is sRGB with straight alpha and multiplies the
/// material's base colour when the material uses vertex colours.
/// `joints` and `weights` bind a skinned mesh to four joints; unskinned
/// meshes leave them at zero.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vertex3D {
    /// Position in mesh space.
    pub position: Vec3,
    /// Unit normal in mesh space.
    pub normal: Vec3,
    /// Unit tangent along +U, with the bitangent sign in `w`.
    pub tangent: Vec4,
    /// First texture coordinate set.
    pub uv: Vec2,
    /// Second texture coordinate set.
    pub uv2: Vec2,
    /// Vertex colour, sRGB with straight alpha.
    pub color: Color,
    /// Indices of the four joints that move this vertex.
    pub joints: [u16; 4],
    /// Weights of the four joints, summing to one for a skinned vertex.
    pub weights: [f32; 4],
}

impl Default for Vertex3D {
    fn default() -> Self {
        Self {
            position: Vec3::ZERO,
            normal: Vec3::Y,
            tangent: Vec4::new(1.0, 0.0, 0.0, 1.0),
            uv: Vec2::ZERO,
            uv2: Vec2::ZERO,
            color: Color::WHITE,
            joints: [0; 4],
            weights: [0.0; 4],
        }
    }
}

impl Vertex3D {
    /// A vertex at `position` with `normal` and texture coordinate `uv`.
    pub fn new(position: Vec3, normal: Vec3, uv: Vec2) -> Self {
        Self {
            position,
            normal,
            uv,
            ..Self::default()
        }
    }
}

/// Indexed triangle geometry on the processor: what the shape generators
/// return and [`Mesh::new`](super::Mesh::new) uploads. Triangles wind
/// counter-clockwise seen from the front.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MeshData {
    /// The vertices.
    pub vertices: Vec<Vertex3D>,
    /// Three indices per triangle into `vertices`.
    pub indices: Vec<u32>,
}

impl MeshData {
    /// Geometry from vertices and indices.
    pub fn new(vertices: Vec<Vertex3D>, indices: Vec<u32>) -> Self {
        Self { vertices, indices }
    }

    /// The box holding every vertex.
    pub fn bounds(&self) -> Aabb {
        Aabb::from_points(self.vertices.iter().map(|v| v.position))
    }

    /// Whether any vertex has a joint weight, which makes the mesh skinned.
    pub fn is_skinned(&self) -> bool {
        self.vertices
            .iter()
            .any(|v| v.weights.iter().any(|w| *w > 0.0))
    }

    /// A copy moved by `m`: positions, normals and tangents.
    pub fn transformed(&self, m: Mat4) -> MeshData {
        let normal_m = Mat3::from_mat4(m).inverse().transpose();
        let vertices = self
            .vertices
            .iter()
            .map(|v| Vertex3D {
                position: m.transform_point3(v.position),
                normal: (normal_m * v.normal).normalize_or(Vec3::Y),
                tangent: m
                    .transform_vector3(v.tangent.truncate())
                    .normalize_or(Vec3::X)
                    .extend(v.tangent.w),
                ..*v
            })
            .collect();
        MeshData {
            vertices,
            indices: self.indices.clone(),
        }
    }

    /// Appends `other`'s triangles, for merging shapes into one mesh.
    pub fn append(&mut self, other: &MeshData) {
        let base = self.vertices.len() as u32;
        self.vertices.extend_from_slice(&other.vertices);
        self.indices.extend(other.indices.iter().map(|i| i + base));
    }

    /// Sets every vertex colour to `color`.
    pub fn with_color(mut self, color: Color) -> Self {
        for v in &mut self.vertices {
            v.color = color;
        }
        self
    }

    /// Recomputes smooth normals from the triangles, weighting each face by
    /// its area.
    pub fn compute_normals(&mut self) {
        let mut acc = vec![Vec3::ZERO; self.vertices.len()];
        for t in self.indices.as_chunks::<3>().0 {
            let [a, b, c] = t.map(|i| i as usize);
            if a.max(b).max(c) >= acc.len() {
                continue;
            }
            let p = |i: usize| self.vertices[i].position;
            let n = (p(b) - p(a)).cross(p(c) - p(a));
            for i in [a, b, c] {
                acc[i] += n;
            }
        }
        for (v, n) in self.vertices.iter_mut().zip(acc) {
            v.normal = n.normalize_or(Vec3::Y);
        }
    }

    /// Recomputes tangents from the first texture coordinates, so normal
    /// maps work on geometry built by hand.
    pub fn compute_tangents(&mut self) {
        let n = self.vertices.len();
        let mut tan = vec![Vec3::ZERO; n];
        let mut bit = vec![Vec3::ZERO; n];
        for t in self.indices.as_chunks::<3>().0 {
            let [a, b, c] = t.map(|i| i as usize);
            if a.max(b).max(c) >= n {
                continue;
            }
            let (va, vb, vc) = (self.vertices[a], self.vertices[b], self.vertices[c]);
            let (e1, e2) = (vb.position - va.position, vc.position - va.position);
            let (d1, d2) = (vb.uv - va.uv, vc.uv - va.uv);
            let det = d1.x * d2.y - d2.x * d1.y;
            if det.abs() < 1e-12 {
                continue;
            }
            let r = 1.0 / det;
            let t = (e1 * d2.y - e2 * d1.y) * r;
            let bt = (e2 * d1.x - e1 * d2.x) * r;
            for i in [a, b, c] {
                tan[i] += t;
                bit[i] += bt;
            }
        }
        for (i, v) in self.vertices.iter_mut().enumerate() {
            let nrm = v.normal;
            let mut t = tan[i] - nrm * nrm.dot(tan[i]);
            if t.length_squared() < 1e-12 {
                t = nrm.any_orthonormal_vector();
            }
            let t = t.normalize();
            let w = if nrm.cross(t).dot(bit[i]) < 0.0 {
                -1.0
            } else {
                1.0
            };
            v.tangent = t.extend(w);
        }
    }
}

/// The vertex stream every mesh has: 72 bytes per vertex.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub(crate) struct GpuVertex {
    pub(crate) position: [f32; 3],
    pub(crate) normal: [f32; 3],
    pub(crate) tangent: [f32; 4],
    pub(crate) uv: [f32; 2],
    pub(crate) uv2: [f32; 2],
    /// Linear, straight alpha.
    pub(crate) color: [f32; 4],
}

/// The second stream of a skinned mesh: joint indices and weights.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub(crate) struct GpuSkin {
    pub(crate) joints: [u16; 4],
    pub(crate) weights: [f32; 4],
}

impl From<&Vertex3D> for GpuVertex {
    fn from(v: &Vertex3D) -> Self {
        Self {
            position: v.position.to_array(),
            normal: v.normal.to_array(),
            tangent: v.tangent.to_array(),
            uv: v.uv.to_array(),
            uv2: v.uv2.to_array(),
            color: v.color.to_linear(),
        }
    }
}

impl From<&Vertex3D> for GpuSkin {
    fn from(v: &Vertex3D) -> Self {
        Self {
            joints: v.joints,
            weights: v.weights,
        }
    }
}

pub(crate) const VERTEX_ATTRIBUTES: [wgpu::VertexAttribute; 6] = wgpu::vertex_attr_array![
    0 => Float32x3,
    1 => Float32x3,
    2 => Float32x4,
    3 => Float32x2,
    4 => Float32x2,
    5 => Float32x4,
];

pub(crate) const SKIN_ATTRIBUTES: [wgpu::VertexAttribute; 2] = wgpu::vertex_attr_array![
    10 => Uint16x4,
    11 => Float32x4,
];

/// One instance in the instance stream: the model matrix's top three rows
/// and the draw's material, joint and morph offsets. The vertex programs
/// read it at locations 6 to 9.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub(crate) struct GpuInstance {
    pub(crate) rows: [[f32; 4]; 3],
    /// Material index, first joint matrix (or `u32::MAX`), first morph
    /// weight, morph weight count.
    pub(crate) params: [u32; 4],
}

pub(crate) const INSTANCE_ATTRIBUTES: [wgpu::VertexAttribute; 4] = wgpu::vertex_attr_array![
    6 => Float32x4,
    7 => Float32x4,
    8 => Float32x4,
    9 => Uint32x4,
];

/// The three vertex buffer layouts: mesh vertices, instances, skin.
pub(crate) fn buffer_layouts(skinned: bool) -> Vec<Option<wgpu::VertexBufferLayout<'static>>> {
    let mut v = vec![
        Some(wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<GpuVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &VERTEX_ATTRIBUTES,
        }),
        Some(wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<GpuInstance>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &INSTANCE_ATTRIBUTES,
        }),
    ];
    if skinned {
        v.push(Some(wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<GpuSkin>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &SKIN_ATTRIBUTES,
        }));
    }
    v
}

/// The rows of `m` the instance stream stores (its bottom row is always
/// `0 0 0 1` for the affine transforms meshes are drawn with).
pub(crate) fn affine_rows(m: &Mat4) -> [[f32; 4]; 3] {
    let t = m.transpose();
    [
        t.x_axis.to_array(),
        t.y_axis.to_array(),
        t.z_axis.to_array(),
    ]
}
