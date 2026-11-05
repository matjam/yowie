//! Loads a glTF model built in memory and renders it headless: skinning
//! driven by a clip, morph targets and per-part material overrides.

use serde_json::json;
use yowie_core::{Color, Mat4, Vec3, uvec2};

use super::*;
use crate::gpu_tests::{headless, render};

/// Wraps a glTF document and its binary buffer in a GLB container.
fn glb(doc: &serde_json::Value, bin: &[u8]) -> Vec<u8> {
    let mut json = serde_json::to_vec(doc).unwrap_or_default();
    while !json.len().is_multiple_of(4) {
        json.push(b' ');
    }
    let mut bin = bin.to_vec();
    while !bin.len().is_multiple_of(4) {
        bin.push(0);
    }
    let total = 12 + 8 + json.len() + 8 + bin.len();
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&(json.len() as u32).to_le_bytes());
    out.extend_from_slice(b"JSON");
    out.extend_from_slice(&json);
    out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
    out.extend_from_slice(b"BIN\0");
    out.extend_from_slice(&bin);
    out
}

fn put<T: bytemuck::Pod>(bin: &mut Vec<u8>, values: &[T]) -> (usize, usize) {
    let start = bin.len();
    bin.extend_from_slice(bytemuck::cast_slice(values));
    (start, bin.len() - start)
}

/// A bar one unit wide and two tall standing on the origin, made of two
/// cells: the bottom bound to joint `base`, the top to joint `tip` one
/// unit up. Clip `slide` moves the tip 1 unit along +X over one second;
/// morph target 0 moves every vertex 1 unit along -X.
fn bar_model() -> Vec<u8> {
    // Two quads with their own corners: vertices 0..4 bottom, 4..8 top.
    let mut pos = Vec::new();
    for y in [0.0f32, 1.0, 1.0, 2.0] {
        for x in [-0.5f32, 0.5] {
            pos.push([x, y, 0.0f32]);
        }
    }
    let joints: Vec<[u16; 4]> = (0..8).map(|i| [u16::from(i >= 4), 0, 0, 0]).collect();
    let normals = vec![[0.0f32, 0.0, 1.0]; 8];
    let weights = vec![[1.0f32, 0.0, 0.0, 0.0]; 8];
    let indices: [u16; 12] = [0, 1, 2, 2, 1, 3, 4, 5, 6, 6, 5, 7];
    let deltas = vec![[-1.0f32, 0.0, 0.0]; 8];
    let ibm = [
        Mat4::IDENTITY.to_cols_array(),
        Mat4::from_translation(Vec3::new(0.0, -1.0, 0.0)).to_cols_array(),
    ];
    let times = [0.0f32, 1.0];
    let slide = [[0.0f32, 1.0, 0.0], [1.0, 1.0, 0.0]];
    let mut bin = Vec::new();
    let views: Vec<(usize, usize)> = vec![
        put(&mut bin, &pos),
        put(&mut bin, &normals),
        put(&mut bin, &joints),
        put(&mut bin, &weights),
        put(&mut bin, &indices),
        put(&mut bin, &deltas),
        put(&mut bin, &ibm),
        put(&mut bin, &times),
        put(&mut bin, &slide),
    ];
    let views_json: Vec<_> = views
        .iter()
        .map(|(o, l)| json!({"buffer": 0, "byteOffset": o, "byteLength": l}))
        .collect();
    let acc = |view: usize, ty: u32, kind: &str, count: usize| json!({"bufferView": view, "componentType": ty, "count": count, "type": kind});
    let mut accessors = vec![
        acc(0, 5126, "VEC3", 8),
        acc(1, 5126, "VEC3", 8),
        acc(2, 5123, "VEC4", 8),
        acc(3, 5126, "VEC4", 8),
        acc(4, 5123, "SCALAR", 12),
        acc(5, 5126, "VEC3", 8),
        acc(6, 5126, "MAT4", 2),
        acc(7, 5126, "SCALAR", 2),
        acc(8, 5126, "VEC3", 2),
    ];
    accessors[0]["min"] = json!([-0.5, 0.0, 0.0]);
    accessors[0]["max"] = json!([0.5, 2.0, 0.0]);
    accessors[5]["min"] = json!([-1.0, 0.0, 0.0]);
    accessors[5]["max"] = json!([-1.0, 0.0, 0.0]);
    accessors[7]["min"] = json!([0.0]);
    accessors[7]["max"] = json!([1.0]);
    let doc = json!({
        "asset": {"version": "2.0"},
        "extensionsUsed": ["KHR_materials_unlit"],
        "buffers": [{"byteLength": bin.len()}],
        "bufferViews": views_json,
        "accessors": accessors,
        "materials": [{
            "name": "paint",
            "pbrMetallicRoughness": {"baseColorFactor": [1.0, 0.0, 0.0, 1.0]},
            "extensions": {"KHR_materials_unlit": {}}
        }],
        "meshes": [{
            "name": "bar",
            "primitives": [{
                "attributes": {"POSITION": 0, "NORMAL": 1, "JOINTS_0": 2, "WEIGHTS_0": 3},
                "indices": 4,
                "material": 0,
                "targets": [{"POSITION": 5}]
            }],
            "weights": [0.0]
        }],
        "nodes": [
            {"name": "body", "mesh": 0, "skin": 0},
            {"name": "base", "children": [2]},
            {"name": "tip", "translation": [0.0, 1.0, 0.0]}
        ],
        "skins": [{"joints": [1, 2], "inverseBindMatrices": 6}],
        "scenes": [{"nodes": [0, 1]}],
        "scene": 0,
        "animations": [{
            "name": "slide",
            "samplers": [{"input": 7, "output": 8}],
            "channels": [{"sampler": 0, "target": {"node": 2, "path": "translation"}}]
        }]
    });
    glb(&doc, &bin)
}

