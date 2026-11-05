//! Meshes: indexed triangle geometry on the GPU, with a copy kept for
//! bounds and picking.

use std::sync::Arc;

use parking_lot::{Mutex, RwLock};
use yowie_core::{Mat4, Vec3};

use super::bounds::{Aabb, Ray};
use super::morph::MorphTarget;
use super::vertex::{GpuSkin, GpuVertex, MeshData};
use crate::Graphics;
use crate::error::{GfxError, Result};
use crate::shared::Shared;
use crate::texture::next_id;

/// Where a ray met a mesh.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RayHit {
    /// The distance along the ray, in world units.
    pub distance: f32,
    /// The world position of the hit.
    pub point: Vec3,
    /// The unit world normal of the triangle that was hit, facing the ray.
    pub normal: Vec3,
    /// The index of the triangle (its first index divided by three).
    pub triangle: u32,
}

/// The GPU side of a mesh, replaced when an update outgrows it.
#[derive(Debug)]
pub(crate) struct MeshGpu {
    pub(crate) vertices: wgpu::Buffer,
    pub(crate) skin: Option<wgpu::Buffer>,
    pub(crate) indices: wgpu::Buffer,
    pub(crate) index_count: u32,
    pub(crate) vertex_count: u32,
}

#[derive(Debug)]
pub(crate) struct MeshInner {
    pub(crate) id: u64,
    pub(crate) gpu: Mutex<MeshGpu>,
    /// Positions and indices for picking.
    pub(crate) cpu: RwLock<(Vec<Vec3>, Vec<u32>)>,
    pub(crate) bounds: RwLock<(Aabb, bool)>,
    pub(crate) morph: Option<super::morph::MorphBuffer>,
}

/// Indexed triangle geometry on the GPU. Cloning shares the mesh; it is
/// freed when the last clone and every queued draw that uses it are gone.
///
/// Draw it with [`Scene::draw`](super::Scene::draw). A mesh whose vertices
/// carry joint weights is skinned and needs joint matrices
/// ([`Scene::draw_skinned`](super::Scene::draw_skinned)).
#[derive(Clone, Debug)]
pub struct Mesh {
    pub(crate) inner: Arc<MeshInner>,
}

fn buffer(shared: &Shared, label: &str, bytes: &[u8], usage: wgpu::BufferUsages) -> wgpu::Buffer {
    let size = (bytes.len() as u64).max(16).next_multiple_of(4);
    let buf = shared.device().create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size,
        usage: usage | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    write(shared, &buf, bytes);
    buf
}

/// Writes `bytes` at the start of `buf`, padded to the copy alignment.
fn write(shared: &Shared, buf: &wgpu::Buffer, bytes: &[u8]) {
    if bytes.is_empty() {
        return;
    }
    if bytes.len().is_multiple_of(4) {
        shared.queue().write_buffer(buf, 0, bytes);
    } else {
        let mut padded = bytes.to_vec();
        padded.resize(bytes.len().next_multiple_of(4), 0);
        shared.queue().write_buffer(buf, 0, &padded);
    }
}

fn validate(data: &MeshData) -> Result<()> {
    if data.vertices.is_empty() || data.indices.is_empty() {
        return Err(GfxError::Mesh("a mesh needs vertices and indices".into()));
    }
    if !data.indices.len().is_multiple_of(3) {
        return Err(GfxError::Mesh(format!(
            "index count {} is not a multiple of three",
            data.indices.len()
        )));
    }
    let n = data.vertices.len() as u32;
    if let Some(bad) = data.indices.iter().find(|i| **i >= n) {
        return Err(GfxError::Mesh(format!(
            "index {bad} is past the last of {n} vertices"
        )));
    }
    Ok(())
}

fn upload(shared: &Shared, data: &MeshData, skinned: bool) -> MeshGpu {
    let verts: Vec<GpuVertex> = data.vertices.iter().map(GpuVertex::from).collect();
    let skin = skinned.then(|| {
        let s: Vec<GpuSkin> = data.vertices.iter().map(GpuSkin::from).collect();
        buffer(
            shared,
            "yowie mesh skin",
            bytemuck::cast_slice(&s),
            wgpu::BufferUsages::VERTEX,
        )
    });
    MeshGpu {
        vertices: buffer(
            shared,
            "yowie mesh vertices",
            bytemuck::cast_slice(&verts),
            wgpu::BufferUsages::VERTEX,
        ),
        skin,
        indices: buffer(
            shared,
            "yowie mesh indices",
            bytemuck::cast_slice(&data.indices),
            wgpu::BufferUsages::INDEX,
        ),
        index_count: data.indices.len() as u32,
        vertex_count: data.vertices.len() as u32,
    }
}

impl Mesh {
    /// Uploads geometry. Fails when it has no triangles, an index count
    /// that is not a multiple of three, or an index past the last vertex.
    pub fn new(gfx: &Graphics, data: &MeshData) -> Result<Mesh> {
        Mesh::create(&gfx.shared, data, &[])
    }

    /// Uploads geometry with morph targets, which
    /// [`Scene::draw_morphed`](super::Scene::draw_morphed) blends by weight
    /// in the vertex program. Every target must have one delta per vertex.
    pub fn with_morph_targets(
        gfx: &Graphics,
        data: &MeshData,
        targets: &[MorphTarget],
    ) -> Result<Mesh> {
        Mesh::create(&gfx.shared, data, targets)
    }

