//! The cube and pen materials: wood, brushed metal, plastic and concrete
//! drawn into textures at start-up (colour, metal-rough and normal maps),
//! plus gold, car paint, velvet, glass and glowing materials made from
//! factors alone.

use yowie::gfx::scene::{AlphaMode, Material};
use yowie::gfx::{Graphics, Texture, TextureOptions, Wrap};
use yowie::{Color, GameResult};

const SIZE: u32 = 128;

/// The kinds of cube, in the order the drop picks them from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    BrushedMetal,
    Gold,
    CarPaint,
    Velvet,
    Glass,
    Glowing,
    Wood,
    Plastic,
}

/// Nine weighted slots; wood takes two so its grain is easy to find.
pub const KINDS: [Kind; 9] = [
    Kind::BrushedMetal,
    Kind::Gold,
    Kind::CarPaint,
    Kind::Velvet,
    Kind::Glass,
    Kind::Glowing,
    Kind::Wood,
    Kind::Wood,
    Kind::Plastic,
];

/// The textured materials, made once and copied per cube.
pub struct Library {
    pub wood: Material,
    pub metal: Material,
    pub plastic: Material,
    pub concrete: Material,
}

/// A surface sample: sRGB colour, roughness, metallic and height (0 to 1).
type Sample = ([u8; 3], f32, f32, f32);

/// Value noise on a grid of `n` by `n` cells that wraps at the edges.
fn noise(u: f32, v: f32, n: u32) -> f32 {
    let hash = |x: u32, y: u32| {
        let mut h = (x % n).wrapping_mul(374_761_393) ^ (y % n).wrapping_mul(668_265_263);
        h = (h ^ (h >> 13)).wrapping_mul(1_274_126_177);
        (h ^ (h >> 16)) as f32 / u32::MAX as f32
    };
    let (x, y) = (u * n as f32, v * n as f32);
    let (xi, yi) = (x.floor() as u32, y.floor() as u32);
    let (fx, fy) = (x.fract(), y.fract());
    let (sx, sy) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
    let a = hash(xi, yi) + (hash(xi + 1, yi) - hash(xi, yi)) * sx;
    let b = hash(xi, yi + 1) + (hash(xi + 1, yi + 1) - hash(xi, yi + 1)) * sx;
    a + (b - a) * sy
}

fn mix(a: [u8; 3], b: [u8; 3], t: f32) -> [u8; 3] {
    std::array::from_fn(|i| (a[i] as f32 + (b[i] as f32 - a[i] as f32) * t.clamp(0.0, 1.0)) as u8)
}

fn wood(u: f32, v: f32) -> Sample {
    let ring = ((u * 9.0 + noise(u, v, 4) * 2.5).sin() * 0.5 + 0.5).powf(0.7);
    let grain = noise(u * 0.5, v * 8.0, 16);
    let c = mix([120, 72, 38], [196, 140, 82], ring * 0.8 + grain * 0.2);
    (c, 0.55 + 0.2 * grain, 0.0, ring * 0.6 + grain * 0.4)
}

fn metal(u: f32, v: f32) -> Sample {
    let streak = noise(u * 0.05, v, 64) * 0.7 + noise(u, v, 8) * 0.3;
    let c = mix([150, 152, 158], [205, 208, 214], streak);
    (c, 0.25 + 0.15 * streak, 1.0, streak * 0.3)
}

fn plastic(u: f32, v: f32) -> Sample {
    let bump = noise(u, v, 32);
    ([235, 235, 235], 0.35 + 0.1 * bump, 0.0, bump * 0.4)
}

fn concrete(u: f32, v: f32) -> Sample {
    let n = noise(u, v, 8) * 0.5 + noise(u, v, 32) * 0.35 + noise(u, v, 64) * 0.15;
    let pit = if noise(u, v, 48) > 0.82 { 0.25 } else { 0.0 };
    let c = mix([110, 110, 112], [170, 168, 165], n - pit);
    (c, 0.85 + 0.1 * n, 0.0, n - pit)
}

/// Draws a surface into colour, metal-rough and normal textures.
fn textures(gfx: &Graphics, f: fn(f32, f32) -> Sample) -> GameResult<[Texture; 3]> {
    let n = SIZE as usize;
    let samples: Vec<Sample> = (0..n * n)
        .map(|i| f((i % n) as f32 / n as f32, (i / n) as f32 / n as f32))
        .collect();
    let (mut color, mut mr, mut normal) = (Vec::new(), Vec::new(), Vec::new());
    for (i, s) in samples.iter().enumerate() {
        color.extend_from_slice(&[s.0[0], s.0[1], s.0[2], 255]);
        mr.extend_from_slice(&[0, (s.1 * 255.0) as u8, (s.2 * 255.0) as u8, 255]);
        let (x, y) = (i % n, i / n);
        let h = |x: usize, y: usize| samples[(y % n) * n + (x % n)].3;
        let dx = (h(x + 1, y) - h(x + n - 1, y)) * 2.0;
        let dy = (h(x, y + 1) - h(x, y + n - 1)) * 2.0;
        let len = (dx * dx + dy * dy + 1.0).sqrt();
        let enc = |c: f32| ((c / len * 0.5 + 0.5) * 255.0) as u8;
        normal.extend_from_slice(&[enc(-dx), enc(-dy), enc(1.0), 255]);
    }
    let srgb = TextureOptions {
        wrap: Wrap::Repeat,
        mipmaps: true,
        ..TextureOptions::smooth()
    };
    let data = TextureOptions { data: true, ..srgb };
    Ok([
        Texture::from_pixels(gfx, SIZE, SIZE, &color, srgb)?,
        Texture::from_pixels(gfx, SIZE, SIZE, &mr, data)?,
        Texture::from_pixels(gfx, SIZE, SIZE, &normal, data)?,
    ])
}

fn textured(gfx: &Graphics, f: fn(f32, f32) -> Sample) -> GameResult<Material> {
    let [color, mr, normal] = textures(gfx, f)?;
    Ok(Material {
        base_color_texture: Some(color),
        metallic: 1.0,
        roughness: 1.0,
        metallic_roughness_texture: Some(mr),
        normal_texture: Some(normal),
        ..Material::default()
    })
}

impl Library {
    pub fn new(gfx: &Graphics) -> GameResult<Self> {
        Ok(Self {
            wood: textured(gfx, wood)?,
            metal: textured(gfx, metal)?,
            plastic: textured(gfx, plastic)?,
            concrete: textured(gfx, concrete)?,
        })
    }

    /// The material of a cube of `kind` in colour `c`.
    pub fn cube(&self, kind: Kind, c: Color) -> Material {
        match kind {
            Kind::BrushedMetal => self.metal.clone(),
            Kind::Gold => Material::color(Color::rgb8(255, 200, 90), 1.0, 0.12),
            Kind::CarPaint => Material::color(c, 0.0, 0.12),
            Kind::Velvet => Material::color(c, 0.0, 1.0),
            Kind::Glass => Material {
                base_color: c.with_alpha(0.35),
                roughness: 0.05,
                alpha_mode: AlphaMode::Blend,
                ..Material::default()
            },
            Kind::Glowing => Material {
                emissive: c,
                emissive_strength: 1.2,
                ..Material::color(c, 0.0, 0.6)
            },
            Kind::Wood => self.wood.clone(),
            Kind::Plastic => Material {
                base_color: c,
                ..self.plastic.clone()
            },
        }
    }
}