/// Whether the pixel at `(x, y)` is mostly red.
fn red(img: &image::RgbaImage, x: u32, y: u32) -> bool {
    let p = img.get_pixel(x, y).0;
    p[0] > 200 && p[1] < 40
}

/// An orthographic camera seeing x and y from -4 to 4 over 64 pixels:
/// world x maps to pixel `32 + 8x`, world y to pixel `32 - 8y`.
fn camera() -> Camera3D {
    Camera3D::orthographic(Vec3::new(0.0, 0.0, 10.0), Vec3::ZERO, 4.0)
}

#[test]
fn a_gltf_clip_skins_the_mesh() {
    let Some(mut gfx) = headless(uvec2(64, 64)) else {
        return;
    };
    let model = Model::from_slice(&gfx, &bar_model()).expect("model");
    assert_eq!(model.parts().len(), 1);
    assert_eq!(model.node_index("tip"), Some(2));
    assert_eq!(model.node_parent(2), Some(1));
    assert!(model.clip("slide").is_some());
    assert!(model.parts()[0].mesh.is_skinned());
    assert_eq!(model.parts()[0].mesh.morph_target_count(), 1);
    assert!(model.materials()[0].unlit);

    let mut inst = model.instance();
    inst.update_pose();
    gfx.scene(&camera(), |s| {
        s.draw_model(&inst, Vec3::ZERO);
    });
    let rest = render(&mut gfx);
    // The top cell spans x -0.5..0.5, y 1..2: pixels 28..36, 16..24.
    assert!(red(&rest, 32, 20), "top cell at rest");
    assert!(!red(&rest, 42, 20), "nothing right of the top cell");

    assert!(inst.play("slide"));
    inst.advance(0.75);
    gfx.scene(&camera(), |s| {
        s.draw_model(&inst, Vec3::ZERO);
    });
    let slid = render(&mut gfx);
    // The tip moved 0.75 right: the top cell spans x 0.25..1.25.
    assert!(red(&slid, 38, 20), "top cell followed the tip joint");
    assert!(!red(&slid, 29, 20), "top cell left its rest place");
    assert!(red(&slid, 32, 28), "bottom cell stays with the base");
}

#[test]
fn morph_weights_and_overrides_change_the_draw() {
    let Some(mut gfx) = headless(uvec2(64, 64)) else {
        return;
    };
    let model = Model::from_slice(&gfx, &bar_model()).expect("model");
    let mut inst = model.instance();
    let body = model.node_index("body").expect("body node");
    inst.update_pose();
    inst.set_morph_weights(body, &[1.0]);
    inst.set_material_override(0, Some(Material::glowing(Color::GREEN, 1.0)));
    gfx.scene(&camera(), |s| {
        s.set_tonemap(Tonemap::None);
        s.draw_model(&inst, Vec3::ZERO);
    });
    let img = render(&mut gfx);
    // The whole bar moved 1 left and is green: x -1.5..-0.5.
    let p = img.get_pixel(24, 28).0;
    assert!(p[1] > 200 && p[0] < 40, "morphed and overridden: {p:?}");
    let q = img.get_pixel(34, 28).0;
    assert!(q[1] < 20, "rest place empty: {q:?}");
}
