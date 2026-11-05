//! Procedural pixel art shared by the 2D world examples. Every picture is
//! computed from integer hashes, so each run draws the same pixels and no
//! asset files or licences are involved.

#![allow(dead_code)]

use yowie::gfx::image::{Rgba, RgbaImage};

/// A deterministic hash of a cell and a seed, 0 to 255.
pub fn hash(x: i32, y: i32, seed: u32) -> u8 {
    let mut h = (x as u32).wrapping_mul(0x9e37_79b1)
        ^ (y as u32).wrapping_mul(0x85eb_ca6b)
        ^ seed.wrapping_mul(0xc2b2_ae35);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2c1b_3c6d);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297a_2d39);
    h ^= h >> 15;
    (h >> 24) as u8
}

/// Scales an RGB colour by `f`.
pub fn shade(c: [u8; 3], f: f32) -> [u8; 4] {
    let s = |v: u8| (f32::from(v) * f).clamp(0.0, 255.0) as u8;
    [s(c[0]), s(c[1]), s(c[2]), 255]
}

/// An RGBA image to draw into.
pub struct Canvas {
    /// The pixels.
    pub img: RgbaImage,
}

impl Canvas {
    /// A transparent canvas.
    pub fn new(w: u32, h: u32) -> Self {
        Self {
            img: RgbaImage::new(w, h),
        }
    }

    /// Sets a pixel; positions outside the canvas are ignored.
    pub fn put(&mut self, x: i32, y: i32, c: [u8; 4]) {
        if x >= 0 && y >= 0 && (x as u32) < self.img.width() && (y as u32) < self.img.height() {
            self.img.put_pixel(x as u32, y as u32, Rgba(c));
        }
    }

    /// Fills a rectangle.
    pub fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, c: [u8; 4]) {
        for yy in y..y + h {
            for xx in x..x + w {
                self.put(xx, yy, c);
            }
        }
    }

    /// Fills a disc.
    pub fn disc(&mut self, cx: f32, cy: f32, r: f32, c: [u8; 4]) {
        let (x0, x1) = ((cx - r).floor() as i32, (cx + r).ceil() as i32);
        let (y0, y1) = ((cy - r).floor() as i32, (cy + r).ceil() as i32);
        for y in y0..=y1 {
            for x in x0..=x1 {
                let (dx, dy) = (x as f32 + 0.5 - cx, y as f32 + 0.5 - cy);
                if dx * dx + dy * dy <= r * r {
                    self.put(x, y, c);
                }
            }
        }
    }

    /// Fills the part of the cell at `(ox, oy)` of `size` pixels where
    /// `f(x, y)` (cell-local) returns a colour.
    pub fn paint(&mut self, ox: i32, oy: i32, size: i32, f: impl Fn(i32, i32) -> Option<[u8; 4]>) {
        for y in 0..size {
            for x in 0..size {
                if let Some(c) = f(x, y) {
                    self.put(ox + x, oy + y, c);
                }
            }
        }
    }
}
