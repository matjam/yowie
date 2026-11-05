//! Runtime texture atlases: a rectangle packer and named regions.

use std::collections::HashMap;

use image::{RgbaImage, imageops};
use yowie_core::{Rect, UVec2, uvec2};

use crate::error::{GfxError, Result};
use crate::{Graphics, Sprite, Texture, TextureOptions};

/// A whole-texel rectangle in a packing area.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PackRect {
    /// Left edge.
    pub x: u32,
    /// Top edge.
    pub y: u32,
    /// Width.
    pub w: u32,
    /// Height.
    pub h: u32,
}

impl PackRect {
    fn intersects(&self, o: &PackRect) -> bool {
        self.x < o.x + o.w && o.x < self.x + self.w && self.y < o.y + o.h && o.y < self.y + self.h
    }

    fn contains(&self, o: &PackRect) -> bool {
        o.x >= self.x
            && o.y >= self.y
            && o.x + o.w <= self.x + self.w
            && o.y + o.h <= self.y + self.h
    }

    /// The rectangle as floats, for use as a sprite source.
    pub fn to_rect(self) -> Rect {
        Rect::new(self.x as f32, self.y as f32, self.w as f32, self.h as f32)
    }
}

/// Packs rectangles into a fixed area with the MaxRects algorithm (best
/// short side fit). Rectangles never overlap and never leave the area.
///
/// ```
/// use yowie_core::uvec2;
/// use yowie_gfx::AtlasPacker;
///
/// let mut packer = AtlasPacker::new(uvec2(64, 64));
/// let a = packer.pack(uvec2(32, 16)).unwrap();
/// let b = packer.pack(uvec2(32, 16)).unwrap();
/// assert_ne!((a.x, a.y), (b.x, b.y));
/// ```
#[derive(Clone, Debug)]
pub struct AtlasPacker {
    size: UVec2,
    free: Vec<PackRect>,
}

impl AtlasPacker {
    /// An empty packer for an area of `size` texels.
    pub fn new(size: UVec2) -> Self {
        Self {
            size,
            free: vec![PackRect {
                x: 0,
                y: 0,
                w: size.x,
                h: size.y,
            }],
        }
    }

    /// The area's size.
    pub fn size(&self) -> UVec2 {
        self.size
    }

    /// Places a rectangle of `size` and returns where it went, or `None`
    /// when no free space fits. A zero-sized request is placed at the
    /// origin without using space.
    pub fn pack(&mut self, size: UVec2) -> Option<PackRect> {
        if size.x == 0 || size.y == 0 {
            return Some(PackRect {
                x: 0,
                y: 0,
                w: size.x,
                h: size.y,
            });
        }
        let mut best: Option<(u32, u32, PackRect)> = None;
        for f in &self.free {
            if f.w >= size.x && f.h >= size.y {
                let (dw, dh) = (f.w - size.x, f.h - size.y);
                let key = (dw.min(dh), dw.max(dh));
                if best.is_none_or(|(s, l, _)| key < (s, l)) {
                    best = Some((key.0, key.1, *f));
                }
            }
        }
        let (_, _, at) = best?;
        let placed = PackRect {
            x: at.x,
            y: at.y,
            w: size.x,
            h: size.y,
        };
        self.split(&placed);
        Some(placed)
    }

    fn split(&mut self, used: &PackRect) {
        let mut next = Vec::with_capacity(self.free.len() + 4);
        for f in &self.free {
            if !f.intersects(used) {
                next.push(*f);
                continue;
            }
            if used.x > f.x {
                next.push(PackRect {
                    w: used.x - f.x,
                    ..*f
                });
            }
            if used.x + used.w < f.x + f.w {
                next.push(PackRect {
                    x: used.x + used.w,
                    w: f.x + f.w - (used.x + used.w),
                    ..*f
                });
            }
            if used.y > f.y {
                next.push(PackRect {
                    h: used.y - f.y,
                    ..*f
                });
            }
            if used.y + used.h < f.y + f.h {
                next.push(PackRect {
                    y: used.y + used.h,
                    h: f.y + f.h - (used.y + used.h),
                    ..*f
                });
            }
        }
        // Drop free rectangles contained in another one.
        let mut keep = vec![true; next.len()];
        for i in 0..next.len() {
            for j in 0..next.len() {
                if i != j && keep[j] && next[j].contains(&next[i]) && (next[i] != next[j] || j < i)
                {
                    keep[i] = false;
                    break;
                }
            }
        }
        self.free = next
            .into_iter()
            .zip(keep)
            .filter_map(|(r, k)| k.then_some(r))
            .collect();
    }
}

