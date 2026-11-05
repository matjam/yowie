//! Colour matrices: saturation, hue rotation, brightness, contrast, sepia
//! and custom 4x5 transforms applied to sprites by a built-in shader.

use bytemuck::{Pod, Zeroable};
use yowie_core::Color;

use crate::error::Result;
use crate::{Graphics, Shader};

const LUMA: [f32; 3] = [0.2126, 0.7152, 0.0722];

/// A 4x5 colour transform. Row `i` produces output channel `i` (red,
/// green, blue, alpha) as the dot product of its first four entries with
/// the input `[r, g, b, a]` plus its fifth entry as a constant. Channels
/// are sRGB values from 0 to 1 with straight alpha, the space CSS filters
/// work in, and results are clamped.
///
/// Build one with the constructors and join them with
/// [`then`](Self::then). Draw sprites through it with
/// [`ColorMatrixShader`]; [`apply`](Self::apply) gives the same result for
/// one colour on the CPU.
///
/// ```
/// use yowie_core::Color;
/// use yowie_gfx::ColorMatrix;
///
/// let m = ColorMatrix::saturation(0.0).then(&ColorMatrix::brightness(0.5));
/// let c = m.apply(Color::rgb(1.0, 0.0, 0.0));
/// assert!((c.r - c.g).abs() < 1e-6 && c.r < 0.2);
/// ```
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ColorMatrix {
    /// Rows for output red, green, blue and alpha; columns for input red,
    /// green, blue, alpha and the constant.
    pub rows: [[f32; 5]; 4],
}

impl Default for ColorMatrix {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl ColorMatrix {
    /// Leaves colours unchanged.
    pub const IDENTITY: Self = Self {
        rows: [
            [1.0, 0.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 0.0, 1.0, 0.0],
        ],
    };

    /// A custom matrix from 20 values, row by row.
    pub fn from_rows(values: [f32; 20]) -> Self {
        let mut rows = [[0.0; 5]; 4];
        for (i, v) in values.iter().enumerate() {
            rows[i / 5][i % 5] = *v;
        }
        Self { rows }
    }

    /// Scales the distance from gray: 1 is unchanged, 0 is gray, above 1
    /// is more vivid.
    pub fn saturation(s: f32) -> Self {
        let mut m = Self::IDENTITY;
        for (r, row) in m.rows.iter_mut().take(3).enumerate() {
            for (c, v) in row.iter_mut().take(3).enumerate() {
                *v = (1.0 - s) * LUMA[c] + if r == c { s } else { 0.0 };
            }
        }
        m
    }

    /// Turns hues by `angle` radians around the gray axis.
    pub fn hue_rotate(angle: f32) -> Self {
        let (s, c) = angle.sin_cos();
        let [lr, lg, lb] = LUMA;
        let mut m = Self::IDENTITY;
        m.rows[0][..3].copy_from_slice(&[
            lr + c * (1.0 - lr) - s * lr,
            lg - c * lg - s * lg,
            lb - c * lb + s * (1.0 - lb),
        ]);
        m.rows[1][..3].copy_from_slice(&[
            lr - c * lr + s * 0.143,
            lg + c * (1.0 - lg) + s * 0.140,
            lb - c * lb - s * 0.283,
        ]);
        m.rows[2][..3].copy_from_slice(&[
            lr - c * lr - s * (1.0 - lr),
            lg - c * lg + s * lg,
            lb + c * (1.0 - lb) + s * lb,
        ]);
        m
    }

    /// Multiplies red, green and blue by `b`.
    pub fn brightness(b: f32) -> Self {
        let mut m = Self::IDENTITY;
        for i in 0..3 {
            m.rows[i][i] = b;
        }
        m
    }

    /// Scales colours away from mid-gray: 1 is unchanged, 0 is flat gray.
    pub fn contrast(c: f32) -> Self {
        let mut m = Self::brightness(c);
        for row in m.rows.iter_mut().take(3) {
            row[4] = 0.5 * (1.0 - c);
        }
        m
    }

    /// Blends towards a sepia tone by `amount`, 0 to 1.
    pub fn sepia(amount: f32) -> Self {
        let a = amount.clamp(0.0, 1.0);
        let full = [
            [0.393, 0.769, 0.189],
            [0.349, 0.686, 0.168],
            [0.272, 0.534, 0.131],
        ];
        let mut m = Self::IDENTITY;
        for r in 0..3 {
            for c in 0..3 {
                let id = if r == c { 1.0 } else { 0.0 };
                m.rows[r][c] = id + (full[r][c] - id) * a;
            }
        }
        m
    }

