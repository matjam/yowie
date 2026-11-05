//! Converting a glTF document into a [`Model`].

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use yowie_anim::gltf_import::{GltfImport, import_path, import_slice};
use yowie_core::{Color, Mat4, Quat, Transform, Vec2, Vec3, Vec4};

use super::textures::TextureCache;
use super::{Model, ModelData, ModelPart, Node, Skin};
use crate::Graphics;
use crate::error::{GfxError, Result};
use crate::scene::bounds::Aabb;
use crate::scene::material::{AlphaMode, Material, TextureTransform, UvSets};
use crate::scene::mesh::Mesh;
use crate::scene::morph::MorphTarget;
use crate::scene::vertex::{MeshData, Vertex3D};

fn err(e: impl std::fmt::Display) -> GfxError {
    GfxError::Gltf(e.to_string())
}

pub(super) fn load_path(gfx: &Graphics, path: &Path) -> Result<Model> {
    let (doc, buffers, images) = gltf::import(path).map_err(err)?;
    let anim = import_path(path).map_err(err)?;
    build(gfx, &doc, &buffers, &images, anim)
}

pub(super) fn load_slice(gfx: &Graphics, bytes: &[u8]) -> Result<Model> {
    let (doc, buffers, images) = gltf::import_slice(bytes).map_err(err)?;
    let anim = import_slice(bytes).map_err(err)?;
    build(gfx, &doc, &buffers, &images, anim)
}

fn build(
    gfx: &Graphics,
    doc: &gltf::Document,
    buffers: &[gltf::buffer::Data],
    images: &[gltf::image::Data],
    anim: GltfImport,
) -> Result<Model> {
    let mut textures = TextureCache::new(images);
    let mut materials: Vec<Material> = doc
        .materials()
        .map(|m| material(gfx, doc, &m, &mut textures))
        .collect::<Result<_>>()?;
    let default_material = materials.len();
    materials.push(Material::default());

    let mut nodes: Vec<Node> = doc
        .nodes()
        .map(|n| {
            let (t, r, s) = n.transform().decomposed();
            Node {
                parent: None,
                rest: Transform {
                    translation: Vec3::from_array(t),
                    rotation: Quat::from_array(r).normalize(),
                    scale: Vec3::from_array(s),
                },
                skin: n.skin().map(|s| s.index()),
                weights: n
                    .weights()
                    .or_else(|| n.mesh().and_then(|m| m.weights()))
                    .map(<[f32]>::to_vec)
                    .unwrap_or_default(),
            }
        })
        .collect();
    for n in doc.nodes() {
        for c in n.children() {
            nodes[c.index()].parent = Some(n.index());
        }
    }
    let order = parents_first(&nodes);

    // Meshes are uploaded once each and placed by every node using them.
    let mut meshes: HashMap<usize, Vec<(Mesh, usize, String)>> = HashMap::new();
    for m in doc.meshes() {
        let mut prims = Vec::new();
        for p in m.primitives() {
            if p.mode() != gltf::mesh::Mode::Triangles {
                log::warn!("yowie-gfx: skipping a non-triangle glTF primitive");
                continue;
            }
            let (data, targets) = primitive(&p, buffers)?;
            let mesh = Mesh::create(&gfx.shared, &data, &targets)?;
            let mat = p.material().index().unwrap_or(default_material);
            let name = p
                .material()
                .name()
                .or(m.name())
                .unwrap_or_default()
                .to_string();
            prims.push((mesh, mat, name));
        }
        meshes.insert(m.index(), prims);
    }
    let mut parts = Vec::new();
    for n in doc.nodes() {
        if let Some(m) = n.mesh() {
            for (mesh, material, name) in meshes.get(&m.index()).into_iter().flatten() {
                parts.push(ModelPart {
                    name: name.clone(),
                    node: n.index(),
                    mesh: mesh.clone(),
                    material: *material,
                });
            }
        }
    }

    let skins = anim
        .skeletons
        .iter()
        .map(|s| Skin {
            joints: s
                .skin_order
                .iter()
                .map(|&j| (s.joint_nodes[j], s.skeleton.joints()[j].inverse_bind))
                .collect(),
        })
        .collect::<Vec<_>>();
    let node_index = anim
        .node_names
        .iter()
        .enumerate()
        .map(|(i, n)| (n.clone(), i))
        .collect();
    let mut data = ModelData {
        nodes,
        order,
        node_names: anim.node_names,
        node_index,
        parts,
        materials,
        skins,
        clips: anim.clips.into_iter().map(Arc::new).collect(),
        bounds: Aabb::EMPTY,
    };
    data.bounds = rest_bounds(&data);
    Ok(Model {
        inner: Arc::new(data),
    })
}

