//! Renders outlines, x-ray tints, stencil masks and game surface shaders
//! headless and checks the pixels, and loads the glTF layer extensions.
//! Every test skips with a printed message when no GPU adapter exists.

use yowie_core::{Color, Mat4, Vec3, uvec2};

use super::gpu_tests_materials::{SIZE, dark, facing, front_camera, luma, mesh, px, rgb, unlit};
use super::*;
use crate::gpu_tests::{headless, render};

#[test]
fn outline_pixels_surround_the_object() {
    let Some(mut gfx) = headless(uvec2(SIZE, SIZE)) else {
        return;
    };
    let sphere = mesh(&gfx, &MeshData::sphere(1.0, 32, 64));
    gfx.scene(&front_camera(), |s| {
        dark(s);
        let m = Material {
            outline: Some(Outline::new(4.0, Color::GREEN)),
            ..unlit(Color::RED)
        };
        s.draw(&sphere, &m, Vec3::ZERO);
    });
    let img = render(&mut gfx);
    // The silhouette is about 31 pixels from the centre.
    let c = SIZE / 2;
    for (x, y) in [(c + 33, c), (c - 33, c), (c, c + 33), (c, c - 33)] {
        let p = rgb(&img, x, y);
        assert!(p[1] > 150.0 && p[0] < 60.0, "outline at ({x}, {y}): {p:?}");
    }
    let inside = rgb(&img, c, c);
    assert!(inside[0] > 200.0 && inside[1] < 30.0, "object: {inside:?}");
    let away = rgb(&img, c + 42, c);
    assert!(away.iter().all(|v| *v < 5.0), "beyond the line: {away:?}");
}

#[test]
fn xray_tints_an_object_through_a_wall() {
    let Some(mut gfx) = headless(uvec2(SIZE, SIZE)) else {
        return;
    };
    let cube = mesh(&gfx, &MeshData::cube(1.0));
    let sphere = mesh(&gfx, &MeshData::sphere(0.8, 24, 48));
    let mut draw = |xray: bool| {
        gfx.scene(&front_camera(), |s| {
            dark(s);
            let wall = Mat4::from_translation(Vec3::new(0.0, 0.0, 1.5))
                * Mat4::from_scale(Vec3::new(4.0, 4.0, 0.2));
            s.draw(&cube, &unlit(Color::GRAY), wall);
            let mut m = unlit(Color::RED);
            if xray {
                m.xray = Some(Color::CYAN);
            }
            s.draw(&sphere, &m, Vec3::ZERO);
        });
        render(&mut gfx)
    };
    let plain = draw(false);
    let seen = draw(true);
    let c = SIZE / 2;
    let (p, q) = (rgb(&plain, c, c), rgb(&seen, c, c));
    assert!(q[1] > p[1] + 40.0 && q[2] > p[2] + 40.0, "{p:?} -> {q:?}");
    assert!(q[0] <= p[0] + 2.0, "cyan adds no red: {p:?} -> {q:?}");
    let (wx, wy) = (6, 6);
    assert!((luma(&seen, wx, wy) - luma(&plain, wx, wy)).abs() < 2.0);
}

#[test]
fn stencil_mask_limits_drawing() {
    let Some(mut gfx) = headless(uvec2(SIZE, SIZE)) else {
        return;
    };
    let quad = mesh(&gfx, &MeshData::plane(1.0, 1));
    let cam = front_camera();
    let mut draw = |masked: bool| {
        gfx.scene(&cam, |s| {
            dark(s);
            let marker = Material {
                stencil: Stencil::mark(1),
                ..unlit(Color::GREEN)
            };
            s.draw(&quad, &marker, facing(Vec3::ZERO));
            let mut big = unlit(Color::RED);
            if masked {
                big.stencil = Stencil::inside(1);
            }
            let at = Mat4::from_translation(Vec3::new(0.0, 0.0, 0.5))
                * Mat4::from_scale(Vec3::splat(3.0));
            s.draw(&quad, &big, at * facing(Vec3::ZERO));
        });
        render(&mut gfx)
    };
    let free = draw(false);
    let masked = draw(true);
    let c = SIZE / 2;
    let centre = rgb(&masked, c, c);
    assert!(
        centre[0] > 200.0 && centre[1] < 30.0,
        "inside the mask: {centre:?}"
    );
    let (ox, oy) = px(&cam, Vec3::new(1.0, 0.0, 0.5));
    let free_out = rgb(&free, ox, oy);
    assert!(free_out[0] > 200.0, "unmasked covers it: {free_out:?}");
    let out = rgb(&masked, ox, oy);
    assert!(out.iter().all(|v| *v < 5.0), "masked leaves it: {out:?}");
}