    pub(crate) fn create(
        shared: &Shared,
        data: &MeshData,
        targets: &[MorphTarget],
    ) -> Result<Mesh> {
        validate(data)?;
        let morph = if targets.is_empty() {
            None
        } else {
            Some(super::morph::MorphBuffer::new(
                shared,
                data.vertices.len(),
                targets,
            )?)
        };
        let gpu = upload(shared, data, data.is_skinned());
        let mut bounds = data.bounds();
        if let Some(m) = &morph {
            bounds = bounds.union(m.reach(data));
        }
        Ok(Mesh {
            inner: Arc::new(MeshInner {
                id: next_id(),
                gpu: Mutex::new(gpu),
                cpu: RwLock::new((
                    data.vertices.iter().map(|v| v.position).collect(),
                    data.indices.clone(),
                )),
                bounds: RwLock::new((bounds, false)),
                morph,
            }),
        })
    }

    /// Replaces the geometry in place, for terrain edits, voxel chunks and
    /// meshes that grow. The buffers are rewritten, or replaced when the
    /// new geometry does not fit; every draw of the current frame sees the
    /// new geometry. A skinned mesh stays skinned. Bounds set with
    /// [`set_bounds`](Self::set_bounds) are kept.
    pub fn update(&self, gfx: &Graphics, data: &MeshData) -> Result<()> {
        validate(data)?;
        let shared = &gfx.shared;
        let mut gpu = self.inner.gpu.lock();
        if let Some(m) = &self.inner.morph
            && m.vertex_count != data.vertices.len()
        {
            return Err(GfxError::Mesh(
                "a morphed mesh must keep its vertex count".into(),
            ));
        }
        let skinned = gpu.skin.is_some() || data.is_skinned();
        let verts: Vec<GpuVertex> = data.vertices.iter().map(GpuVertex::from).collect();
        let vbytes: &[u8] = bytemuck::cast_slice(&verts);
        let ibytes: &[u8] = bytemuck::cast_slice(&data.indices);
        let fits = vbytes.len() as u64 <= gpu.vertices.size()
            && ibytes.len() as u64 <= gpu.indices.size()
            && gpu.skin.is_some() == skinned;
        if fits {
            write(shared, &gpu.vertices, vbytes);
            write(shared, &gpu.indices, ibytes);
            if let Some(s) = &gpu.skin {
                let skin: Vec<GpuSkin> = data.vertices.iter().map(GpuSkin::from).collect();
                write(shared, s, bytemuck::cast_slice(&skin));
            }
            gpu.index_count = data.indices.len() as u32;
            gpu.vertex_count = data.vertices.len() as u32;
        } else {
            *gpu = upload(shared, data, skinned);
        }
        *self.inner.cpu.write() = (
            data.vertices.iter().map(|v| v.position).collect(),
            data.indices.clone(),
        );
        let mut b = self.inner.bounds.write();
        if !b.1 {
            b.0 = data.bounds();
        }
        Ok(())
    }

    /// The box holding the mesh in mesh space, morph targets at full
    /// weight included. Culling uses it.
    pub fn bounds(&self) -> Aabb {
        self.inner.bounds.read().0
    }

    /// Sets the box culling uses, for a mesh whose drawn shape leaves its
    /// vertices (skinned meshes, shaders that move vertices). Updates keep
    /// it.
    pub fn set_bounds(&self, bounds: Aabb) {
        *self.inner.bounds.write() = (bounds, true);
    }

    /// The number of vertices.
    pub fn vertex_count(&self) -> u32 {
        self.inner.gpu.lock().vertex_count
    }

    /// The number of indices: three per triangle.
    pub fn index_count(&self) -> u32 {
        self.inner.gpu.lock().index_count
    }

    /// Whether the mesh has joint weights.
    pub fn is_skinned(&self) -> bool {
        self.inner.gpu.lock().skin.is_some()
    }

    /// The number of morph targets.
    pub fn morph_target_count(&self) -> usize {
        self.inner.morph.as_ref().map_or(0, |m| m.target_count)
    }

    /// The nearest place a world-space ray meets the mesh placed by
    /// `model`, testing the bounds first and then every triangle. Skinned
    /// and morphed meshes are tested in their rest shape.
    pub fn intersect(&self, model: &Mat4, ray: &Ray) -> Option<RayHit> {
        let world_box = self.bounds().transformed(model);
        world_box.intersect_ray(ray)?;
        let inv = model.inverse();
        let local = ray.transformed(&inv);
        let cpu = self.inner.cpu.read();
        let (pos, idx) = (&cpu.0, &cpu.1);
        let mut best: Option<(f32, u32)> = None;
        for (i, t) in idx.as_chunks::<3>().0.iter().enumerate() {
            let [a, b, c] = t.map(|i| pos[i as usize]);
            if let Some(d) = local.intersect_triangle(a, b, c)
                && best.is_none_or(|(bd, _)| d < bd)
            {
                best = Some((d, i as u32));
            }
        }
        let (d, tri) = best?;
        let point = model.transform_point3(local.at(d));
        let t = &idx[tri as usize * 3..tri as usize * 3 + 3];
        let [a, b, c] = [t[0], t[1], t[2]].map(|i| model.transform_point3(pos[i as usize]));
        let mut normal = (b - a).cross(c - a).normalize_or(Vec3::Y);
        if normal.dot(ray.direction) > 0.0 {
            normal = -normal;
        }
        Some(RayHit {
            distance: (point - ray.origin).length(),
            point,
            normal,
            triangle: tri,
        })
    }

    pub(crate) fn id(&self) -> u64 {
        self.inner.id
    }
}

impl PartialEq for Mesh {
    fn eq(&self, other: &Self) -> bool {
        self.inner.id == other.inner.id
    }
}

impl Eq for Mesh {}