/// Node indices ordered so every parent comes before its children.
fn parents_first(nodes: &[Node]) -> Vec<usize> {
    let depth = |mut n: usize| {
        let mut d = 0;
        while let Some(p) = nodes[n].parent {
            d += 1;
            n = p;
            if d > nodes.len() {
                break;
            }
        }
        d
    };
    let mut order: Vec<usize> = (0..nodes.len()).collect();
    order.sort_by_key(|&n| depth(n));
    order
}

/// Model-space matrices of every node in the rest pose.
pub(super) fn world_matrices(data: &ModelData, locals: &[Transform], out: &mut Vec<Mat4>) {
    out.clear();
    out.resize(data.nodes.len(), Mat4::IDENTITY);
    for &n in &data.order {
        let local = locals[n].to_matrix();
        out[n] = match data.nodes[n].parent {
            Some(p) => out[p] * local,
            None => local,
        };
    }
}

fn rest_bounds(data: &ModelData) -> Aabb {
    let locals: Vec<Transform> = data.nodes.iter().map(|n| n.rest).collect();
    let mut world = Vec::new();
    world_matrices(data, &locals, &mut world);
    data.parts.iter().fold(Aabb::EMPTY, |acc, p| {
        let b = p.mesh.bounds();
        if data.nodes[p.node].skin.is_some() {
            acc.union(b)
        } else {
            acc.union(b.transformed(&world[p.node]))
        }
    })
}

fn primitive(
    p: &gltf::Primitive,
    buffers: &[gltf::buffer::Data],
) -> Result<(MeshData, Vec<MorphTarget>)> {
    let reader = p.reader(|b| buffers.get(b.index()).map(|d| &d.0[..]));
    let positions: Vec<Vec3> = reader
        .read_positions()
        .ok_or_else(|| err("a primitive has no positions"))?
        .map(Vec3::from_array)
        .collect();
    let n = positions.len();
    let mut vertices: Vec<Vertex3D> = positions
        .iter()
        .map(|p| Vertex3D {
            position: *p,
            ..Vertex3D::default()
        })
        .collect();
    let normals = reader
        .read_normals()
        .map(|it| it.map(Vec3::from_array).collect::<Vec<_>>());
    if let Some(ns) = &normals {
        for (v, nrm) in vertices.iter_mut().zip(ns) {
            v.normal = *nrm;
        }
    }
    if let Some(uv) = reader.read_tex_coords(0) {
        for (v, t) in vertices.iter_mut().zip(uv.into_f32()) {
            v.uv = Vec2::from_array(t);
        }
    }
    if let Some(uv) = reader.read_tex_coords(1) {
        for (v, t) in vertices.iter_mut().zip(uv.into_f32()) {
            v.uv2 = Vec2::from_array(t);
        }
    }
    if let Some(c) = reader.read_colors(0) {
        for (v, c) in vertices.iter_mut().zip(c.into_rgba_f32()) {
            v.color = Color::from_linear(c);
        }
    }
    if let (Some(j), Some(w)) = (reader.read_joints(0), reader.read_weights(0)) {
        for ((v, j), w) in vertices.iter_mut().zip(j.into_u16()).zip(w.into_f32()) {
            let sum: f32 = w.iter().sum();
            v.joints = j;
            v.weights = if sum > 0.0 { w.map(|x| x / sum) } else { w };
        }
    }
    let indices: Vec<u32> = match reader.read_indices() {
        Some(i) => i.into_u32().collect(),
        None => (0..n as u32).collect(),
    };
    let mut data = MeshData::new(vertices, indices);
    if normals.is_none() {
        data.compute_normals();
    }
    match reader.read_tangents() {
        Some(t) => {
            for (v, t) in data.vertices.iter_mut().zip(t) {
                v.tangent = Vec4::from_array(t);
            }
        }
        None => data.compute_tangents(),
    }
    let targets = reader
        .read_morph_targets()
        .map(|(p, nrm, _)| MorphTarget {
            positions: p.map_or_else(
                || vec![Vec3::ZERO; n],
                |it| it.map(Vec3::from_array).collect(),
            ),
            normals: nrm.map_or_else(Vec::new, |it| it.map(Vec3::from_array).collect()),
        })
        .collect();
    Ok((data, targets))
}

