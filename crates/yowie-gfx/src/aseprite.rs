//! Aseprite files (`.ase` and `.aseprite`): flattened frames, tags,
//! slices and per-frame durations.

use asefile::{AnimationDirection, AsepriteFile};
use image::RgbaImage;
use yowie_anim::flipbook::{FlipFrame, Tag};
use yowie_anim::{Flipbook, TagDirection};
use yowie_asset::Vfs;
use yowie_core::{Rect, UVec2, Vec2, uvec2, vec2};

use crate::error::{GfxError, Result};
use crate::sheet::{SheetGrid, SpriteSheet};
use crate::{Graphics, Sprite, Texture, TextureOptions};

/// A named range of frames in an Aseprite file.
#[derive(Clone, Debug, PartialEq)]
pub struct AseTag {
    /// The tag's name.
    pub name: String,
    /// Index of the first frame.
    pub from: usize,
    /// Index of the last frame, inclusive.
    pub to: usize,
    /// The order the frames play in.
    pub direction: TagDirection,
    /// How many times the tag repeats; `None` means forever.
    pub repeat: Option<u32>,
}

/// One position and shape of a slice, valid from `from_frame` until the
/// next key.
#[derive(Clone, Debug, PartialEq)]
pub struct AseSliceKey {
    /// First frame this key applies to.
    pub from_frame: usize,
    /// The slice rectangle in sprite pixels.
    pub bounds: Rect,
    /// The pivot relative to the slice's top-left, if set.
    pub pivot: Option<Vec2>,
    /// The nine-slice centre rectangle relative to the slice's top-left,
    /// if the slice has nine-slice data.
    pub center: Option<Rect>,
}

/// A named region of the sprite, such as a hit box or an attachment point.
#[derive(Clone, Debug, PartialEq)]
pub struct AseSlice {
    /// The slice's name.
    pub name: String,
    /// Its keys in frame order.
    pub keys: Vec<AseSliceKey>,
}

impl AseSlice {
    /// The key that applies at `frame`.
    pub fn key_at(&self, frame: usize) -> Option<&AseSliceKey> {
        self.keys.iter().rev().find(|k| k.from_frame <= frame)
    }
}

/// The contents of an Aseprite file as plain data, with every frame
/// flattened to one RGBA image. It needs no GPU.
#[derive(Clone, Debug)]
pub struct AsepriteData {
    /// The sprite's size in pixels.
    pub size: UVec2,
    /// One flattened image per frame, in sRGB with straight alpha.
    pub frames: Vec<RgbaImage>,
    /// How long each frame shows, in seconds.
    pub durations: Vec<f32>,
    /// The file's animation tags.
    pub tags: Vec<AseTag>,
    /// The file's slices.
    pub slices: Vec<AseSlice>,
}

