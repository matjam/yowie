//! Resolving a recorded scene on the processor: culling, sorting and
//! batching the draws into instance runs, placing the shadow maps, and
//! clustering the lights.
//!
//! Opaque and cut-out draws sort by a packed key (pipeline, material,
//! mesh) so equal draws become one instanced call; the batches are then
//! ordered front to back within each pipeline, and each batch's instances
//! front to back. Blended draws sort back to front, and neighbours that
//! share a key still merge.

use yowie_core::{Mat4, Vec3, Vec4};

use super::bounds::{Aabb, Frustum};
use super::camera::{Camera3D, Projection};
use super::classes::{Extras, PassLists, classify};
use super::cluster::{ClusterGrid, MAX_LIGHTS, assign};
use super::frame_data::FrameUniform;
use super::light::{GpuLight, LocalLight};
use super::pipelines::{MeshKey, MeshPass};
use super::record::{DrawRecord, SceneRecord};
use super::shadow::{self, MAX_POINT_SHADOWS, MAX_SPOT_SHADOWS, POINT_BASE, SPOT_BASE, ShadowView};
use super::vertex::{GpuInstance, affine_rows};

/// A run of instances drawn with one pipeline, mesh and material.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Batch {
    pub(crate) key: MeshKey,
    /// The packed sort key the batch's draws share.
    pub(crate) sort: u64,
    pub(crate) mesh: u32,
    pub(crate) material: u32,
    pub(crate) first: u32,
    pub(crate) count: u32,
}

/// What the GPU passes of one scene need.
#[derive(Debug, Default)]
pub(crate) struct Prepared {
    pub(crate) frame: FrameUniform,
    pub(crate) instances: Vec<GpuInstance>,
    pub(crate) opaque: Vec<Batch>,
    /// Sorted blended batches, back to front.
    pub(crate) blend: Vec<Batch>,
    /// Transmissive batches, drawn after the scene copy.
    pub(crate) transmit: Vec<Batch>,
    /// Order-independent blended batches.
    pub(crate) oit: Vec<Batch>,
    /// Outline hulls and x-ray tints, drawn after the opaque scene.
    pub(crate) outline: Vec<Batch>,
    pub(crate) xray: Vec<Batch>,
    /// The scene's decals, written by the renderer once their images have
    /// atlas cells.
    pub(crate) decals: Vec<super::decals::GpuDecal>,
    /// Shadow views in render order, each with its batches.
    pub(crate) shadows: Vec<(usize, Vec<Batch>)>,
    pub(crate) lights: Vec<GpuLight>,
    pub(crate) clusters: Vec<u32>,
    pub(crate) light_indices: Vec<u32>,
    /// The lights given shadow maps, with their first shadow view.
    pub(crate) shadow_lights: Vec<(LocalLight, usize)>,
    pub(crate) culled: u32,
    pub(crate) lights_dropped: u32,
    pub(crate) shadow_instances: u32,
}

/// One draw after culling: its sort key, depth and sphere.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Resolved {
    pub(crate) key: u64,
    pub(crate) depth: f32,
    pub(crate) sphere: Vec4,
    pub(crate) draw: u32,
    pub(crate) mesh_key: MeshKey,
}

pub(crate) fn pack_key(key: &MeshKey, material: u32, mesh: u32) -> u64 {
    (u64::from(key.index()) << 56)
        | (u64::from(material & 0x0fff_ffff) << 28)
        | u64::from(mesh & 0x0fff_ffff)
}

/// The world box of a draw: a skinned draw's rest box moved by each joint
/// matrix, which holds every blend of them.
fn world_bounds(rec: &SceneRecord, d: &DrawRecord, local: Aabb, skinned: bool) -> Aabb {
    if skinned {
        let joints = &rec.joints[d.joints as usize..];
        let count = joints.len().min(256);
        let posed = joints[..count]
            .iter()
            .fold(Aabb::EMPTY, |acc, j| acc.union(local.transformed(j)));
        posed.transformed(&d.model)
    } else {
        local.transformed(&d.model)
    }
}

/// The instance record of a draw, for shadow view `view`.
pub(crate) fn instance(d: &DrawRecord, view: u32) -> GpuInstance {
    GpuInstance {
        rows: affine_rows(&d.model),
        params: [
            d.material,
            d.joints,
            d.morph_first,
            d.morph_count | (view << 8),
        ],
    }
}

/// Builds batches from resolved draws already in draw order, merging
/// neighbours with equal keys, and appends their instances.
pub(crate) fn emit(
    rec: &SceneRecord,
    order: &[Resolved],
    view: u32,
    instances: &mut Vec<GpuInstance>,
    out: &mut Vec<Batch>,
) {
    for r in order {
        let d = &rec.draws[r.draw as usize];
        let at = instances.len() as u32;
        instances.push(instance(d, view));
        match out.last_mut() {
            Some(b) if b.sort == r.key && b.first + b.count == at => {
                b.count += 1;
            }
            _ => out.push(Batch {
                key: r.mesh_key,
                sort: r.key,
                mesh: d.mesh,
                material: d.material,
                first: at,
                count: 1,
            }),
        }
    }
}