    /// Inverts red, green and blue.
    pub fn invert() -> Self {
        let mut m = Self::brightness(-1.0);
        for row in m.rows.iter_mut().take(3) {
            row[4] = 1.0;
        }
        m
    }

    /// Multiplies each channel by the matching component of `color`.
    pub fn tint(color: Color) -> Self {
        let mut m = Self::IDENTITY;
        m.rows[0][0] = color.r;
        m.rows[1][1] = color.g;
        m.rows[2][2] = color.b;
        m.rows[3][3] = color.a;
        m
    }

    /// The transform that applies `self` first and `next` second.
    pub fn then(&self, next: &ColorMatrix) -> ColorMatrix {
        let mut out = [[0.0f32; 5]; 4];
        for (r, out_row) in out.iter_mut().enumerate() {
            for (c, v) in out_row.iter_mut().enumerate() {
                let mut sum: f32 = (0..4).map(|k| next.rows[r][k] * self.rows[k][c]).sum();
                if c == 4 {
                    sum += next.rows[r][4];
                }
                *v = sum;
            }
        }
        ColorMatrix { rows: out }
    }

    /// Applies the matrix to an sRGB colour with straight alpha.
    pub fn apply(&self, color: Color) -> Color {
        let input = [color.r, color.g, color.b, color.a, 1.0];
        let out: [f32; 4] = std::array::from_fn(|r| {
            let dot: f32 = self.rows[r].iter().zip(input).map(|(m, v)| m * v).sum();
            dot.clamp(0.0, 1.0)
        });
        Color::rgba(out[0], out[1], out[2], out[3])
    }

