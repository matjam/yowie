//! A glTF model built in code: a tentacle skinned to a chain of joints,
//! with a glowing bulb on its tip, a morph target that swells its middle,
//! and two animation clips ("wave" and "coil"). The 3D examples load it
//! with `Model::from_slice` so they need no asset files.

#![allow(dead_code)]

use std::f32::consts::TAU;

use serde_json::{Value, json};
use yowie::{Mat4, Quat, Vec3};

/// Joints along the tentacle.
pub const SEGMENTS: usize = 8;
/// Length of one segment in world units.
pub const SEG_LEN: f32 = 0.35;
const SIDES: usize = 14;

/// Wraps a glTF document and its binary buffer in a GLB container.
pub fn glb(doc: &Value, bin: &[u8]) -> Vec<u8> {
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

/// The binary buffer and the buffer views and accessors that describe it.
#[derive(Default)]
struct Buffers {
    bin: Vec<u8>,
    views: Vec<Value>,
    accessors: Vec<Value>,
}

/// Numbers glTF stores little-endian in the binary buffer.
trait Component: Copy {
    /// The glTF component type: 5126 for float, 5123 for unsigned short.
    const CODE: u32;
    fn put(self, out: &mut Vec<u8>);
}

impl Component for f32 {
    const CODE: u32 = 5126;
    fn put(self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.to_le_bytes());
    }
}

impl Component for u16 {
    const CODE: u32 = 5123;
    fn put(self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.to_le_bytes());
    }
}

impl Buffers {
    /// Appends `data` as a buffer view with one accessor of `count`
    /// elements over it and returns the accessor's index. `kind` is the
    /// glTF accessor type.
    fn add<T: Component>(&mut self, data: &[T], kind: &str, count: usize) -> usize {
        while !self.bin.len().is_multiple_of(4) {
            self.bin.push(0);
        }
        let start = self.bin.len();
        for v in data {
            v.put(&mut self.bin);
        }
        let bytes = self.bin.len() - start;
        self.views
            .push(json!({"buffer": 0, "byteOffset": start, "byteLength": bytes}));
        self.accessors.push(json!({
            "bufferView": self.views.len() - 1,
            "componentType": T::CODE,
            "count": count,
            "type": kind,
        }));
        self.accessors.len() - 1
    }

    /// Adds a float accessor and records its bounds, which glTF requires
    /// for positions and key times.
    fn add_bounded(&mut self, data: &[f32], kind: &str, width: usize) -> usize {
        let count = data.len() / width;
        let a = self.add(data, kind, count);
        let mut lo = vec![f32::MAX; width];
        let mut hi = vec![f32::MIN; width];
        for chunk in data.chunks(width) {
            for (i, v) in chunk.iter().enumerate() {
                lo[i] = lo[i].min(*v);
                hi[i] = hi[i].max(*v);
            }
        }
        self.accessors[a]["min"] = json!(lo);
        self.accessors[a]["max"] = json!(hi);
        a
    }
}

/// The tentacle tube's vertex streams, flattened, and the swelling morph
/// target's position deltas.
struct Tube {
    pos: Vec<f32>,
    nrm: Vec<f32>,
    joints: Vec<u16>,
    weights: Vec<f32>,
    idx: Vec<u16>,
    swell: Vec<f32>,
}

