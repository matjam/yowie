use base64::Engine;
use serde_json::json;
use yowie_core::{Mat4, Quat, Vec3};

use super::*;
use crate::value::Value;

fn floats(out: &mut Vec<u8>, values: &[f32]) {
    for v in values {
        out.extend_from_slice(&v.to_le_bytes());
    }
}

/// A glTF with a three-node tree (root, child, body), a skin listing the
/// child before the root, and three animations covering linear, cubic
/// spline and morph weight channels.
fn sample_gltf() -> Vec<u8> {
    let mut b = Vec::new();
    // 0: inverse bind matrices for [child, root].
    floats(
        &mut b,
        &[
            1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., -1., 0., 1.,
        ],
    );
    floats(
        &mut b,
        &[
            1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
        ],
    );
    // 128: key times.
    floats(&mut b, &[0., 1.]);
    // 136: child translations.
    floats(&mut b, &[0., 1., 0., 0., 3., 0.]);
    // 160: root rotations, identity then a quarter turn about Z.
    let h = std::f32::consts::FRAC_1_SQRT_2;
    floats(&mut b, &[0., 0., 0., 1., 0., 0., h, h]);
    // 192: cubic spline scales (in, value, out per key).
    floats(&mut b, &[0.; 3]);
    floats(&mut b, &[1.; 3]);
    floats(&mut b, &[0.; 3]);
    floats(&mut b, &[0.; 3]);
    floats(&mut b, &[3.; 3]);
    floats(&mut b, &[0.; 3]);
    // 264: morph weights, two targets per key.
    floats(&mut b, &[0., 1., 1., 0.]);
    let uri = format!(
        "data:application/octet-stream;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(&b)
    );
    let view = |offset: usize, length: usize| json!({"buffer": 0, "byteOffset": offset, "byteLength": length});
    let acc = |view: usize, ty: &str, count: usize| json!({"bufferView": view, "componentType": 5126, "count": count, "type": ty});
    let doc = json!({
        "asset": {"version": "2.0"},
        "buffers": [{"byteLength": b.len(), "uri": uri}],
        "bufferViews": [view(0, 128), view(128, 8), view(136, 24), view(160, 32), view(192, 72), view(264, 16)],
        "accessors": [
            acc(0, "MAT4", 2),
            {"bufferView": 1, "componentType": 5126, "count": 2, "type": "SCALAR", "min": [0.0], "max": [1.0]},
            acc(2, "VEC3", 2), acc(3, "VEC4", 2), acc(4, "VEC3", 6), acc(5, "SCALAR", 4)
        ],
        "nodes": [
            {"name": "root", "children": [1]},
            {"name": "child", "translation": [0.0, 1.0, 0.0]},
            {"name": "body", "skin": 0, "weights": [0.0, 0.0]}
        ],
        "scenes": [{"nodes": [0, 2]}],
        "skins": [{"joints": [1, 0], "inverseBindMatrices": 0}],
        "animations": [
            {"name": "move",
             "samplers": [{"input": 1, "output": 2}, {"input": 1, "output": 3}],
             "channels": [
                {"sampler": 0, "target": {"node": 1, "path": "translation"}},
                {"sampler": 1, "target": {"node": 0, "path": "rotation"}}]},
            {"name": "grow",
             "samplers": [{"input": 1, "output": 4, "interpolation": "CUBICSPLINE"}],
             "channels": [{"sampler": 0, "target": {"node": 1, "path": "scale"}}]},
            {"name": "smile",
             "samplers": [{"input": 1, "output": 5}],
             "channels": [{"sampler": 0, "target": {"node": 2, "path": "weights"}}]}
        ]
    });
    serde_json::to_vec(&doc).unwrap()
}

#[test]
fn imports_skeleton_with_node_mapping() {
    let import = import_slice(&sample_gltf()).unwrap();
    assert_eq!(import.node_names, ["root", "child", "body"]);
    let sk = &import.skeletons[0];
    // Joints are reordered parents first even though the skin lists the child first.
    let names: Vec<_> = sk
        .skeleton
        .joints()
        .iter()
        .map(|j| j.name.as_str())
        .collect();
    assert_eq!(names, ["root", "child"]);
    assert_eq!(sk.skeleton.joints()[1].parent, Some(0));
    assert_eq!(sk.node_to_joint, [Some(0), Some(1), None]);
    assert_eq!(sk.joint_nodes, [0, 1]);
    assert_eq!(sk.skin_order, [1, 0]);
    for m in sk.skeleton.joint_matrices(&sk.skeleton.rest_pose()) {
        assert!(m.abs_diff_eq(Mat4::IDENTITY, 1e-5), "{m:?}");
    }
}

#[test]
fn imports_linear_cubic_and_weight_clips() {
    let import = import_slice(&sample_gltf()).unwrap();
    assert_eq!(import.clips.len(), 3);
    let sk = &import.skeletons[0].skeleton;

    let pose = sk.pose_from_clip(&import.clips[0], 0.5);
    assert!((pose.locals[1].translation - Vec3::new(0.0, 2.0, 0.0)).length() < 1e-5);
    assert!(
        pose.locals[0]
            .rotation
            .dot(Quat::from_rotation_z(std::f32::consts::FRAC_PI_4))
            .abs()
            > 0.9999
    );

    let grow = import.clips[1]
        .sample_transform("child", 0.5, Default::default())
        .unwrap();
    // Zero tangents between 1 and 3 give the midpoint 2.
    assert!((grow.scale - Vec3::splat(2.0)).length() < 1e-5);

    let mut seen = None;
    import.clips[2].for_each_value(0.25, |n, v| seen = Some((n.to_string(), v)));
    assert_eq!(
        seen,
        Some(("body".to_string(), Value::Weights(vec![0.25, 0.75])))
    );
}

#[test]
fn rejects_invalid_data() {
    assert!(import_slice(b"not gltf").is_err());
}

#[test]
fn imports_from_a_path() {
    let path = std::env::temp_dir().join("yowie_anim_test.gltf");
    std::fs::write(&path, sample_gltf()).unwrap();
    let import = import_path(&path).unwrap();
    std::fs::remove_file(&path).unwrap();
    assert_eq!(import.skeletons.len(), 1);
}