    fn uniform(&self) -> Uniform {
        let col = |c: usize| std::array::from_fn(|r| self.rows[r][c]);
        // Columns of the 4x4 part, as a WGSL mat4x4 stores them, then the
        // constant column.
        Uniform {
            cols: [col(0), col(1), col(2), col(3)],
            offset: col(4),
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Uniform {
    cols: [[f32; 4]; 4],
    offset: [f32; 4],
}

const SOURCE: &str = "
struct ColorMatrix {
    m: mat4x4<f32>,
    offset: vec4<f32>,
};
@group(2) @binding(0) var<uniform> cm: ColorMatrix;

fn to_srgb(v: vec3<f32>) -> vec3<f32> {
    let lo = v * 12.92;
    let hi = 1.055 * pow(max(v, vec3<f32>(0.0)), vec3<f32>(1.0 / 2.4)) - vec3<f32>(0.055);
    return select(hi, lo, v <= vec3<f32>(0.0031308));
}

fn to_linear(v: vec3<f32>) -> vec3<f32> {
    let lo = v / 12.92;
    let hi = pow((v + vec3<f32>(0.055)) / 1.055, vec3<f32>(2.4));
    return select(hi, lo, v <= vec3<f32>(0.04045));
}

fn fragment(input: SpriteInput) -> vec4<f32> {
    let c = sample_texture(input.uv) * input.color;
    if (c.a <= 0.0) {
        return vec4<f32>(0.0);
    }
    let straight = vec4<f32>(to_srgb(c.rgb / c.a), c.a);
    let o = clamp(cm.m * straight + cm.offset, vec4<f32>(0.0), vec4<f32>(1.0));
    return vec4<f32>(to_linear(o.rgb) * o.a, o.a);
}
";

/// A sprite shader that applies a [`ColorMatrix`] to everything drawn
/// through it.
///
/// ```ignore
/// let mut sepia = ColorMatrixShader::new(&gfx, &ColorMatrix::sepia(1.0))?;
/// gfx.with_shader(sepia.shader(), |g| g.draw_sprite(&tex, &sprite));
/// sepia.set(&ColorMatrix::saturation(0.0))?;
/// ```
///
/// Changing the matrix with [`set`](Self::set) affects draws queued
/// afterwards; draws already queued keep the matrix they were queued
/// with. Draws through the shader batch together with other draws that use
/// the same shader and texture.
#[derive(Clone, Debug)]
pub struct ColorMatrixShader {
    shader: Shader,
}

impl ColorMatrixShader {
    /// Compiles the shader with `matrix` as its starting value.
    pub fn new(gfx: &Graphics, matrix: &ColorMatrix) -> Result<Self> {
        let shader = gfx.compile_shader_with_uniforms(SOURCE, &matrix.uniform())?;
        Ok(Self { shader })
    }

    /// Replaces the matrix.
    pub fn set(&self, matrix: &ColorMatrix) -> Result<()> {
        self.shader.set_uniforms(&matrix.uniform())
    }

    /// The shader, for [`Graphics::with_shader`] and
    /// [`Graphics::set_shader`].
    pub fn shader(&self) -> &Shader {
        &self.shader
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Color, b: Color) -> bool {
        (a.r - b.r).abs() < 1e-4
            && (a.g - b.g).abs() < 1e-4
            && (a.b - b.b).abs() < 1e-4
            && (a.a - b.a).abs() < 1e-4
    }

    #[test]
    fn identity_like_matrices_leave_colours() {
        let c = Color::rgba(0.2, 0.5, 0.9, 0.7);
        for m in [
            ColorMatrix::IDENTITY,
            ColorMatrix::saturation(1.0),
            ColorMatrix::hue_rotate(0.0),
            ColorMatrix::brightness(1.0),
            ColorMatrix::contrast(1.0),
            ColorMatrix::sepia(0.0),
        ] {
            assert!(close(m.apply(c), c), "{m:?}");
        }
    }

    #[test]
    fn saturation_zero_is_luma_gray() {
        let g = ColorMatrix::saturation(0.0).apply(Color::rgb(1.0, 0.0, 0.0));
        assert!(close(g, Color::rgb(0.2126, 0.2126, 0.2126)));
    }

    #[test]
    fn hue_rotation_turns_back() {
        let c = Color::rgb(0.8, 0.3, 0.1);
        // The matrix uses the rounded coefficients CSS filters use, so
        // turning back is exact only to about a hundredth.
        let near = |a: Color, b: Color| {
            (a.r - b.r)
                .abs()
                .max((a.g - b.g).abs())
                .max((a.b - b.b).abs())
                < 0.01
        };
        let there = ColorMatrix::hue_rotate(1.1).then(&ColorMatrix::hue_rotate(-1.1));
        assert!(near(there.apply(c), c));
        let full = ColorMatrix::hue_rotate(std::f32::consts::TAU);
        assert!(close(full.apply(c), c));
    }

    #[test]
    fn then_applies_left_first() {
        let a = ColorMatrix::brightness(0.5);
        let b = ColorMatrix::invert();
        let c = Color::rgb(0.3, 0.6, 0.4);
        let separate = b.apply(a.apply(c));
        assert!(close(a.then(&b).apply(c), separate));
        assert!(!close(b.then(&a).apply(c), separate), "order matters");
    }

    #[test]
    fn invert_and_custom_rows() {
        assert!(close(
            ColorMatrix::invert().apply(Color::rgb(1.0, 0.25, 0.0)),
            Color::rgb(0.0, 0.75, 1.0)
        ));
        let swap = ColorMatrix::from_rows([
            0.0, 0.0, 1.0, 0.0, 0.0, //
            0.0, 1.0, 0.0, 0.0, 0.0, //
            1.0, 0.0, 0.0, 0.0, 0.0, //
            0.0, 0.0, 0.0, 1.0, 0.0,
        ]);
        assert!(close(
            swap.apply(Color::rgb(1.0, 0.5, 0.0)),
            Color::rgb(0.0, 0.5, 1.0)
        ));
    }

    #[test]
    fn gpu_matches_cpu() {
        use crate::gpu_tests::{assert_px, headless, render};
        use crate::{Sprite, Texture, TextureOptions};
        use yowie_core::{uvec2, vec2};
        let Some(mut gfx) = headless(uvec2(16, 16)) else {
            return;
        };
        let src = Color::rgb8(200, 80, 40);
        let [r, g, b, _] = src.to_rgba8();
        let Ok(tex) = Texture::from_pixels(&gfx, 1, 1, &[r, g, b, 255], TextureOptions::default())
        else {
            panic!("texture");
        };
        let m = ColorMatrix::hue_rotate(2.0).then(&ColorMatrix::saturation(0.5));
        let Ok(shader) = ColorMatrixShader::new(&gfx, &m) else {
            panic!("shader");
        };
        gfx.begin_frame();
        gfx.with_shader(shader.shader(), |g| {
            g.draw_sprite(&tex, &Sprite::at(vec2(0.0, 0.0)).size(vec2(16.0, 16.0)));
        });
        let img = render(&mut gfx);
        let want = m.apply(src).to_rgba8();
        assert_px(&img, 8, 8, want, 3);
    }
}
