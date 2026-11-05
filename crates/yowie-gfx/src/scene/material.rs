//! Metallic-roughness materials and their GPU record.
//!
//! # Layout and how to extend it
//!
//! A frame's materials are interned per scene (two draws with equal
//! materials share one record) and uploaded as an array of
//! [`MaterialGpu`] in a storage buffer at group 0, binding 4, indexed by
//! the instance stream. The textures of a material live in a bind group at
//! group 1 (the five core textures at bindings 0 to 4 with their samplers
//! at 5 to 9, the layered feature textures at 10 to 18 sharing the sampler
//! at 19), cached across frames by the texture ids.
//!
//! The layered features (clearcoat, sheen, transmission and the rest, in
//! `layers.rs`) pack into the record's `ext` vectors with a bit each in
//! `flags[2]`; `flags[3]` says which feature textures are present. To add
//! another:
//!
//! 1. Add its type to `layers.rs` and an `Option` of it to [`Material`].
//! 2. Pack it in `layers::pack` into free `ext` components (`ext[9]` is
//!    free) and set a new bit in `flags[2]`.
//! 3. In `shaders/layers.wgsl`, read it in `read_layers` into a new field
//!    of `Layers`; in `shaders/lobes.wgsl`, add its lobe in `layer_light`
//!    and `layer_ambient`.
//! 4. A feature with its own texture appends a slot after the nine layer
//!    textures (`LAYER_TEXTURES` in `layouts.rs`) and to
//!    `layers::layer_textures`.

use bytemuck::{Pod, Zeroable};
use yowie_core::{Color, Vec2};

use super::layers::{
    self, Anisotropy, Clearcoat, Fur, Iridescence, Outline, Sheen, Specular, Transmission,
};
use super::layouts::MATERIAL_TEXTURES;
use super::stencil::Stencil;
use super::surface_shader::SurfaceShader;
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
    /// The surface is drawn after the opaque scene and alpha fades the
    /// whole shaded result (glass, smoke, water). Blended surfaces are
    /// composited order-independently unless the scene asks for sorting
    /// (see [`Transparency`](super::Transparency)).
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

/// Which texture coordinate set (0 or 1) each core texture reads. The
/// layered feature textures read the first set.
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