/// Per-mesh facts looked up once a frame.
struct MeshInfo {
    bounds: Aabb,
    skinned: bool,
}

/// The camera facts the preparation needs.
pub(crate) struct View {
    pub(crate) view: Mat4,
    pub(crate) proj: Mat4,
    pub(crate) view_proj: Mat4,
    pub(crate) position: Vec3,
    pub(crate) forward: Vec3,
    pub(crate) near: f32,
    pub(crate) far: f32,
    pub(crate) ortho: bool,
    pub(crate) size: yowie_core::Vec2,
}

impl View {
    /// The view of `cam` over a target `size` pixels large.
    pub(crate) fn new(cam: &Camera3D, size: yowie_core::Vec2) -> Self {
        let proj = cam.projection_matrix(size.x / size.y);
        let view = cam.view_matrix();
        Self {
            view,
            proj,
            view_proj: proj * view,
            position: cam.position,
            forward: cam.forward(),
            near: cam.near.max(1e-4),
            far: cam.far.max(cam.near * 1.001),
            ortho: matches!(cam.projection, Projection::Orthographic { .. }),
            size,
        }
    }
}

pub(crate) fn prepare(rec: &SceneRecord, v: &View, out: &mut Prepared) {
    out.frame = FrameUniform::default();
    out.instances.clear();
    out.opaque.clear();
    out.blend.clear();
    out.transmit.clear();
    out.oit.clear();
    out.outline.clear();
    out.xray.clear();
    out.shadows.clear();
    out.culled = 0;
    let mut extras = Extras::default();
    let frustum = Frustum::from_view_proj(&v.view_proj);
    let infos: Vec<MeshInfo> = rec
        .meshes
        .iter()
        .map(|m| MeshInfo {
            bounds: m.bounds(),
            skinned: m.is_skinned(),
        })
        .collect();
    let mut lit: Vec<Resolved> = Vec::with_capacity(rec.draws.len());
    let mut blend: Vec<Resolved> = Vec::new();
    let mut casters: Vec<Resolved> = Vec::new();
    let sun_shadows = rec.sun.is_some_and(|s| s.shadows);
    let any_shadows = sun_shadows || rec.lights.iter().any(LocalLight::shadows);
    for (i, d) in rec.draws.iter().enumerate() {
        let info = &infos[d.mesh as usize];
        let skinned = info.skinned && d.joints != u32::MAX;
        let material = &rec.materials[d.material as usize];
        let class = classify(material, rec.transparency);
        let pass = class.pass;
        let mesh_key = MeshKey {
            pass,
            double_sided: material.double_sided,
            skinned,
            stencil: class.stencil,
            shader: class.shader,
        };
        let b = world_bounds(rec, d, info.bounds, skinned);
        let sphere = b.center().extend(b.radius() + class.reach());
        let r = Resolved {
            key: pack_key(&mesh_key, d.material, d.mesh),
            depth: (b.center() - v.position).dot(v.forward),
            sphere,
            draw: i as u32,
            mesh_key,
        };
        if any_shadows && class.casts_shadow() {
            let shadow_pass = if pass == MeshPass::Mask {
                MeshPass::ShadowMask
            } else {
                MeshPass::ShadowOpaque
            };
            let sk = MeshKey {
                pass: shadow_pass,
                double_sided: true,
                skinned,
                ..MeshKey::default()
            };
            // Opaque casters ignore the material, so they batch by mesh.
            let mat = if shadow_pass == MeshPass::ShadowMask {
                d.material
            } else {
                0
            };
            casters.push(Resolved {
                key: pack_key(&sk, mat, d.mesh),
                mesh_key: sk,
                ..r
            });
        }
        if !frustum.intersects_sphere(sphere.truncate(), sphere.w) {
            out.culled += 1;
            continue;
        }
        match pass {
            MeshPass::Blend => blend.push(r),
            MeshPass::Opaque | MeshPass::Mask => lit.push(r),
            _ => {}
        }
        extras.add(rec, &class, r);
    }
    // Opaque: group by key with instances front to back, then order the
    // batches front to back within each pipeline.
    lit.sort_unstable_by(|a, b| a.key.cmp(&b.key).then(a.depth.total_cmp(&b.depth)));
    let mut groups: Vec<(u32, f32, std::ops::Range<usize>)> = Vec::new();
    let mut start = 0;
    for i in 1..=lit.len() {
        if i == lit.len() || lit[i].key != lit[start].key {
            groups.push((lit[start].mesh_key.index(), lit[start].depth, start..i));
            start = i;
        }
    }
    groups.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)));
    for (_, _, range) in groups {
        emit(rec, &lit[range], 0, &mut out.instances, &mut out.opaque);
    }
    extras.emit(
        rec,
        &mut out.instances,
        &mut PassLists {
            opaque: &mut out.opaque,
            transmit: &mut out.transmit,
            oit: &mut out.oit,
            outline: &mut out.outline,
            xray: &mut out.xray,
        },
    );
    blend.sort_by(|a, b| b.depth.total_cmp(&a.depth));
    emit(rec, &blend, 0, &mut out.instances, &mut out.blend);
    prepare_lights(rec, v, &frustum, out);
    if any_shadows {
        casters.sort_unstable_by_key(|c| c.key);
        prepare_shadows(rec, v, &casters, out);
    }
}

