//! The layered material features (clearcoat, sheen, transmission,
//! specular, iridescence, anisotropy, fur, outlines, x-ray) and how they
//! pack into the material record's `ext` vectors.
//!
//! Each feature is an `Option` on [`Material`]; `None` switches it off and
//! costs nothing. The glTF extension each one follows is named on it.

use yowie_core::Color;

use super::material::{Material, MaterialGpu};
use crate::texture::Texture;

/// A clear varnish over the base (KHR_materials_clearcoat): car paint,
/// lacquer, wet surfaces. The coat has its own roughness and its own
/// normal: the mesh's normal, or `normal_texture` when set, never the base
/// normal map.
#[derive(Clone, Debug, PartialEq)]
pub struct Clearcoat {
    /// How much of the coat there is, 0 to 1.
    pub strength: f32,
    /// The coat's roughness, 0 to 1.
    pub roughness: f32,
    /// Strength in red and roughness in green, multiplying the factors;
    /// data, not colour.
    pub texture: Option<Texture>,
    /// A tangent-space normal map for the coat alone; data.
    pub normal_texture: Option<Texture>,
    /// Scales the coat normal map's X and Y.
    pub normal_scale: f32,
}

impl Clearcoat {
    /// A full coat of the given roughness.
    pub fn new(strength: f32, roughness: f32) -> Self {
        Self {
            strength,
            roughness,
            ..Self::default()
        }
    }
}

impl Default for Clearcoat {
    fn default() -> Self {
        Self {
            strength: 1.0,
            roughness: 0.05,
            texture: None,
            normal_texture: None,
            normal_scale: 1.0,
        }
    }
}

/// Soft light scattered back at grazing angles (KHR_materials_sheen, the
/// Charlie distribution): velvet, felt, cloth. A sheen colour near the
/// base colour with a roughness around 0.4 is a good start.
#[derive(Clone, Debug, PartialEq)]
pub struct Sheen {
    /// The sheen's colour, sRGB.
    pub color: Color,
    /// How spread out the sheen is, 0 to 1.
    pub roughness: f32,
    /// Colour in RGB (sRGB) and roughness in alpha, multiplying the
    /// factors. Create it with `premultiplied` set so the upload leaves
    /// the colour alone.
    pub texture: Option<Texture>,
}

impl Sheen {
    /// A sheen of `color` and `roughness`.
    pub fn new(color: Color, roughness: f32) -> Self {
        Self {
            color,
            roughness,
            texture: None,
        }
    }
}

/// Light passing through the surface (KHR_materials_transmission and
/// KHR_materials_volume): glass, water, ice. The opaque scene shows
/// through, refracted by the material's `ior` across `thickness` world
/// units, blurred by the roughness, tinted by the base colour and absorbed
/// towards `attenuation_color` over `attenuation_distance`. Transmissive
/// meshes draw after the opaque scene, which is all they show; other
/// glass and blended meshes behind them do not appear through them.
#[derive(Clone, Debug, PartialEq)]
pub struct Transmission {
    /// How much light passes through, 0 to 1.
    pub strength: f32,
    /// Multiplies the strength by its red channel; data.
    pub texture: Option<Texture>,
    /// How far light travels inside, in world units; 0 is a thin wall
    /// that does not bend light.
    pub thickness: f32,
    /// What white light becomes after `attenuation_distance` inside, sRGB.
    pub attenuation_color: Color,
    /// The distance in world units over which light takes the attenuation
    /// colour; 0 means no absorption.
    pub attenuation_distance: f32,
}

impl Transmission {
    /// Clear glass `thickness` units through.
    pub fn glass(thickness: f32) -> Self {
        Self {
            thickness,
            ..Self::default()
        }
    }
}

impl Default for Transmission {
    fn default() -> Self {
        Self {
            strength: 1.0,
            texture: None,
            thickness: 0.0,
            attenuation_color: Color::WHITE,
            attenuation_distance: 0.0,
        }
    }
}

/// How strongly and in what colour a dielectric reflects
/// (KHR_materials_specular). Metals keep their own reflection.
#[derive(Clone, Debug, PartialEq)]
pub struct Specular {
    /// Scales the reflection, 0 to 1; 0.02 is chalk.
    pub strength: f32,
    /// Tints the reflection at normal incidence, sRGB.
    pub color: Color,
    /// Colour in RGB (sRGB) and strength in alpha, multiplying the
    /// factors. Create it with `premultiplied` set.
    pub texture: Option<Texture>,
}

impl Specular {
    /// A reflection tinted `color`.
    pub fn tint(color: Color) -> Self {
        Self {
            strength: 1.0,
            color,
            texture: None,
        }
    }
}

impl Default for Specular {
    fn default() -> Self {
        Self::tint(Color::WHITE)
    }
}