#[test]
fn game_surface_shader_colour_is_lit() {
    let Some(mut gfx) = headless(uvec2(SIZE, SIZE)) else {
        return;
    };
    let shader = gfx
        .compile_surface_shader(
            "fn material(s: Surface) -> Surface {\n    var o = s;\n    o.base = vec3<f32>(0.0, 1.0, 0.0);\n    return o;\n}\n",
        )
        .expect("shader");
    let sphere = mesh(&gfx, &MeshData::sphere(1.5, 24, 48));
    gfx.scene(&front_camera(), |s| {
        dark(s);
        s.set_sun(DirectionalLight::new(-Vec3::X, Color::WHITE, 2.0));
        let m = Material {
            shader: Some(shader.clone()),
            ..Material::color(Color::WHITE, 0.0, 0.8)
        };
        s.draw(&sphere, &m, Vec3::ZERO);
    });
    let img = render(&mut gfx);
    // Green diffuse light under the white dielectric reflection.
    let lit = rgb(&img, 100, 64);
    assert!(
        lit[1] > 100.0 && lit[1] > lit[0] * 3.0 && (lit[0] - lit[2]).abs() < 3.0,
        "lit side is green: {lit:?}"
    );
    let shade = rgb(&img, 30, 64);
    assert!(shade[1] < 10.0, "far side is dark: {shade:?}");
}

#[test]
fn surface_shader_errors_name_the_game_line() {
    let Some(gfx) = headless(uvec2(8, 8)) else {
        return;
    };
    let err = gfx
        .compile_surface_shader("fn material(s: Surface) -> Surface {\n    return nope;\n}\n")
        .expect_err("bad shader");
    let msg = err.to_string();
    assert!(msg.contains("line 2,"), "line 2 of the game source: {msg}");
}

#[test]
fn gltf_layer_extensions_load() {
    let Some(gfx) = headless(uvec2(8, 8)) else {
        return;
    };
    let doc = serde_json::json!({
        "asset": {"version": "2.0"},
        "extensionsUsed": [
            "KHR_materials_clearcoat", "KHR_materials_sheen", "KHR_materials_transmission",
            "KHR_materials_volume", "KHR_materials_ior", "KHR_materials_specular",
            "KHR_materials_iridescence", "KHR_materials_anisotropy"
        ],
        "materials": [{
            "extensions": {
                "KHR_materials_clearcoat": {"clearcoatFactor": 0.8, "clearcoatRoughnessFactor": 0.2},
                "KHR_materials_sheen": {"sheenColorFactor": [1.0, 0.0, 0.0], "sheenRoughnessFactor": 0.4},
                "KHR_materials_transmission": {"transmissionFactor": 0.9},
                "KHR_materials_volume": {
                    "thicknessFactor": 0.5, "attenuationDistance": 2.0,
                    "attenuationColor": [1.0, 0.5, 0.5]
                },
                "KHR_materials_ior": {"ior": 1.4},
                "KHR_materials_specular": {"specularFactor": 0.7},
                "KHR_materials_iridescence": {"iridescenceFactor": 1.0, "iridescenceThicknessMaximum": 500.0},
                "KHR_materials_anisotropy": {"anisotropyStrength": 0.6, "anisotropyRotation": 0.3}
            }
        }]
    });
    let bytes = serde_json::to_vec(&doc).unwrap_or_default();
    let model = Model::from_slice(&gfx, &bytes).expect("model");
    let m = &model.materials()[0];
    let near = |a: f32, b: f32| (a - b).abs() < 1e-4;
    let cc = m.clearcoat.as_ref().expect("clearcoat");
    assert!(near(cc.strength, 0.8) && near(cc.roughness, 0.2));
    let sh = m.sheen.as_ref().expect("sheen");
    assert!(near(sh.roughness, 0.4) && sh.color.to_linear()[1] < 1e-3);
    let tr = m.transmission.as_ref().expect("transmission");
    assert!(near(tr.strength, 0.9) && near(tr.thickness, 0.5));
    assert!(near(tr.attenuation_distance, 2.0));
    assert!(near(m.ior, 1.4));
    assert!(near(m.specular.as_ref().expect("specular").strength, 0.7));
    let ir = m.iridescence.as_ref().expect("iridescence");
    assert!(near(ir.strength, 1.0) && near(ir.thickness_max, 500.0) && near(ir.ior, 1.3));
    let an = m.anisotropy.as_ref().expect("anisotropy");
    assert!(near(an.strength, 0.6) && near(an.rotation, 0.3));
}