fn tube() -> Tube {
    let (mut pos, mut nrm, mut joints, mut weights, mut idx, mut swell) = (
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
    );
    for k in 0..=SEGMENTS {
        let y = k as f32 * SEG_LEN;
        let radius = 0.3 * (1.0 - k as f32 / (SEGMENTS as f32 + 1.5));
        // A ring at joint k bends half with the segment below and half
        // with the one above, which keeps the bend smooth.
        let (j0, j1, w0) = match k {
            0 => (0, 0, 1.0),
            k if k >= SEGMENTS => (SEGMENTS - 1, SEGMENTS - 1, 1.0),
            k => (k, k - 1, 0.5),
        };
        let bump = (k as f32 / SEGMENTS as f32 * std::f32::consts::PI).sin();
        for s in 0..SIDES {
            let a = s as f32 / SIDES as f32 * TAU;
            let (nx, nz) = (a.sin(), a.cos());
            pos.extend_from_slice(&[nx * radius, y, nz * radius]);
            nrm.extend_from_slice(&[nx, 0.15, nz]);
            joints.extend_from_slice(&[j0 as u16, j1 as u16, 0, 0]);
            weights.extend_from_slice(&[w0, 1.0 - w0, 0.0, 0.0]);
            swell.extend_from_slice(&[nx * 0.14 * bump, 0.0, nz * 0.14 * bump]);
        }
    }
    for k in 0..SEGMENTS {
        for s in 0..SIDES {
            let a = (k * SIDES + s) as u16;
            let b = (k * SIDES + (s + 1) % SIDES) as u16;
            let (c, d) = (a + SIDES as u16, b + SIDES as u16);
            idx.extend_from_slice(&[a, b, c, b, d, c]);
        }
    }
    // A tip vertex closes the top ring.
    let tip = (pos.len() / 3) as u16;
    pos.extend_from_slice(&[0.0, SEGMENTS as f32 * SEG_LEN + 0.08, 0.0]);
    nrm.extend_from_slice(&[0.0, 1.0, 0.0]);
    joints.extend_from_slice(&[SEGMENTS as u16 - 1, 0, 0, 0]);
    weights.extend_from_slice(&[1.0, 0.0, 0.0, 0.0]);
    swell.extend_from_slice(&[0.0, 0.0, 0.0]);
    let top = (SEGMENTS * SIDES) as u16;
    for s in 0..SIDES as u16 {
        idx.extend_from_slice(&[top + s, top + (s + 1) % SIDES as u16, tip]);
    }
    Tube {
        pos,
        nrm,
        joints,
        weights,
        idx,
        swell,
    }
}

/// A small octahedron for the bulb on the tip.
fn bulb() -> (Vec<f32>, Vec<f32>, Vec<u16>) {
    let r = 0.12;
    let p = [
        [r, 0.0, 0.0],
        [-r, 0.0, 0.0],
        [0.0, r, 0.0],
        [0.0, -r, 0.0],
        [0.0, 0.0, r],
        [0.0, 0.0, -r],
    ];
    let pos: Vec<f32> = p.iter().flatten().copied().collect();
    let nrm: Vec<f32> = p.iter().flatten().map(|v| v / r).collect();
    let idx = vec![
        0, 2, 4, 4, 2, 1, 1, 2, 5, 5, 2, 0, 4, 3, 0, 1, 3, 4, 5, 3, 1, 0, 3, 5,
    ];
    (pos, nrm, idx)
}

/// One joint's local rotation in clip `name` at time `t` seconds.
fn joint_rotation(name: &str, joint: usize, t: f32) -> Quat {
    let j = joint as f32;
    match name {
        "wave" => {
            let phase = t / 2.0 * TAU + j * 0.6;
            Quat::from_rotation_z(0.32 * phase.sin()) * Quat::from_rotation_x(0.18 * phase.cos())
        }
        _ => {
            let curl = (t / 3.0 * TAU).sin() * 0.5 + 0.5;
            Quat::from_rotation_x(0.38 * curl * (0.4 + j / SEGMENTS as f32))
        }
    }
}

/// Adds one animation: a rotation channel per joint and the swell
/// weight on the body node.
fn animation(buf: &mut Buffers, name: &str, seconds: f32, joint_nodes: &[usize]) -> Value {
    const KEYS: usize = 25;
    let times: Vec<f32> = (0..KEYS)
        .map(|i| i as f32 / (KEYS - 1) as f32 * seconds)
        .collect();
    let input = buf.add_bounded(&times, "SCALAR", 1);
    let mut samplers = Vec::new();
    let mut channels = Vec::new();
    for (j, node) in joint_nodes.iter().enumerate() {
        let rot: Vec<f32> = times
            .iter()
            .flat_map(|t| joint_rotation(name, j, *t).to_array())
            .collect();
        let output = buf.add(&rot, "VEC4", KEYS);
        samplers.push(json!({"input": input, "output": output}));
        channels.push(
            json!({"sampler": samplers.len() - 1, "target": {"node": node, "path": "rotation"}}),
        );
    }
    let weights: Vec<f32> = times
        .iter()
        .map(|t| 0.5 + 0.5 * (t / seconds * TAU).sin())
        .collect();
    let output = buf.add(&weights, "SCALAR", KEYS);
    samplers.push(json!({"input": input, "output": output}));
    channels.push(json!({"sampler": samplers.len() - 1, "target": {"node": 0, "path": "weights"}}));
    json!({"name": name, "samplers": samplers, "channels": channels})
}

