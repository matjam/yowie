//! Metallic-roughness materials and their GPU record.
//!
//! # Layout and how to extend it
//!
//! A frame's materials are interned per scene (two draws with equal
//! materials share one record) and uploaded as an array of
//! [`MaterialGpu`] in a storage buffer at group 0, binding 4, indexed by
//! the instance stream. The textures of a material live in a bind group at
//! group 1 (bindings 0 to 4 are the textures, 5 to 9 their samplers),
//! cached across frames by the five texture ids.
//!
//! To add a layered feature (clearcoat, sheen, transmission, iridescence,
//! anisotropy, specular tint, subsurface, fur shells):
//!
//! 1. Add its fields to [`Material`] with defaults that switch it off.
//! 2. Pack them into one or more of the `ext` vectors of [`MaterialGpu`]
//!    in [`Material::gpu`] and set a bit in `flags[2]` (the
//!    [`EXT_CLEARCOAT`]-style constants below reserve the bits). The record
//!    keeps its size, so the storage layout and every shader stay valid.
//! 3. In `shaders/surface.wgsl`, read the vectors in `surface()` into new
//!    fields of `Surface`, and add the lobe in `shade_light()` and
//!    `shade_ambient()` behind the flag bit.
//! 4. A feature with its own texture appends a binding pair after 9 to
//!    `MATERIAL_TEXTURES` in `layouts.rs` and a slot in the cache key in
//!    `textures.rs`. Fur shells, which draw a mesh several times, add a
//!    draw class in `prepare.rs` beside the blended one.

use bytemuck::{Pod, Zeroable};
use yowie_core::{Color, Vec2};

use crate::texture::Texture;

/// How a material's alpha is used.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum AlphaMode {
    /// Alpha is ignored; the surface is solid.
    #[default]
    Opaque,
    /// Fragments with alpha below the cutoff are discarded, in the lit pass
    /// and the shadow pass, giving hard edges and cut-out shadows (leaves,
    /// fences, grass).
    Mask(f32),
    /// The surface is drawn after the opaque scene, sorted back to front,
    /// and alpha fades the whole shaded result (glass, smoke, water).
    Blend,
}

/// An affine transform of texture coordinates (KHR_texture_transform):
/// scale, then rotate by `rotation` radians, then offset.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextureTransform {
    /// Added after scaling and rotation.
    pub offset: Vec2,
    /// Counter-clockwise turn in radians.
    pub rotation: f32,
    /// Multiplies the coordinates first.
    pub scale: Vec2,
}

impl Default for TextureTransform {
    fn default() -> Self {
        Self {
            offset: Vec2::ZERO,
            rotation: 0.0,
            scale: Vec2::ONE,
        }
    }
}

impl TextureTransform {
    /// A transform that tiles the texture `x` by `y` times.
    pub fn tiled(x: f32, y: f32) -> Self {
        Self {
            scale: Vec2::new(x, y),
            ..Self::default()
        }
    }

    /// The two rows of the 2x3 matrix, as glTF defines it.
    fn rows(&self) -> [[f32; 4]; 2] {
        let (s, c) = self.rotation.sin_cos();
        [
            [c * self.scale.x, s * self.scale.y, self.offset.x, 0.0],
            [-s * self.scale.x, c * self.scale.y, self.offset.y, 0.0],
        ]
    }
}

/// Which texture coordinate set (0 or 1) each texture reads.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct UvSets {
    /// For the base colour texture.
    pub base_color: u8,
    /// For the metallic-roughness texture.
    pub metallic_roughness: u8,
    /// For the normal map.
    pub normal: u8,
    /// For the occlusion texture.
    pub occlusion: u8,
    /// For the emissive texture.
    pub emissive: u8,
}