fn prepare_lights(rec: &SceneRecord, v: &View, frustum: &Frustum, out: &mut Prepared) {
    out.lights.clear();
    out.shadow_lights.clear();
    out.lights_dropped = 0;
    let mut spheres = Vec::new();
    let (mut spots, mut points) = (0, 0);
    for l in &rec.lights {
        let (p, r) = (l.position(), l.range());
        if !frustum.intersects_sphere(p, r) {
            continue;
        }
        if out.lights.len() >= MAX_LIGHTS {
            out.lights_dropped += 1;
            continue;
        }
        let mut shadow_view = -1;
        if l.shadows() {
            match l {
                LocalLight::Spot(_) if spots < MAX_SPOT_SHADOWS => {
                    shadow_view = (SPOT_BASE + spots) as i32;
                    spots += 1;
                }
                LocalLight::Point(_) if points < MAX_POINT_SHADOWS => {
                    shadow_view = (POINT_BASE + points * 6) as i32;
                    points += 1;
                }
                _ => {}
            }
        }
        if shadow_view >= 0 {
            out.shadow_lights.push((*l, shadow_view as usize));
        }
        out.lights.push(l.gpu(shadow_view));
        spheres.push(v.view.transform_point3(p).extend(r));
    }
    let grid = ClusterGrid::new(v.size, v.near, v.far);
    let dropped = assign(
        &grid,
        &v.proj,
        &spheres,
        &mut out.clusters,
        &mut out.light_indices,
    );
    out.lights_dropped += dropped;
    let (c, d) = grid.uniforms();
    out.frame.cluster = c;
    out.frame.cluster_depth = d;
    out.frame.counts[0] = out.lights.len() as u32;
}

fn prepare_shadows(rec: &SceneRecord, v: &View, casters: &[Resolved], out: &mut Prepared) {
    let mut views: Vec<ShadowView> = Vec::new();
    if let Some(sun) = rec.sun.filter(|s| s.shadows) {
        let to_sun = -sun.direction.normalize_or(Vec3::NEG_Y);
        let reach = casters
            .iter()
            .map(|c| c.sphere.truncate().dot(to_sun) + c.sphere.w)
            .fold(f32::NEG_INFINITY, f32::max);
        let distance = sun.shadow_distance.min(v.far);
        let splits = shadow::cascade_splits(v.near, if distance > 0.0 { distance } else { 60.0 });
        let inv = v.view_proj.inverse();
        views.extend(shadow::cascades(
            &inv, v.near, v.far, v.ortho, &splits, to_sun, reach,
        ));
        out.frame.cascade_far = [splits[0], splits[1], splits[2], 0.0];
        out.frame.shadow[2] = shadow::CASCADES as f32;
    }
    for (l, g) in &out.shadow_lights {
        let g = *g;
        match l {
            LocalLight::Spot(s) => views.push(shadow::spot(
                g,
                s.position,
                s.direction,
                s.outer_angle,
                l.range(),
            )),
            LocalLight::Point(p) => views.extend(shadow::point_faces(g, p.position, l.range())),
        }
    }
    let texel = shadow::atlas_texel();
    out.frame.shadow[0] = texel[0];
    out.frame.shadow[1] = texel[1];
    out.shadow_instances = 0;
    let mut pass: Vec<Resolved> = Vec::new();
    for view in &views {
        out.frame.shadow_views[view.index] = view.gpu();
        pass.clear();
        pass.extend(casters.iter().filter(|c| {
            view.frustum
                .intersects_sphere(c.sphere.truncate(), c.sphere.w)
        }));
        let mut batches = Vec::new();
        emit(
            rec,
            &pass,
            view.index as u32,
            &mut out.instances,
            &mut batches,
        );
        out.shadow_instances += pass.len() as u32;
        out.shadows.push((view.index, batches));
    }
}
