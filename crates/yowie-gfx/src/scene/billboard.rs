//! Billboards: textured quads in the scene that turn to face the camera,
//! either fully or about the up axis only (trees, characters). They are
//! instanced per style, lit and shadowed like meshes when asked, and cut
//! out or blended. Impostors draw through the same class.

use bytemuck::{Pod, Zeroable};
use yowie_core::{Color, Rect, Vec2, Vec3};

use super::material::AlphaMode;
use super::record::FastMap;
use super::submit::Scene;
use crate::texture::Texture;

pub(crate) use super::billboard_render::{BillboardFrame, encode, encode_shadow, prepare};

/// How a billboard turns towards the camera.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Facing {
    /// Faces the camera's view plane, so a field of billboards turns
    /// together: smoke, glows, markers.
    #[default]
    Camera,
    /// Turns about the world's up axis only towards the camera, staying
    /// upright when seen from above: trees, characters.
    Upright,
}

/// The look a batch of billboards shares: one draw call per style and
/// pass draws every billboard with it.
#[derive(Clone, Debug, PartialEq)]
pub struct BillboardStyle {
    /// The image, sRGB; `None` draws the tint alone.
    pub texture: Option<Texture>,
    /// A normal map in the quad's own frame (X right, Y up, Z towards
    /// the viewer), created with `TextureOptions::data`; `None` lights
    /// the quad as flat.
    pub normal_texture: Option<Texture>,
    /// Cut out at a threshold (writes depth, casts shadows) or blended
    /// over the scene. `Opaque` draws the whole quad.
    pub alpha: AlphaMode,
    /// Lights the quads with the scene's lights; otherwise they show their
    /// colour as it is.
    pub lit: bool,
    /// Roughness when lit.
    pub roughness: f32,
    /// Whether cut-out and opaque billboards cast shadows.
    pub cast_shadows: bool,
}

impl Default for BillboardStyle {
    fn default() -> Self {
        Self {
            texture: None,
            normal_texture: None,
            alpha: AlphaMode::Mask(0.5),
            lit: true,
            roughness: 0.9,
            cast_shadows: true,
        }
    }
}

impl BillboardStyle {
    /// A lit, cut-out style showing `texture`.
    pub fn textured(texture: &Texture) -> Self {
        Self {
            texture: Some(texture.clone()),
            ..Self::default()
        }
    }
}

/// One billboard: where it stands, how large it is and which part of the
/// style's texture it shows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Billboard {
    /// The world point the pivot sits on.
    pub position: Vec3,
    /// Width and height in world units.
    pub size: Vec2,
    /// The point of the quad placed at `position`, in fractions of its
    /// size from the bottom-left corner: (0.5, 0.5) is the centre and
    /// (0.5, 0) the middle of the bottom edge, for something standing on
    /// the ground.
    pub pivot: Vec2,
    /// How the quad turns towards the camera.
    pub facing: Facing,
    /// Multiplies the texture, sRGB with straight alpha.
    pub color: Color,
    /// The part of the texture shown, in texture coordinates (0 to 1,
    /// origin top-left).
    pub region: Rect,
}

impl Billboard {
    /// A camera-facing billboard centred on `position`.
    pub fn new(position: Vec3, size: Vec2) -> Self {
        Self {
            position,
            size,
            pivot: Vec2::splat(0.5),
            facing: Facing::Camera,
            color: Color::WHITE,
            region: Rect::new(0.0, 0.0, 1.0, 1.0),
        }
    }

    /// An upright billboard standing on `position` (its bottom edge's
    /// middle), for trees and characters.
    pub fn upright(position: Vec3, size: Vec2) -> Self {
        Self {
            pivot: Vec2::new(0.5, 0.0),
            facing: Facing::Upright,
            ..Self::new(position, size)
        }
    }

    /// The same billboard tinted `color`.
    pub fn tinted(self, color: Color) -> Self {
        Self { color, ..self }
    }

    /// The same billboard showing `region` of the texture.
    pub fn with_region(self, region: Rect) -> Self {
        Self { region, ..self }
    }
}

/// The instance record the billboard programs read; matches
/// `BillboardIn` in `shaders/billboard.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub(crate) struct GpuBillboard {
    pub(crate) position: [f32; 3],
    /// 1: upright, 2: impostor.
    pub(crate) flags: u32,
    /// Size, then pivot.
    pub(crate) size_pivot: [f32; 4],
    /// u0, v0, u1, v1.
    pub(crate) uv: [f32; 4],
    /// Linear colour, straight alpha.
    pub(crate) color: [f32; 4],
    /// The fraction of pixels shown (1 for all), and an impostor's yaw.
    pub(crate) fade_yaw: [f32; 2],
    /// Shadow view, unused.
    pub(crate) view: [u32; 2],
}