/// A metallic-roughness physically based material, as glTF defines it,
/// with the layered features of its material extensions.
///
/// Every texture is optional; the factors alone are a complete material.
/// Colours are sRGB with straight alpha. Textures that hold data
/// (metallic-roughness, normal, occlusion and the feature maps) must be
/// created with [`TextureOptions::data`](crate::TextureOptions::data) set;
/// colour ones (base colour, emissive) without it. The default is a white,
/// fully rough dielectric with every layered feature off.
///
/// ```no_run
/// # use yowie_gfx::scene::*;
/// # use yowie_core::Color;
/// let car_paint = Material {
///     clearcoat: Some(Clearcoat::new(1.0, 0.05)),
///     ..Material::color(Color::rgb8(150, 20, 30), 0.0, 0.6)
/// };
/// let glass = Material {
///     transmission: Some(Transmission {
///         attenuation_color: Color::rgb8(120, 200, 255),
///         attenuation_distance: 1.0,
///         ..Transmission::glass(0.8)
///     }),
///     ..Material::color(Color::WHITE, 0.0, 0.05)
/// };
/// ```
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
    /// Which coordinate set each core texture reads.
    pub uv_sets: UvSets,
    /// The index of refraction (KHR_materials_ior): 1.5 for glass and most
    /// plastics, 1.33 for water. It sets how strongly a dielectric
    /// reflects and how much transmission bends light.
    pub ior: f32,
    /// Scales and tints a dielectric's reflection.
    pub specular: Option<Specular>,
    /// A clear varnish layer with its own roughness and normal.
    pub clearcoat: Option<Clearcoat>,
    /// Soft grazing light, the look of cloth.
    pub sheen: Option<Sheen>,
    /// Light let through thin parts and tinted by the base colour (leaves,
    /// wax, skin), 0 to 1; 0 is off. `thickness_texture` shapes it.
    pub subsurface: f32,
    /// Thickness in green (KHR_materials_volume), 0 thin to 1 thick: it
    /// scales the transmission thickness and keeps subsurface light out
    /// of thick parts; data. Without it subsurface treats the surface as
    /// thin everywhere.
    pub thickness_texture: Option<Texture>,
    /// Refracting glass with absorption.
    pub transmission: Option<Transmission>,
    /// A thin film whose colour turns with the angle.
    pub iridescence: Option<Iridescence>,
    /// A highlight stretched along the surface.
    pub anisotropy: Option<Anisotropy>,
    /// Shells of fur over the mesh.
    pub fur: Option<Fur>,
    /// A line around the mesh's silhouette.
    pub outline: Option<Outline>,
    /// A tint, sRGB with alpha as strength, drawn over the parts of the
    /// mesh hidden behind other geometry, so it shows through walls.
    /// Applies to opaque and cut-out materials.
    pub xray: Option<Color>,
    /// The stencil test and write; the default draws everywhere.
    pub stencil: Stencil,
    /// Whether decals project onto the surface.
    pub receive_decals: bool,
    /// A game-written surface program that adjusts the surface before the
    /// engine lights it.
    pub shader: Option<SurfaceShader>,
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
            ior: 1.5,
            specular: None,
            clearcoat: None,
            sheen: None,
            subsurface: 0.0,
            thickness_texture: None,
            transmission: None,
            iridescence: None,
            anisotropy: None,
            fur: None,
            outline: None,
            xray: None,
            stencil: Stencil::default(),
            receive_decals: true,
            shader: None,
        }
    }
}

/// Bits of `MaterialGpu::flags[0]`.
pub(crate) const FLAG_UNLIT: u32 = 1;
pub(crate) const FLAG_VERTEX_COLORS: u32 = 2;
pub(crate) const FLAG_DOUBLE_SIDED: u32 = 4;
pub(crate) const FLAG_BLEND: u32 = 8;
pub(crate) const FLAG_NORMAL_MAP: u32 = 16;
pub(crate) const FLAG_DECALS: u32 = 32;

/// The material record the shaders read: 256 bytes, matching `Material`
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
    /// Feature flags, texture coordinate sets (one bit per core texture),
    /// layered feature flags, layered feature textures present.
    pub flags: [u32; 4],
    /// The layered features' parameters; zero where a feature is off.
    pub ext: [[f32; 4]; 10],
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
        if self.receive_decals {
            flags |= FLAG_DECALS;
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
        let mut g = MaterialGpu {
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
            ext: [[0.0; 4]; 10],
        };
        layers::pack(self, &mut g);
        g
    }

    /// The ids of the material's textures, 0 for none, in binding order.
    pub(crate) fn texture_ids(&self) -> [u64; MATERIAL_TEXTURES as usize] {
        self.textures().map(|t| t.map_or(0, Texture::id))
    }

    /// The textures in binding order: the five core textures, then the
    /// layered feature textures.
    pub(crate) fn textures(&self) -> [Option<&Texture>; MATERIAL_TEXTURES as usize] {
        let core = [
            self.base_color_texture.as_ref(),
            self.metallic_roughness_texture.as_ref(),
            self.normal_texture.as_ref(),
            self.occlusion_texture.as_ref(),
            self.emissive_texture.as_ref(),
        ];
        let ext = layers::layer_textures(self);
        std::array::from_fn(|i| if i < 5 { core[i] } else { ext[i - 5] })
    }

    /// Whether the material draws in the transmission pass.
    pub(crate) fn transmits(&self) -> bool {
        self.transmission.as_ref().is_some_and(|t| t.strength > 0.0)
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
    fn record_is_256_bytes_and_flags_pack() {
        assert_eq!(std::mem::size_of::<MaterialGpu>(), 256);
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