/// The creature as GLB bytes. `color` is the body's linear base colour.
pub fn creature(color: [f32; 3]) -> Vec<u8> {
    let mut buf = Buffers::default();
    let Tube {
        pos,
        nrm,
        joints,
        weights,
        idx,
        swell,
    } = tube();
    let n = pos.len() / 3;
    let a_pos = buf.add_bounded(&pos, "VEC3", 3);
    let a_nrm = buf.add(&nrm, "VEC3", n);
    let a_joints = buf.add(&joints, "VEC4", n);
    let a_weights = buf.add(&weights, "VEC4", n);
    let a_idx = buf.add(&idx, "SCALAR", idx.len());
    let a_swell = buf.add_bounded(&swell, "VEC3", 3);
    let (bpos, bnrm, bidx) = bulb();
    let b_pos = buf.add_bounded(&bpos, "VEC3", 3);
    let b_nrm = buf.add(&bnrm, "VEC3", bpos.len() / 3);
    let b_idx = buf.add(&bidx, "SCALAR", bidx.len());
    let ibm: Vec<f32> = (0..SEGMENTS)
        .flat_map(|i| {
            Mat4::from_translation(Vec3::new(0.0, -(i as f32) * SEG_LEN, 0.0)).to_cols_array()
        })
        .collect();
    let a_ibm = buf.add(&ibm, "MAT4", SEGMENTS);

    // Node 0 is the skinned body, node 1 the root joint, nodes 1..=SEGMENTS
    // the joint chain, and the last node the bulb on the final joint.
    let joint_nodes: Vec<usize> = (1..=SEGMENTS).collect();
    let bulb_node = SEGMENTS + 1;
    let mut nodes = vec![json!({"name": "body", "mesh": 0, "skin": 0})];
    for (i, _) in joint_nodes.iter().enumerate() {
        let mut node = json!({"name": format!("joint{i}")});
        if i > 0 {
            node["translation"] = json!([0.0, SEG_LEN, 0.0]);
        }
        let child = if i + 1 < SEGMENTS { i + 2 } else { bulb_node };
        node["children"] = json!([child]);
        nodes.push(node);
    }
    nodes.push(json!({"name": "bulb", "mesh": 1, "translation": [0.0, SEG_LEN + 0.1, 0.0]}));
    let animations = vec![
        animation(&mut buf, "wave", 2.0, &joint_nodes),
        animation(&mut buf, "coil", 3.0, &joint_nodes),
    ];
    let doc = json!({
        "asset": {"version": "2.0", "generator": "yowie examples"},
        "extensionsUsed": ["KHR_materials_emissive_strength"],
        "buffers": [{"byteLength": buf.bin.len()}],
        "bufferViews": buf.views,
        "accessors": buf.accessors,
        "materials": [
            {
                "name": "skin",
                "pbrMetallicRoughness": {
                    "baseColorFactor": [color[0], color[1], color[2], 1.0],
                    "metallicFactor": 0.0,
                    "roughnessFactor": 0.45
                }
            },
            {
                "name": "glow",
                "pbrMetallicRoughness": {"baseColorFactor": [1.0, 0.9, 0.6, 1.0]},
                "emissiveFactor": [1.0, 0.7, 0.3],
                "extensions": {"KHR_materials_emissive_strength": {"emissiveStrength": 4.0}}
            }
        ],
        "meshes": [
            {
                "name": "tentacle",
                "primitives": [{
                    "attributes": {
                        "POSITION": a_pos, "NORMAL": a_nrm,
                        "JOINTS_0": a_joints, "WEIGHTS_0": a_weights
                    },
                    "indices": a_idx,
                    "material": 0,
                    "targets": [{"POSITION": a_swell}]
                }],
                "weights": [0.0]
            },
            {
                "name": "bulb",
                "primitives": [{
                    "attributes": {"POSITION": b_pos, "NORMAL": b_nrm},
                    "indices": b_idx,
                    "material": 1
                }]
            }
        ],
        "nodes": nodes,
        "skins": [{"joints": joint_nodes, "inverseBindMatrices": a_ibm}],
        "scenes": [{"nodes": [0, 1]}],
        "scene": 0,
        "animations": animations
    });
    glb(&doc, &buf.bin)
}