/// A metallic-roughness physically based material, as glTF defines it.
///
/// Every texture is optional; the factors alone are a complete material.
/// Colours are sRGB with straight alpha. Textures that hold data
/// (metallic-roughness, normal, occlusion) must be created with
/// [`TextureOptions::data`](crate::TextureOptions::data) set; colour ones
/// (base colour, emissive) without it. The default is a white, fully rough
/// dielectric.
#[derive(Clone, Debug, PartialEq)]
pub struct Material {
    /// Multiplies the base colour texture and the vertex colour; its alpha
    /// is the surface's alpha.
    pub base_color: Color,
    /// The albedo, sRGB.
    pub base_color_texture: Option<Texture>,
    /// 0 for a dielectric, 1 for a metal.
    pub metallic: f32,
    /// 0 for a mirror, 1 for fully matte.
    pub roughness: f32,
    /// Roughness in green and metallic in blue, multiplying the factors.
    pub metallic_roughness_texture: Option<Texture>,
    /// A tangent-space normal map.
    pub normal_texture: Option<Texture>,
    /// Scales the normal map's X and Y.
    pub normal_scale: f32,
    /// Ambient occlusion in red.
    pub occlusion_texture: Option<Texture>,
    /// How much the occlusion texture darkens ambient light, 0 to 1.
    pub occlusion_strength: f32,
    /// The emitted colour, sRGB; black for none.
    pub emissive: Color,
    /// Multiplies `emissive` (KHR_materials_emissive_strength); values
    /// above 1 glow past white in the HDR scene.
    pub emissive_strength: f32,
    /// Multiplies `emissive`, sRGB.
    pub emissive_texture: Option<Texture>,
    /// Opaque, cut out, or blended.
    pub alpha_mode: AlphaMode,
    /// Draws back faces too, lit with a flipped normal.
    pub double_sided: bool,
    /// Shows the base colour and emissive without lighting.
    pub unlit: bool,
    /// Whether vertex colours multiply the base colour.
    pub vertex_colors: bool,
    /// Moves every texture's coordinates.
    pub uv_transform: TextureTransform,
    /// Which coordinate set each texture reads.
    pub uv_sets: UvSets,
}

impl Default for Material {
    fn default() -> Self {
        Self {
            base_color: Color::WHITE,
            base_color_texture: None,
            metallic: 0.0,
            roughness: 1.0,
            metallic_roughness_texture: None,
            normal_texture: None,
            normal_scale: 1.0,
            occlusion_texture: None,
            occlusion_strength: 1.0,
            emissive: Color::BLACK,
            emissive_strength: 1.0,
            emissive_texture: None,
            alpha_mode: AlphaMode::Opaque,
            double_sided: false,
            unlit: false,
            vertex_colors: true,
            uv_transform: TextureTransform::default(),
            uv_sets: UvSets::default(),
        }
    }
}

/// Bits of `MaterialGpu::flags[0]`.
pub(crate) const FLAG_UNLIT: u32 = 1;
pub(crate) const FLAG_VERTEX_COLORS: u32 = 2;
pub(crate) const FLAG_DOUBLE_SIDED: u32 = 4;
pub(crate) const FLAG_BLEND: u32 = 8;
pub(crate) const FLAG_NORMAL_MAP: u32 = 16;

/// Bits of `MaterialGpu::flags[2]` reserved for the layered features.
pub const EXT_CLEARCOAT: u32 = 1;
/// Reserved for sheen.
pub const EXT_SHEEN: u32 = 2;
/// Reserved for transmission.
pub const EXT_TRANSMISSION: u32 = 4;
/// Reserved for iridescence.
pub const EXT_IRIDESCENCE: u32 = 8;
/// Reserved for anisotropy.
pub const EXT_ANISOTROPY: u32 = 16;
/// Reserved for specular tint.
pub const EXT_SPECULAR: u32 = 32;
/// Reserved for subsurface scattering.
pub const EXT_SUBSURFACE: u32 = 64;
/// Reserved for fur shells.
pub const EXT_FUR: u32 = 128;

/// The material record the shaders read: 192 bytes, matching `Material`
/// in `shaders/common.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct MaterialGpu {
    /// Linear base colour with straight alpha.
    pub base_color: [f32; 4],
    /// Linear emitted colour times strength; `w` is the normal scale.
    pub emissive: [f32; 4],
    /// Metallic, roughness, occlusion strength, alpha cutoff (negative
    /// when the material does not cut out).
    pub params: [f32; 4],
    /// The texture transform's first row (`a b tx`).
    pub uv_row0: [f32; 4],
    /// The texture transform's second row (`c d ty`).
    pub uv_row1: [f32; 4],
    /// Feature flags, texture coordinate sets (one bit per texture),
    /// extension flags, reserved.
    pub flags: [u32; 4],
    /// Free vectors for the layered features; zero means off.
    pub ext: [[f32; 4]; 6],
}