fn uv_set(info: &gltf::texture::Info) -> u8 {
    u8::from(info.tex_coord() > 0)
}

fn material(
    gfx: &Graphics,
    doc: &gltf::Document,
    m: &gltf::Material,
    textures: &mut TextureCache,
) -> Result<Material> {
    let pbr = m.pbr_metallic_roughness();
    let mut out = Material {
        base_color: Color::from_linear(pbr.base_color_factor()),
        metallic: pbr.metallic_factor(),
        roughness: pbr.roughness_factor(),
        double_sided: m.double_sided(),
        unlit: m.unlit(),
        emissive_strength: m.emissive_strength().unwrap_or(1.0),
        ..Material::default()
    };
    let e = m.emissive_factor();
    out.emissive = Color::from_linear([e[0], e[1], e[2], 1.0]);
    out.alpha_mode = match m.alpha_mode() {
        gltf::material::AlphaMode::Opaque => AlphaMode::Opaque,
        gltf::material::AlphaMode::Mask => AlphaMode::Mask(m.alpha_cutoff().unwrap_or(0.5)),
        gltf::material::AlphaMode::Blend => AlphaMode::Blend,
    };
    let mut uv = UvSets::default();
    if let Some(info) = pbr.base_color_texture() {
        out.base_color_texture = Some(textures.get(gfx, &info.texture(), true)?);
        uv.base_color = uv_set(&info);
        if let Some(t) = info.texture_transform() {
            out.uv_transform = TextureTransform {
                offset: Vec2::from_array(t.offset()),
                rotation: t.rotation(),
                scale: Vec2::from_array(t.scale()),
            };
            if let Some(set) = t.tex_coord() {
                uv.base_color = u8::from(set > 0);
            }
        }
    }
    if let Some(info) = pbr.metallic_roughness_texture() {
        out.metallic_roughness_texture = Some(textures.get(gfx, &info.texture(), false)?);
        uv.metallic_roughness = uv_set(&info);
    }
    if let Some(n) = m.normal_texture() {
        out.normal_texture = Some(textures.get(gfx, &n.texture(), false)?);
        out.normal_scale = n.scale();
        uv.normal = u8::from(n.tex_coord() > 0);
    }
    if let Some(o) = m.occlusion_texture() {
        out.occlusion_texture = Some(textures.get(gfx, &o.texture(), false)?);
        out.occlusion_strength = o.strength();
        uv.occlusion = u8::from(o.tex_coord() > 0);
    }
    if let Some(info) = m.emissive_texture() {
        out.emissive_texture = Some(textures.get(gfx, &info.texture(), true)?);
        uv.emissive = uv_set(&info);
    }
    out.uv_sets = uv;
    super::extensions::import(gfx, doc, m, textures, &mut out)?;
    Ok(out)
}