/// A thin film over the surface whose interference turns the reflection's
/// hue with the viewing angle (KHR_materials_iridescence): soap bubbles,
/// oil, beetle shells, tempered steel.
#[derive(Clone, Debug, PartialEq)]
pub struct Iridescence {
    /// How much of the film there is, 0 to 1.
    pub strength: f32,
    /// The film's index of refraction.
    pub ior: f32,
    /// The film's thickness in nanometres where the texture's green is 0.
    pub thickness_min: f32,
    /// The film's thickness in nanometres where the texture's green is 1,
    /// and everywhere without a texture; 100 to 800 shows colour.
    pub thickness_max: f32,
    /// Strength in red, thickness between the two in green; data.
    pub texture: Option<Texture>,
}

impl Iridescence {
    /// A full film `thickness` nanometres thick.
    pub fn new(thickness: f32) -> Self {
        Self {
            thickness_max: thickness,
            ..Self::default()
        }
    }
}

impl Default for Iridescence {
    fn default() -> Self {
        Self {
            strength: 1.0,
            ior: 1.3,
            thickness_min: 100.0,
            thickness_max: 400.0,
            texture: None,
        }
    }
}

/// A highlight stretched along the surface (KHR_materials_anisotropy):
/// brushed metal, hair, satin. The direction follows the mesh's tangents
/// (texture U), turned by `rotation`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Anisotropy {
    /// How much the highlight stretches, -1 to 1; negative values stretch
    /// it across the tangent instead of along it.
    pub strength: f32,
    /// Turns the direction about the normal, in radians.
    pub rotation: f32,
    /// Direction in red and green (around 0.5, in tangent space) and
    /// strength in blue; data.
    pub texture: Option<Texture>,
}

impl Anisotropy {
    /// A highlight stretched by `strength` along the tangents.
    pub fn new(strength: f32) -> Self {
        Self {
            strength,
            ..Self::default()
        }
    }
}

/// Fur, grass or moss: the mesh drawn again as shells, each further out
/// along its normals. A shell keeps a fragment where the strand mask is
/// above the shell's height, so strands thin towards their tips. Shells
/// cast no shadow and draw only for opaque and cut-out materials.
#[derive(Clone, Debug, PartialEq)]
pub struct Fur {
    /// The number of shells, 1 to 64; 8 to 24 look like fur. Each costs
    /// an instance of the mesh in the same draw call.
    pub shells: u32,
    /// How far the outermost shell stands off, in world units.
    pub length: f32,
    /// The strand mask in red (each texel the height its strand reaches,
    /// 0 to 1), tiled by the material's texture transform; data. Without
    /// one the engine uses a hash of a 64 by 64 grid.
    pub texture: Option<Texture>,
}

impl Fur {
    /// `shells` shells reaching `length` units out.
    pub fn new(shells: u32, length: f32) -> Self {
        Self {
            shells,
            length,
            texture: None,
        }
    }
}

/// A line drawn around a mesh's silhouette: selection rings, cartoon
/// edges. It is hidden where other geometry stands in front, and draws for
/// opaque and cut-out materials.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Outline {
    /// The line's width in pixels.
    pub width: f32,
    /// The line's colour, sRGB; alpha fades it.
    pub color: Color,
}

impl Outline {
    /// A line `width` pixels wide in `color`.
    pub fn new(width: f32, color: Color) -> Self {
        Self { width, color }
    }
}

/// Bits of `MaterialGpu::flags[2]`, one per feature.
pub const EXT_CLEARCOAT: u32 = 1;
/// Set when [`Material::sheen`] is on.
pub const EXT_SHEEN: u32 = 2;
/// Set when [`Material::transmission`] is on.
pub const EXT_TRANSMISSION: u32 = 4;
/// Set when [`Material::iridescence`] is on.
pub const EXT_IRIDESCENCE: u32 = 8;
/// Set when [`Material::anisotropy`] is on.
pub const EXT_ANISOTROPY: u32 = 16;
/// Set when [`Material::specular`] is on or the index of refraction is not
/// 1.5.
pub const EXT_SPECULAR: u32 = 32;
/// Set when [`Material::subsurface`] is above zero.
pub const EXT_SUBSURFACE: u32 = 64;
/// Set when [`Material::fur`] is on.
pub const EXT_FUR: u32 = 128;
/// Set when [`Material::outline`] is on.
pub const EXT_OUTLINE: u32 = 256;
/// Set when [`Material::xray`] is on.
pub const EXT_XRAY: u32 = 512;

fn rgb(c: Color) -> [f32; 3] {
    let l = c.to_linear();
    [l[0], l[1], l[2]]
}

/// The textures of the layered features in binding order (after the five
/// core textures): clearcoat, clearcoat normal, sheen, specular,
/// transmission, thickness, iridescence, anisotropy, fur.
pub(crate) fn layer_textures(m: &Material) -> [Option<&Texture>; 9] {
    let cc = m.clearcoat.as_ref();
    [
        cc.and_then(|c| c.texture.as_ref()),
        cc.and_then(|c| c.normal_texture.as_ref()),
        m.sheen.as_ref().and_then(|s| s.texture.as_ref()),
        m.specular.as_ref().and_then(|s| s.texture.as_ref()),
        m.transmission.as_ref().and_then(|t| t.texture.as_ref()),
        m.thickness_texture.as_ref(),
        m.iridescence.as_ref().and_then(|i| i.texture.as_ref()),
        m.anisotropy.as_ref().and_then(|a| a.texture.as_ref()),
        m.fur.as_ref().and_then(|f| f.texture.as_ref()),
    ]
}