pub(crate) const FLAG_UPRIGHT: u32 = 1;
pub(crate) const FLAG_IMPOSTOR: u32 = 2;

/// An impostor atlas's layout: views, columns, rows and pitch.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct AtlasGrid {
    pub(crate) views: u32,
    pub(crate) cols: u32,
    pub(crate) rows: u32,
    pub(crate) pitch: f32,
}

/// One style as the renderer batches it.
#[derive(Clone, Debug)]
pub(crate) struct Sheet {
    pub(crate) albedo: Option<Texture>,
    pub(crate) normal: Option<Texture>,
    pub(crate) cutoff: f32,
    pub(crate) blend: bool,
    pub(crate) lit: bool,
    pub(crate) roughness: f32,
    pub(crate) shadows: bool,
    pub(crate) grid: Option<AtlasGrid>,
}

impl Sheet {
    pub(crate) fn key(&self) -> SheetKey {
        let id = |t: &Option<Texture>| t.as_ref().map_or(0, Texture::id);
        let g = self
            .grid
            .map_or([0; 4], |g| [g.views, g.cols, g.rows, g.pitch.to_bits()]);
        SheetKey {
            textures: [id(&self.albedo), id(&self.normal)],
            bits: [
                self.cutoff.to_bits(),
                u32::from(self.blend) | u32::from(self.lit) << 1 | u32::from(self.shadows) << 2,
                self.roughness.to_bits(),
                g[0],
                g[1],
                g[2],
                g[3],
            ],
        }
    }

    fn from_style(style: &BillboardStyle) -> Self {
        let (cutoff, blend) = match style.alpha {
            AlphaMode::Opaque => (-1.0, false),
            AlphaMode::Mask(c) => (c, false),
            AlphaMode::Blend => (-1.0, true),
        };
        Self {
            albedo: style.texture.clone(),
            normal: style.normal_texture.clone(),
            cutoff,
            blend,
            lit: style.lit,
            roughness: style.roughness,
            shadows: style.cast_shadows && !blend,
            grid: None,
        }
    }
}

/// Identifies a sheet within a frame and across frames.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct SheetKey {
    textures: [u64; 2],
    bits: [u32; 7],
}

/// The billboards one scene recorded.
#[derive(Debug, Default)]
pub(crate) struct BillboardRecord {
    pub(crate) sheets: Vec<Sheet>,
    slots: FastMap<SheetKey, u32>,
    /// Each instance and its sheet.
    pub(crate) instances: Vec<(u32, GpuBillboard)>,
}

impl BillboardRecord {
    pub(crate) fn clear(&mut self) {
        self.sheets.clear();
        self.slots.clear();
        self.instances.clear();
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.instances.is_empty()
    }

    /// The index of `sheet` in this frame, added when new.
    pub(crate) fn sheet_slot(&mut self, sheet: Sheet) -> u32 {
        let next = self.sheets.len() as u32;
        let slot = *self.slots.entry(sheet.key()).or_insert(next);
        if slot == next {
            self.sheets.push(sheet);
        }
        slot
    }
}

pub(crate) fn gpu_billboard(b: &Billboard) -> GpuBillboard {
    let r = b.region;
    GpuBillboard {
        position: b.position.to_array(),
        flags: if b.facing == Facing::Upright {
            FLAG_UPRIGHT
        } else {
            0
        },
        size_pivot: [b.size.x, b.size.y, b.pivot.x, b.pivot.y],
        uv: [r.x, r.y, r.x + r.w, r.y + r.h],
        color: b.color.to_linear(),
        fade_yaw: [1.0, 0.0],
        view: [0, 0],
    }
}

impl Scene<'_> {
    /// Draws one billboard in `style`.
    pub fn draw_billboard(&mut self, style: &BillboardStyle, billboard: Billboard) {
        self.draw_billboards(style, std::slice::from_ref(&billboard));
    }

    /// Draws billboards in `style`. All billboards of one style in a scene
    /// are one instanced draw call; the frustum culls those out of view.
    pub fn draw_billboards(&mut self, style: &BillboardStyle, billboards: &[Billboard]) {
        if billboards.is_empty() {
            return;
        }
        let rec = &mut self.rec.large.billboards;
        let sheet = rec.sheet_slot(Sheet::from_style(style));
        rec.instances
            .extend(billboards.iter().map(|b| (sheet, gpu_billboard(b))));
    }
}
