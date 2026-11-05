//! The valley's ground: the height map, the four ground textures and the
//! rules that paint them.

use yowie::gfx::Graphics;
use yowie::gfx::Texture;
use yowie::gfx::TextureOptions;
use yowie::gfx::image::RgbaImage;
use yowie::gfx::scene::{HeightMap, LayerRule, SplatRules, TerrainLayer};
use yowie::rng::Noise;
use yowie::{GameResult, Vec2};

/// Height samples along each side.
pub const SAMPLES: u32 = 257;
/// World units between samples.
pub const CELL: f32 = 1.5;
/// The lake's surface height.
pub const WATER: f32 = 0.0;
/// The middle of the lake basin, world X and Z.
pub const LAKE: Vec2 = Vec2::new(30.0, 35.0);
/// The basin's radius in world units.
pub const LAKE_RADIUS: f32 = 60.0;
/// Heights above this are snow on gentle slopes.
pub const SNOW_LINE: f32 = 30.0;

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The ground height at world `x`, `z`: rolling hills, a ridge of
/// mountains along the north (-Z) side and a basin for the lake.
fn height(noise: &Noise, x: f32, z: f32) -> f32 {
    let rolling = noise.fbm(x / 120.0, z / 120.0, 5, 2.0, 0.5);
    let hills = rolling * 16.0 + 5.0;
    let north = smoothstep(-10.0, -170.0, z);
    let ridge = noise.ridged(x / 75.0 + 3.1, z / 75.0 - 1.7, 5, 2.1, 0.5);
    let mountains = north * (ridge * 62.0 + 6.0) * (0.75 + 0.5 * rolling);
    let d = (Vec2::new(x, z) - LAKE).length() / LAKE_RADIUS;
    let basin = 1.0 - smoothstep(0.0, 1.0, d);
    hills + mountains - basin * 17.0
}

/// The valley's height map, centred on the origin.
pub fn height_map(seed: u64) -> HeightMap {
    let noise = Noise::new(seed);
    let half = (SAMPLES - 1) as f32 * CELL * 0.5;
    HeightMap::from_fn(SAMPLES, SAMPLES, |i, j| {
        height(&noise, i as f32 * CELL - half, j as f32 * CELL - half)
    })
}

/// Sand at the shore, grass over the hills, rock on steep ground and
/// snow on high gentle slopes.
pub fn splat_rules() -> SplatRules {
    SplatRules {
        base: 1,
        layers: [
            Some(LayerRule::below(WATER + 1.3, 1.2)),
            None,
            Some(LayerRule::steeper_than(0.6, 0.14)),
            Some(LayerRule::above(SNOW_LINE, 5.0).gentler_than(0.85)),
        ],
        jitter: 0.8,
    }
}

/// Fractal noise that repeats every `size` pixels in both directions,
/// in about -0.5 to 0.5.
fn tiling(noise: &Noise, x: f32, y: f32, size: f32, freq: f32) -> f32 {
    let s = |x: f32, y: f32| noise.fbm(x * freq / size, y * freq / size, 4, 2.0, 0.5);
    let (u, v) = (x / size, y / size);
    s(x, y) * (1.0 - u) * (1.0 - v)
        + s(x - size, y) * u * (1.0 - v)
        + s(x, y - size) * (1.0 - u) * v
        + s(x - size, y - size) * u * v
}

/// A tiling ground texture: `base` sRGB shaded by `paint(x, y)`, which
/// returns an offset to add to each channel.
fn ground(
    gfx: &Graphics,
    base: [f32; 3],
    paint: impl Fn(f32, f32) -> [f32; 3],
) -> GameResult<Texture> {
    const SIZE: u32 = 128;
    let img = RgbaImage::from_fn(SIZE, SIZE, |x, y| {
        let d = paint(x as f32, y as f32);
        let c = |i: usize| (base[i] + d[i]).clamp(0.0, 255.0) as u8;
        yowie::gfx::image::Rgba([c(0), c(1), c(2), 255])
    });
    Ok(Texture::from_image(gfx, &img, TextureOptions::smooth())?)
}

/// The sand, grass, rock and snow layers.
pub fn layers(gfx: &Graphics, seed: u64) -> GameResult<[TerrainLayer; 4]> {
    let n = Noise::new(seed ^ 0x5eed);
    let size = 128.0;
    let sand = ground(gfx, [200.0, 182.0, 140.0], |x, y| {
        let grain = tiling(&n, x, y, size, 24.0) * 40.0;
        let ripple = tiling(&n, x + 50.0, y, size, 4.0) * 30.0;
        [grain + ripple, grain + ripple, grain * 0.8 + ripple]
    })?;
    let grass = ground(gfx, [82.0, 122.0, 52.0], |x, y| {
        let patch = tiling(&n, x, y + 30.0, size, 3.0) * 70.0;
        let blade = tiling(&n, x * 1.0, y + 90.0, size, 32.0) * 45.0;
        [
            patch * 0.6 + blade,
            patch + blade,
            patch * 0.3 + blade * 0.5,
        ]
    })?;
    let rock = ground(gfx, [118.0, 112.0, 104.0], |x, y| {
        let strata = tiling(&n, x * 0.3 + 10.0, y, size, 10.0) * 70.0;
        let grit = tiling(&n, x, y + 70.0, size, 40.0) * 35.0;
        let v = strata + grit;
        [v, v, v * 0.95]
    })?;
    let snow = ground(gfx, [232.0, 238.0, 247.0], |x, y| {
        let drift = tiling(&n, x + 20.0, y + 20.0, size, 5.0) * 24.0;
        let sparkle = tiling(&n, x, y, size, 48.0) * 10.0;
        [drift + sparkle, drift + sparkle, drift * 0.5 + sparkle]
    })?;
    Ok([
        TerrainLayer::textured(&sand, 6.0, 0.95),
        TerrainLayer::textured(&grass, 7.0, 0.9),
        TerrainLayer::textured(&rock, 9.0, 0.8),
        TerrainLayer::textured(&snow, 12.0, 0.6),
    ])
}
