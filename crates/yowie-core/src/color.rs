//! Colours as games write them: sRGB components with straight alpha.

use serde::{Deserialize, Serialize};

/// An RGBA colour with sRGB-encoded components in `0.0..=1.0` and straight
/// (not premultiplied) alpha.
///
/// Games specify colours the way artists and colour pickers give them, which
/// is sRGB. Renderers that light or blend in linear space call
/// [`Color::to_linear`]; components outside `0..=1` are allowed so HDR
/// emissive colours can be expressed.
#[repr(C)]
#[derive(
    Clone, Copy, Debug, PartialEq, Serialize, Deserialize, bytemuck::Pod, bytemuck::Zeroable,
)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Default for Color {
    fn default() -> Self {
        Self::WHITE
    }
}

impl Color {
    pub const TRANSPARENT: Self = Self::rgba(0.0, 0.0, 0.0, 0.0);
    pub const BLACK: Self = Self::rgb(0.0, 0.0, 0.0);
    pub const WHITE: Self = Self::rgb(1.0, 1.0, 1.0);
    pub const GRAY: Self = Self::rgb(0.5, 0.5, 0.5);
    pub const RED: Self = Self::rgb(1.0, 0.0, 0.0);
    pub const GREEN: Self = Self::rgb(0.0, 1.0, 0.0);
    pub const BLUE: Self = Self::rgb(0.0, 0.0, 1.0);
    pub const YELLOW: Self = Self::rgb(1.0, 1.0, 0.0);
    pub const CYAN: Self = Self::rgb(0.0, 1.0, 1.0);
    pub const MAGENTA: Self = Self::rgb(1.0, 0.0, 1.0);
    pub const ORANGE: Self = Self::rgb(1.0, 0.5, 0.0);

    /// An opaque colour from sRGB components.
    pub const fn rgb(r: f32, g: f32, b: f32) -> Self {
        Self { r, g, b, a: 1.0 }
    }

    /// A colour from sRGB components and straight alpha.
    pub const fn rgba(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }

    /// An opaque colour from 8-bit sRGB components.
    pub const fn rgb8(r: u8, g: u8, b: u8) -> Self {
        Self::rgba8(r, g, b, 255)
    }

    /// A colour from 8-bit sRGB components and alpha.
    pub const fn rgba8(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self {
            r: r as f32 / 255.0,
            g: g as f32 / 255.0,
            b: b as f32 / 255.0,
            a: a as f32 / 255.0,
        }
    }