impl Material {
    /// The GPU record for this material.
    pub fn gpu(&self) -> MaterialGpu {
        let mut flags = 0;
        if self.unlit {
            flags |= FLAG_UNLIT;
        }
        if self.vertex_colors {
            flags |= FLAG_VERTEX_COLORS;
        }
        if self.double_sided {
            flags |= FLAG_DOUBLE_SIDED;
        }
        if self.alpha_mode == AlphaMode::Blend {
            flags |= FLAG_BLEND;
        }
        if self.normal_texture.is_some() {
            flags |= FLAG_NORMAL_MAP;
        }
        let u = &self.uv_sets;
        let uv_bits = [
            u.base_color,
            u.metallic_roughness,
            u.normal,
            u.occlusion,
            u.emissive,
        ]
        .iter()
        .enumerate()
        .fold(0u32, |acc, (i, s)| acc | (u32::from(*s > 0) << i));
        let cutoff = match self.alpha_mode {
            AlphaMode::Mask(c) => c,
            _ => -1.0,
        };
        let e = self.emissive.to_linear();
        let k = self.emissive_strength;
        let [row0, row1] = self.uv_transform.rows();
        MaterialGpu {
            base_color: self.base_color.to_linear(),
            emissive: [e[0] * k, e[1] * k, e[2] * k, self.normal_scale],
            params: [
                self.metallic.clamp(0.0, 1.0),
                self.roughness.clamp(0.0, 1.0),
                self.occlusion_strength.clamp(0.0, 1.0),
                cutoff,
            ],
            uv_row0: row0,
            uv_row1: row1,
            flags: [flags, uv_bits, 0, 0],
            ext: [[0.0; 4]; 6],
        }
    }

    /// The ids of the five textures, 0 for none, in binding order.
    pub(crate) fn texture_ids(&self) -> [u64; 5] {
        [
            &self.base_color_texture,
            &self.metallic_roughness_texture,
            &self.normal_texture,
            &self.occlusion_texture,
            &self.emissive_texture,
        ]
        .map(|t| t.as_ref().map_or(0, Texture::id))
    }

    /// The textures in binding order.
    pub(crate) fn textures(&self) -> [Option<&Texture>; 5] {
        [
            self.base_color_texture.as_ref(),
            self.metallic_roughness_texture.as_ref(),
            self.normal_texture.as_ref(),
            self.occlusion_texture.as_ref(),
            self.emissive_texture.as_ref(),
        ]
    }

    /// A plain coloured material with the given metallic and roughness.
    pub fn color(base_color: Color, metallic: f32, roughness: f32) -> Self {
        Self {
            base_color,
            metallic,
            roughness,
            ..Self::default()
        }
    }

    /// A material that glows `color` at `strength` and is not lit.
    pub fn glowing(color: Color, strength: f32) -> Self {
        Self {
            base_color: Color::BLACK,
            emissive: color,
            emissive_strength: strength,
            unlit: true,
            ..Self::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_is_192_bytes_and_flags_pack() {
        assert_eq!(std::mem::size_of::<MaterialGpu>(), 192);
        let m = Material {
            alpha_mode: AlphaMode::Mask(0.4),
            double_sided: true,
            uv_sets: UvSets {
                occlusion: 1,
                ..UvSets::default()
            },
            ..Material::default()
        };
        let g = m.gpu();
        assert_eq!(g.params[3], 0.4);
        assert_eq!(g.flags[0] & FLAG_DOUBLE_SIDED, FLAG_DOUBLE_SIDED);
        assert_eq!(g.flags[1], 1 << 3);
        let t = TextureTransform {
            offset: Vec2::new(0.5, 0.0),
            rotation: 0.0,
            scale: Vec2::new(2.0, 3.0),
        };
        assert_eq!(t.rows(), [[2.0, 0.0, 0.5, 0.0], [0.0, 3.0, 0.0, 0.0]]);
    }
}