/// Collects images and packs them into one texture.
///
/// Each image gets `padding` transparent texels around it, and the edge
/// texels are extruded into the padding so linear filtering at a region's
/// border does not pick up a neighbour. The atlas grows from 64 texels, in
/// powers of two, until everything fits or `max_size` is reached.
#[derive(Clone, Debug)]
pub struct AtlasBuilder {
    images: Vec<(String, RgbaImage)>,
    /// Texels of space around each image; default 1.
    pub padding: u32,
    /// The largest atlas side to try; default 4096.
    pub max_size: u32,
}

impl Default for AtlasBuilder {
    fn default() -> Self {
        Self {
            images: Vec::new(),
            padding: 1,
            max_size: 4096,
        }
    }
}

impl AtlasBuilder {
    /// An empty builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds an image under `name`. A second image with the same name
    /// replaces the first.
    pub fn add(&mut self, name: impl Into<String>, image: RgbaImage) -> &mut Self {
        let name = name.into();
        if let Some(slot) = self.images.iter_mut().find(|(n, _)| *n == name) {
            slot.1 = image;
        } else {
            self.images.push((name, image));
        }
        self
    }

    /// Packs the images and returns the combined image with each name's
    /// region, in the order the images were added.
    pub fn build_image(&self) -> Result<(RgbaImage, Vec<(String, Rect)>)> {
        let pad = self.padding;
        let mut order: Vec<usize> = (0..self.images.len()).collect();
        order.sort_by_key(|&i| std::cmp::Reverse(self.images[i].1.height()));
        let mut side = 64u32;
        loop {
            let mut packer = AtlasPacker::new(uvec2(side, side));
            let mut spots = vec![None; self.images.len()];
            let fits = order.iter().all(|&i| {
                let img = &self.images[i].1;
                let placed = packer.pack(uvec2(img.width() + 2 * pad, img.height() + 2 * pad));
                spots[i] = placed;
                placed.is_some()
            });
            if fits {
                let mut atlas = RgbaImage::new(side, side);
                let mut regions = Vec::with_capacity(self.images.len());
                for (i, (name, img)) in self.images.iter().enumerate() {
                    let Some(spot) = spots[i] else { continue };
                    let (x, y) = (spot.x + pad, spot.y + pad);
                    blit_extruded(&mut atlas, img, x, y, pad);
                    regions.push((
                        name.clone(),
                        Rect::new(x as f32, y as f32, img.width() as f32, img.height() as f32),
                    ));
                }
                return Ok((atlas, regions));
            }
            if side >= self.max_size {
                return Err(GfxError::Atlas(format!(
                    "{} images do not fit {m}x{m}",
                    self.images.len(),
                    m = self.max_size
                )));
            }
            side *= 2;
        }
    }

    /// Packs the images and uploads the result.
    pub fn build(&self, gfx: &Graphics, options: TextureOptions) -> Result<Atlas> {
        let (image, regions) = self.build_image()?;
        let texture = Texture::from_image(gfx, &image, options)?;
        Ok(Atlas::new(texture, regions))
    }
}

/// Copies `img` to `(x, y)` and repeats its edge texels `pad` texels out.
fn blit_extruded(atlas: &mut RgbaImage, img: &RgbaImage, x: u32, y: u32, pad: u32) {
    imageops::replace(atlas, img, i64::from(x), i64::from(y));
    if img.width() == 0 || img.height() == 0 {
        return;
    }
    for p in 1..=pad {
        for ix in 0..img.width() {
            atlas.put_pixel(x + ix, y - p, *img.get_pixel(ix, 0));
            atlas.put_pixel(
                x + ix,
                y + img.height() - 1 + p,
                *img.get_pixel(ix, img.height() - 1),
            );
        }
    }
    for p in 1..=pad {
        for iy in 0..img.height() + 2 * pad {
            let ay = y + iy - pad;
            atlas.put_pixel(x - p, ay, *atlas.get_pixel(x, ay));
            atlas.put_pixel(
                x + img.width() - 1 + p,
                ay,
                *atlas.get_pixel(x + img.width() - 1, ay),
            );
        }
    }
}