    /// A colour from `0xRRGGBB`; alpha is opaque.
    pub const fn hex(rgb: u32) -> Self {
        Self::rgb8((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
    }

    /// A colour from `0xRRGGBBAA`.
    pub const fn hexa(rgba: u32) -> Self {
        Self::rgba8(
            (rgba >> 24) as u8,
            (rgba >> 16) as u8,
            (rgba >> 8) as u8,
            rgba as u8,
        )
    }

    /// Parses `#rgb`, `#rgba`, `#rrggbb` or `#rrggbbaa` (the `#` is optional).
    pub fn parse_hex(s: &str) -> Option<Self> {
        let s = s.strip_prefix('#').unwrap_or(s);
        let nibble = |i: usize| {
            u8::from_str_radix(s.get(i..i + 1)?, 16)
                .ok()
                .map(|v| v * 17)
        };
        let byte = |i: usize| u8::from_str_radix(s.get(i..i + 2)?, 16).ok();
        match s.len() {
            3 => Some(Self::rgb8(nibble(0)?, nibble(1)?, nibble(2)?)),
            4 => Some(Self::rgba8(nibble(0)?, nibble(1)?, nibble(2)?, nibble(3)?)),
            6 => Some(Self::rgb8(byte(0)?, byte(2)?, byte(4)?)),
            8 => Some(Self::rgba8(byte(0)?, byte(2)?, byte(4)?, byte(6)?)),
            _ => None,
        }
    }

    /// A colour from hue in degrees, saturation and value in `0..=1`.
    pub fn hsv(hue: f32, saturation: f32, value: f32) -> Self {
        let h = hue.rem_euclid(360.0) / 60.0;
        let c = value * saturation;
        let x = c * (1.0 - (h % 2.0 - 1.0).abs());
        let m = value - c;
        let (r, g, b) = match h as u32 {
            0 => (c, x, 0.0),
            1 => (x, c, 0.0),
            2 => (0.0, c, x),
            3 => (0.0, x, c),
            4 => (x, 0.0, c),
            _ => (c, 0.0, x),
        };
        Self::rgb(r + m, g + m, b + m)
    }

    /// The same colour with a different alpha.
    pub const fn with_alpha(self, a: f32) -> Self {
        Self { a, ..self }
    }

    /// Interpolates in sRGB space; `t = 0` gives `self`, `t = 1` gives `other`.
    pub fn lerp(self, other: Self, t: f32) -> Self {
        Self::rgba(
            self.r + (other.r - self.r) * t,
            self.g + (other.g - self.g) * t,
            self.b + (other.b - self.b) * t,
            self.a + (other.a - self.a) * t,
        )
    }

    /// Components as an array `[r, g, b, a]`, still sRGB.
    pub const fn to_array(self) -> [f32; 4] {
        [self.r, self.g, self.b, self.a]
    }

    /// Components as 8-bit sRGB, clamped.
    pub fn to_rgba8(self) -> [u8; 4] {
        let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
        [q(self.r), q(self.g), q(self.b), q(self.a)]
    }

    /// Linear-light components `[r, g, b, a]` for lighting and blending.
    /// Alpha is not encoded and passes through unchanged.
    pub fn to_linear(self) -> [f32; 4] {
        [
            srgb_to_linear(self.r),
            srgb_to_linear(self.g),
            srgb_to_linear(self.b),
            self.a,
        ]
    }

    /// Linear-light components with RGB multiplied by alpha, the form a
    /// premultiplied blend state expects.
    pub fn to_linear_premultiplied(self) -> [f32; 4] {
        let [r, g, b, a] = self.to_linear();
        [r * a, g * a, b * a, a]
    }

    /// The colour whose [`Color::to_linear`] is `linear`.
    pub fn from_linear(linear: [f32; 4]) -> Self {
        Self::rgba(
            linear_to_srgb(linear[0]),
            linear_to_srgb(linear[1]),
            linear_to_srgb(linear[2]),
            linear[3],
        )
    }
}

/// Multiplies each component, alpha included: tinting a white sprite by a
/// colour gives that colour.
impl std::ops::Mul for Color {
    type Output = Color;

    fn mul(self, other: Color) -> Color {
        Color::rgba(
            self.r * other.r,
            self.g * other.g,
            self.b * other.b,
            self.a * other.a,
        )
    }
}

impl From<[f32; 4]> for Color {
    fn from(v: [f32; 4]) -> Self {
        Self::rgba(v[0], v[1], v[2], v[3])
    }
}

impl From<[f32; 3]> for Color {
    fn from(v: [f32; 3]) -> Self {
        Self::rgb(v[0], v[1], v[2])
    }
}

impl From<Color> for [f32; 4] {
    fn from(c: Color) -> Self {
        c.to_array()
    }
}

/// Decodes one sRGB component to linear light (the IEC 61966-2-1 curve).
/// Negative values and values above one are extended symmetrically so HDR
/// colours survive the round trip.
pub fn srgb_to_linear(v: f32) -> f32 {
    let a = v.abs();
    let l = if a <= 0.04045 {
        a / 12.92
    } else {
        ((a + 0.055) / 1.055).powf(2.4)
    };
    l.copysign(v)
}

/// Encodes one linear-light component to sRGB; the inverse of [`srgb_to_linear`].
pub fn linear_to_srgb(v: f32) -> f32 {
    let a = v.abs();
    let s = if a <= 0.003_130_8 {
        a * 12.92
    } else {
        1.055 * a.powf(1.0 / 2.4) - 0.055
    };
    s.copysign(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_forms_agree() {
        let want = Color::rgba8(0x11, 0x22, 0x33, 0xff);
        for s in ["#123", "#123f", "112233", "#112233ff"] {
            assert_eq!(Color::parse_hex(s), Some(want), "{s}");
        }
        assert_eq!(Color::hex(0x112233), want);
        assert_eq!(Color::hexa(0x112233ff), want);
        assert_eq!(Color::parse_hex("#12"), None);
        assert_eq!(Color::parse_hex("#zzzzzz"), None);
    }

    #[test]
    fn linear_round_trip() {
        for i in 0..=20 {
            let v = i as f32 / 10.0 - 0.5;
            assert!((linear_to_srgb(srgb_to_linear(v)) - v).abs() < 1e-5, "{v}");
        }
        assert!((srgb_to_linear(0.5) - 0.214_041).abs() < 1e-5);
    }

    #[test]
    fn hsv_primaries() {
        let cases = [
            (0.0, Color::RED),
            (120.0, Color::GREEN),
            (240.0, Color::BLUE),
            (360.0, Color::RED),
        ];
        for (h, want) in cases {
            let got = Color::hsv(h, 1.0, 1.0);
            assert!(
                (got.r - want.r).abs() < 1e-6
                    && (got.g - want.g).abs() < 1e-6
                    && (got.b - want.b).abs() < 1e-6
            );
        }
    }

    #[test]
    fn rgba8_round_trip_and_serde() {
        let c = Color::rgba8(10, 200, 30, 128);
        assert_eq!(c.to_rgba8(), [10, 200, 30, 128]);
        let json = serde_json::to_string(&c).unwrap();
        assert_eq!(serde_json::from_str::<Color>(&json).unwrap(), c);
    }
}
