//! The glTF material extensions beyond the core model:
//! KHR_materials_clearcoat, sheen, transmission, volume, ior, specular,
//! iridescence and anisotropy. Their textures read the base colour's UV
//! set and transform.

use gltf::json::Value;
use yowie_core::Color;

use super::textures::TextureCache;
use crate::Graphics;
use crate::error::Result;
use crate::scene::layers::{Anisotropy, Clearcoat, Iridescence, Sheen, Specular, Transmission};
use crate::scene::material::Material;

const R: [bool; 4] = [true, false, false, false];
const G: [bool; 4] = [false, true, false, false];
const A: [bool; 4] = [false, false, false, true];
const RGB: [bool; 4] = [true, true, true, false];

fn number(v: &Value, key: &str, default: f32) -> f32 {
    v.get(key)
        .and_then(Value::as_f64)
        .map_or(default, |n| n as f32)
}

fn linear3(v: &Value, key: &str, default: [f32; 3]) -> Color {
    let c = match v.get(key).and_then(Value::as_array) {
        Some(a) if a.len() == 3 => {
            [0, 1, 2].map(|i| a[i].as_f64().map_or(default[i], |n| n as f32))
        }
        _ => default,
    };
    Color::from_linear([c[0], c[1], c[2], 1.0])
}

/// The texture a texture info object under `key` names.
fn texture<'a>(doc: &'a gltf::Document, v: &Value, key: &str) -> Option<gltf::Texture<'a>> {
    let index = v.get(key)?.get("index")?.as_u64()?;
    doc.textures().nth(usize::try_from(index).ok()?)
}

/// Reads the extensions `m` uses into `out`.
pub(super) fn import(
    gfx: &Graphics,
    doc: &gltf::Document,
    m: &gltf::Material,
    textures: &mut TextureCache,
    out: &mut Material,
) -> Result<()> {
    if let Some(ior) = m.ior() {
        out.ior = ior;
    }
    if let Some(v) = m.extension_value("KHR_materials_clearcoat") {
        let mut c = Clearcoat::new(
            number(v, "clearcoatFactor", 0.0),
            number(v, "clearcoatRoughnessFactor", 0.0),
        );
        c.texture = textures.packed(
            gfx,
            &[
                (texture(doc, v, "clearcoatTexture"), R),
                (texture(doc, v, "clearcoatRoughnessTexture"), G),
            ],
            false,
        )?;
        if let Some(t) = texture(doc, v, "clearcoatNormalTexture") {
            c.normal_texture = Some(textures.get(gfx, &t, false)?);
            c.normal_scale = v
                .get("clearcoatNormalTexture")
                .map_or(1.0, |n| number(n, "scale", 1.0));
        }
        out.clearcoat = Some(c);
    }
    if let Some(v) = m.extension_value("KHR_materials_sheen") {
        let mut s = Sheen::new(
            linear3(v, "sheenColorFactor", [0.0; 3]),
            number(v, "sheenRoughnessFactor", 0.0),
        );
        s.texture = textures.packed(
            gfx,
            &[
                (texture(doc, v, "sheenColorTexture"), RGB),
                (texture(doc, v, "sheenRoughnessTexture"), A),
            ],
            true,
        )?;
        out.sheen = Some(s);
    }
    if let Some(t) = m.transmission() {
        let mut tr = Transmission {
            strength: t.transmission_factor(),
            ..Transmission::default()
        };
        if let Some(info) = t.transmission_texture() {
            tr.texture = Some(textures.get(gfx, &info.texture(), false)?);
        }
        if let Some(vol) = m.volume() {
            tr.thickness = vol.thickness_factor();
            tr.attenuation_color = Color::from_linear({
                let c = vol.attenuation_color();
                [c[0], c[1], c[2], 1.0]
            });
            let d = vol.attenuation_distance();
            tr.attenuation_distance = if d.is_finite() { d } else { 0.0 };
            if let Some(info) = vol.thickness_texture() {
                out.thickness_texture = Some(textures.get(gfx, &info.texture(), false)?);
            }
        }
        out.transmission = Some(tr);
    }
    if let Some(s) = m.specular() {
        let c = s.specular_color_factor();
        let mut sp = Specular {
            strength: s.specular_factor(),
            color: Color::from_linear([c[0], c[1], c[2], 1.0]),
            texture: None,
        };
        sp.texture = textures.packed(
            gfx,
            &[
                (s.specular_color_texture().map(|i| i.texture()), RGB),
                (s.specular_texture().map(|i| i.texture()), A),
            ],
            true,
        )?;
        out.specular = Some(sp);
    }
    if let Some(v) = m.extension_value("KHR_materials_iridescence") {
        let mut ir = Iridescence {
            strength: number(v, "iridescenceFactor", 0.0),
            ior: number(v, "iridescenceIor", 1.3),
            thickness_min: number(v, "iridescenceThicknessMinimum", 100.0),
            thickness_max: number(v, "iridescenceThicknessMaximum", 400.0),
            texture: None,
        };
        ir.texture = textures.packed(
            gfx,
            &[
                (texture(doc, v, "iridescenceTexture"), R),
                (texture(doc, v, "iridescenceThicknessTexture"), G),
            ],
            false,
        )?;
        out.iridescence = Some(ir);
    }
    if let Some(v) = m.extension_value("KHR_materials_anisotropy") {
        let mut an = Anisotropy {
            strength: number(v, "anisotropyStrength", 0.0),
            rotation: number(v, "anisotropyRotation", 0.0),
            texture: None,
        };
        if let Some(t) = texture(doc, v, "anisotropyTexture") {
            an.texture = Some(textures.get(gfx, &t, false)?);
        }
        out.anisotropy = Some(an);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn factors_fall_back_to_the_spec_defaults() {
        let v: Value = gltf::json::deserialize::from_str(
            r#"{"clearcoatFactor": 0.5, "sheenColorFactor": [1.0, 0.0, 0.0]}"#,
        )
        .unwrap_or(Value::Null);
        assert_eq!(number(&v, "clearcoatFactor", 0.0), 0.5);
        assert_eq!(number(&v, "clearcoatRoughnessFactor", 0.25), 0.25);
        let c = linear3(&v, "sheenColorFactor", [0.0; 3]).to_linear();
        assert!((c[0] - 1.0).abs() < 1e-3 && c[1] < 1e-3);
        let d = linear3(&v, "missing", [0.0; 3]).to_linear();
        assert!(d[0] < 1e-3);
    }
}