/// A texture with named rectangular regions.
#[derive(Clone, Debug)]
pub struct Atlas {
    texture: Texture,
    regions: HashMap<String, Rect>,
    names: Vec<String>,
}

impl Atlas {
    /// An atlas over `texture` with the given regions in texels.
    pub fn new(texture: Texture, regions: Vec<(String, Rect)>) -> Self {
        let names = regions.iter().map(|(n, _)| n.clone()).collect();
        Self {
            texture,
            regions: regions.into_iter().collect(),
            names,
        }
    }

    /// The atlas texture.
    pub fn texture(&self) -> &Texture {
        &self.texture
    }

    /// The region called `name`, in texels.
    pub fn region(&self, name: &str) -> Option<Rect> {
        self.regions.get(name).copied()
    }

    /// Region names in the order they were added.
    pub fn names(&self) -> &[String] {
        &self.names
    }

    /// Queues the region `name` placed by `sprite`. The sprite's source
    /// is replaced by the region. Does nothing for an unknown name.
    pub fn draw(&self, gfx: &mut Graphics, name: &str, sprite: &Sprite) {
        if let Some(src) = self.region(name) {
            gfx.draw_sprite(
                &self.texture,
                &Sprite {
                    source: Some(src),
                    ..*sprite
                },
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    fn overlaps(a: &PackRect, b: &PackRect) -> bool {
        a.intersects(b)
    }

    #[test]
    fn packing_has_no_overlaps_and_stays_inside() {
        // A deterministic spread of sizes.
        let mut seed = 12345u32;
        let mut next = move |lo: u32, hi: u32| {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            lo + (seed >> 16) % (hi - lo + 1)
        };
        for area in [128u32, 256, 512] {
            let mut packer = AtlasPacker::new(uvec2(area, area));
            let mut placed: Vec<PackRect> = Vec::new();
            for _ in 0..200 {
                let size = uvec2(next(4, 48), next(4, 48));
                if let Some(r) = packer.pack(size) {
                    assert_eq!((r.w, r.h), (size.x, size.y));
                    assert!(r.x + r.w <= area && r.y + r.h <= area, "{r:?} outside");
                    for p in &placed {
                        assert!(!overlaps(&r, p), "{r:?} overlaps {p:?}");
                    }
                    placed.push(r);
                }
            }
            assert!(placed.len() > 20, "area {area} placed {}", placed.len());
        }
    }

    #[test]
    fn a_full_area_refuses_more() {
        let mut p = AtlasPacker::new(uvec2(16, 16));
        assert!(p.pack(uvec2(16, 8)).is_some());
        assert!(p.pack(uvec2(16, 8)).is_some());
        assert!(p.pack(uvec2(1, 1)).is_none());
    }

    #[test]
    fn builder_places_images_with_extruded_edges() {
        let mut b = AtlasBuilder::new();
        let red = RgbaImage::from_pixel(10, 6, Rgba([255, 0, 0, 255]));
        let blue = RgbaImage::from_pixel(7, 20, Rgba([0, 0, 255, 255]));
        b.add("red", red).add("blue", blue);
        let (atlas, regions) = b.build_image().expect("fits");
        assert_eq!(regions.len(), 2);
        let get = |n: &str| regions.iter().find(|(m, _)| m == n).map(|r| r.1);
        let (r, bl) = (get("red").expect("red"), get("blue").expect("blue"));
        assert!(!r.intersects(&bl));
        assert_eq!(atlas.get_pixel(r.x as u32, r.y as u32).0, [255, 0, 0, 255]);
        // The padding texel left of the region repeats the edge.
        assert_eq!(
            atlas.get_pixel(r.x as u32 - 1, r.y as u32).0,
            [255, 0, 0, 255]
        );
        assert_eq!(
            atlas.get_pixel(bl.x as u32 + 6, bl.y as u32 + 20).0,
            [0, 0, 255, 255]
        );
    }

    #[test]
    fn builder_reports_images_that_cannot_fit() {
        let mut b = AtlasBuilder::new();
        b.max_size = 64;
        b.add("big", RgbaImage::new(100, 10));
        assert!(matches!(b.build_image(), Err(GfxError::Atlas(_))));
    }
}