impl AsepriteData {
    /// Reads an Aseprite file from memory. Visible layers are flattened
    /// with their blend modes and opacity; indexed and grayscale files are
    /// converted to RGBA.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let file = AsepriteFile::read(bytes).map_err(|e| GfxError::Aseprite(e.to_string()))?;
        Ok(Self::from_file(&file))
    }

    /// Reads the Aseprite file at `path` in `vfs`.
    pub fn load(vfs: &Vfs, path: &str) -> Result<Self> {
        let bytes = vfs
            .read(path)
            .map_err(|e| GfxError::Aseprite(format!("{path}: {e}")))?;
        Self::from_bytes(&bytes).map_err(|e| GfxError::Aseprite(format!("{path}: {e}")))
    }

    fn from_file(file: &AsepriteFile) -> Self {
        let (w, h) = (file.width() as u32, file.height() as u32);
        let mut frames = Vec::new();
        let mut durations = Vec::new();
        for i in 0..file.num_frames() {
            let frame = file.frame(i);
            let img = frame.image();
            let (iw, ih) = (img.width(), img.height());
            // asefile builds its images with another version of the image
            // crate, so the pixels cross as a plain byte buffer.
            frames.push(
                RgbaImage::from_raw(iw, ih, img.into_raw()).unwrap_or_else(|| RgbaImage::new(w, h)),
            );
            durations.push(frame.duration() as f32 / 1000.0);
        }
        let tags = (0..file.num_tags())
            .filter_map(|i| file.get_tag(i))
            .map(|t| AseTag {
                name: t.name().to_string(),
                from: t.from_frame() as usize,
                to: t.to_frame() as usize,
                direction: match t.animation_direction() {
                    AnimationDirection::Forward => TagDirection::Forward,
                    AnimationDirection::Reverse => TagDirection::Reverse,
                    AnimationDirection::PingPong => TagDirection::PingPong,
                },
                repeat: t.repeat().map(|r| r.get()),
            })
            .collect();
        let slices = file
            .slices()
            .iter()
            .map(|s| AseSlice {
                name: s.name.clone(),
                keys: s
                    .keys
                    .iter()
                    .map(|k| AseSliceKey {
                        from_frame: k.from_frame as usize,
                        bounds: Rect::new(
                            k.origin.0 as f32,
                            k.origin.1 as f32,
                            k.size.0 as f32,
                            k.size.1 as f32,
                        ),
                        pivot: k.pivot.map(|p| vec2(p.0 as f32, p.1 as f32)),
                        center: k.slice9.as_ref().map(|c| {
                            Rect::new(
                                c.center_x as f32,
                                c.center_y as f32,
                                c.center_width as f32,
                                c.center_height as f32,
                            )
                        }),
                    })
                    .collect(),
            })
            .collect();
        Self {
            size: uvec2(w, h),
            frames,
            durations,
            tags,
            slices,
        }
    }

    /// The animation as a flipbook whose frame indices are indices into
    /// [`frames`](Self::frames) and whose tags are the file's tags. Play it
    /// with [`yowie_anim::FlipbookPlayer`].
    pub fn flipbook(&self) -> Flipbook {
        Flipbook {
            frames: self
                .durations
                .iter()
                .enumerate()
                .map(|(index, &duration)| FlipFrame { index, duration })
                .collect(),
            tags: self
                .tags
                .iter()
                .map(|t| Tag {
                    name: t.name.clone(),
                    from: t.from,
                    to: t.to,
                    direction: t.direction,
                })
                .collect(),
        }
    }

    /// The tag called `name`.
    pub fn tag(&self, name: &str) -> Option<&AseTag> {
        self.tags.iter().find(|t| t.name == name)
    }

    /// The slice called `name`.
    pub fn slice(&self, name: &str) -> Option<&AseSlice> {
        self.slices.iter().find(|s| s.name == name)
    }

    /// The frame indices a tag plays once through, in play order. A
    /// ping-pong tag goes forward then back without repeating either end.
    pub fn tag_frames(&self, name: &str) -> Option<Vec<usize>> {
        let t = self.tag(name)?;
        let last = self.frames.len().checked_sub(1)?;
        let (from, to) = (t.from.min(last), t.to.min(last));
        let forward: Vec<usize> = (from..=to).collect();
        Some(match t.direction {
            TagDirection::Forward => forward,
            TagDirection::Reverse => forward.into_iter().rev().collect(),
            TagDirection::PingPong => {
                let back = (from + 1..to).rev();
                forward.iter().copied().chain(back).collect()
            }
        })
    }

    /// Packs the frames into one grid image, as columns wide as needed
    /// for a roughly square result, with the grid describing where each
    /// frame sits.
    pub fn pack_grid(&self) -> (RgbaImage, SheetGrid) {
        let n = self.frames.len().max(1) as u32;
        let columns = (n as f32).sqrt().ceil() as u32;
        let rows = n.div_ceil(columns);
        let mut out = RgbaImage::new(columns * self.size.x.max(1), rows * self.size.y.max(1));
        for (i, f) in self.frames.iter().enumerate() {
            let (c, r) = (i as u32 % columns, i as u32 / columns);
            image::imageops::replace(
                &mut out,
                f,
                i64::from(c * self.size.x),
                i64::from(r * self.size.y),
            );
        }
        let grid = SheetGrid {
            frame_size: self.size,
            columns,
            rows,
            margin: 0,
            spacing: 0,
        };
        (out, grid)
    }
}

/// An Aseprite file uploaded as a sprite sheet with its animation.
#[derive(Clone, Debug)]
pub struct AsepriteSheet {
    /// The frames as a sprite sheet; frame `i` is the file's frame `i`.
    pub sheet: SpriteSheet,
    /// The file's contents.
    pub data: AsepriteData,
}

impl AsepriteSheet {
    /// Uploads the frames of `data`.
    pub fn new(gfx: &Graphics, data: AsepriteData, options: TextureOptions) -> Result<Self> {
        let (image, grid) = data.pack_grid();
        let texture = Texture::from_image(gfx, &image, options)?;
        Ok(Self {
            sheet: SpriteSheet::from_grid(texture, grid),
            data,
        })
    }

    /// Reads and uploads the file at `path` in `vfs`.
    pub fn load(gfx: &Graphics, vfs: &Vfs, path: &str, options: TextureOptions) -> Result<Self> {
        Self::new(gfx, AsepriteData::load(vfs, path)?, options)
    }

    /// The animation as a flipbook over this sheet's frames.
    pub fn flipbook(&self) -> Flipbook {
        self.data.flipbook()
    }

    /// Queues frame `index` placed by `sprite`.
    pub fn draw_frame(&self, gfx: &mut Graphics, index: usize, sprite: &Sprite) {
        self.sheet.draw_frame(gfx, index as u32, sprite);
    }
}

#[cfg(test)]
#[path = "aseprite_tests.rs"]
mod tests;