/// Writes the layered features into `g.ext`, their flag bits into
/// `g.flags[2]` and which feature textures are present into `g.flags[3]`.
pub(crate) fn pack(m: &Material, g: &mut MaterialGpu) {
    let mut ext = 0u32;
    let e = &mut g.ext;
    if let Some(c) = &m.clearcoat {
        ext |= EXT_CLEARCOAT;
        e[0][0] = c.strength.clamp(0.0, 1.0);
        e[0][1] = c.roughness.clamp(0.0, 1.0);
        e[0][2] = c.normal_scale;
    }
    if let Some(s) = &m.sheen {
        ext |= EXT_SHEEN;
        e[0][3] = s.roughness.clamp(0.0, 1.0);
        let [r, gr, b] = rgb(s.color);
        e[1][0] = r;
        e[1][1] = gr;
        e[1][2] = b;
    }
    if m.subsurface > 0.0 {
        ext |= EXT_SUBSURFACE;
        e[1][3] = m.subsurface.min(1.0);
    }
    let custom_ior = m.ior != 1.5;
    if m.specular.is_some() || custom_ior {
        ext |= EXT_SPECULAR;
    }
    if let Some(t) = &m.transmission {
        ext |= EXT_TRANSMISSION;
        e[2][0] = t.strength.clamp(0.0, 1.0);
        e[2][2] = t.thickness.max(0.0);
        e[2][3] = t.attenuation_distance.max(0.0);
        let [r, gr, b] = rgb(t.attenuation_color);
        e[3][0] = r;
        e[3][1] = gr;
        e[3][2] = b;
    }
    if ext & (EXT_SPECULAR | EXT_TRANSMISSION) != 0 {
        e[2][1] = m.ior.max(1.0);
    }
    if ext & EXT_SPECULAR != 0 {
        let s = m.specular.clone().unwrap_or_default();
        e[3][3] = s.strength.clamp(0.0, 1.0);
        let [r, gr, b] = rgb(s.color);
        e[4][0] = r;
        e[4][1] = gr;
        e[4][2] = b;
    }
    if let Some(i) = &m.iridescence {
        ext |= EXT_IRIDESCENCE;
        e[4][3] = i.strength.clamp(0.0, 1.0);
        e[5][0] = i.ior.max(1.0);
        e[5][1] = i.thickness_min.max(0.0);
        e[5][2] = i.thickness_max.max(0.0);
    }
    if let Some(a) = &m.anisotropy {
        ext |= EXT_ANISOTROPY;
        e[5][3] = a.strength.clamp(-1.0, 1.0);
        e[6][0] = a.rotation;
    }
    if let Some(f) = &m.fur {
        ext |= EXT_FUR;
        e[6][1] = f.shells.clamp(1, 64) as f32;
        e[6][2] = f.length.max(0.0);
    }
    if let Some(o) = &m.outline {
        ext |= EXT_OUTLINE;
        e[6][3] = o.width.max(0.0);
        e[7] = o.color.to_linear();
    }
    if let Some(x) = &m.xray {
        ext |= EXT_XRAY;
        e[8] = x.to_linear();
    }
    g.flags[2] = ext;
    g.flags[3] = layer_textures(m)
        .iter()
        .enumerate()
        .fold(0u32, |acc, (i, t)| acc | (u32::from(t.is_some()) << i));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn features_set_their_bits_and_vectors() {
        let m = Material {
            clearcoat: Some(Clearcoat::new(1.0, 0.1)),
            transmission: Some(Transmission::glass(0.5)),
            ior: 1.33,
            fur: Some(Fur::new(100, 0.2)),
            ..Material::default()
        };
        let g = m.gpu();
        assert_eq!(
            g.flags[2],
            EXT_CLEARCOAT | EXT_TRANSMISSION | EXT_SPECULAR | EXT_FUR
        );
        assert_eq!(g.ext[0][1], 0.1);
        assert_eq!(g.ext[2][1], 1.33);
        assert_eq!(g.ext[2][2], 0.5);
        assert_eq!(g.ext[3][3], 1.0, "specular strength defaults to 1");
        assert_eq!(g.ext[6][1], 64.0, "shells clamp to 64");
        assert_eq!(g.flags[3], 0);
    }

    #[test]
    fn plain_materials_leave_the_vectors_zero() {
        let g = Material::default().gpu();
        assert_eq!(g.flags[2], 0);
        assert!(g.ext.iter().flatten().all(|v| *v == 0.0));
    }
}
