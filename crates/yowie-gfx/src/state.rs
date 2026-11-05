//! Drawing state: layer, sort key, blend mode, shader, camera and clip,
//! with closure scopes that restore it.

use yowie_core::{Affine2, Color, Mat4, Rect, Vec2, Vec4};

use crate::Graphics;
use crate::batch::{DEFAULT_SHADER, DrawState, ViewEntry};
use crate::blend::BlendMode;
use crate::camera::Camera2D;
use crate::pipelines::{Vertex2D, ViewUniform};
use crate::shader::Shader;
use crate::texture::Texture;

/// Maps `0..width` to clip-space X `-1..1` and `0..height` to clip-space Y
/// `1..-1`, so +Y points down the screen; Z passes through.
pub(crate) fn ortho_2d(width: f32, height: f32) -> Mat4 {
    let (w, h) = (width.max(1e-6), height.max(1e-6));
    Mat4::from_cols(
        Vec4::new(2.0 / w, 0.0, 0.0, 0.0),
        Vec4::new(0.0, -2.0 / h, 0.0, 0.0),
        Vec4::new(0.0, 0.0, 1.0, 0.0),
        Vec4::new(-1.0, 1.0, 0.0, 1.0),
    )
}

/// The state later draws are recorded with.
#[derive(Clone, Debug, Default)]
pub(crate) struct Cursor {
    pub(crate) layer: i32,
    pub(crate) sort_key: f32,
    pub(crate) blend: BlendMode,
    pub(crate) shader: Option<Shader>,
    pub(crate) camera: Option<Camera2D>,
    pub(crate) clip: Option<Rect>,
    /// The view slot for the current list, camera and clip, once computed.
    pub(crate) view: Option<u32>,
    /// The visible area matching `view`; valid while `view` is `Some`.
    pub(crate) visible: Rect,
    /// The last texture looked up and its slot.
    pub(crate) last_texture: Option<(u64, u32)>,
    /// The transform applied to drawing coordinates before the camera.
    pub(crate) transform: Affine2,
    /// Transforms saved by `push_transform`.
    pub(crate) transform_stack: Vec<Affine2>,
}

impl Graphics {
    /// Sets the layer later draws go on. Layers draw in ascending order;
    /// within a layer, by sort key and then in call order. The default is 0.
    pub fn set_layer(&mut self, layer: i32) {
        self.cursor.layer = layer;
    }

    /// The layer draws currently go on.
    pub fn layer(&self) -> i32 {
        self.cursor.layer
    }

    /// Runs `f` with draws on `layer`, then restores the previous layer.
    pub fn with_layer<R>(&mut self, layer: i32, f: impl FnOnce(&mut Self) -> R) -> R {
        let old = std::mem::replace(&mut self.cursor.layer, layer);
        let r = f(self);
        self.cursor.layer = old;
        r
    }

    /// Sets the sort key for later draws: within a layer, lower keys draw
    /// first. A top-down game sets it to each sprite's foot position. The
    /// default is 0.
    pub fn set_sort_key(&mut self, key: f32) {
        self.cursor.sort_key = key;
    }

    /// The current sort key.
    pub fn sort_key(&self) -> f32 {
        self.cursor.sort_key
    }

    /// Sets the blend mode for later draws.
    pub fn set_blend(&mut self, blend: BlendMode) {
        self.cursor.blend = blend;
    }

    /// The current blend mode.
    pub fn blend(&self) -> BlendMode {
        self.cursor.blend
    }

    /// Runs `f` with `blend`, then restores the previous mode.
    pub fn with_blend<R>(&mut self, blend: BlendMode, f: impl FnOnce(&mut Self) -> R) -> R {
        let old = std::mem::replace(&mut self.cursor.blend, blend);
        let r = f(self);
        self.cursor.blend = old;
        r
    }

    /// Sets the game shader later draws use; `None` returns to the engine's.
    pub fn set_shader(&mut self, shader: Option<&Shader>) {
        self.cursor.shader = shader.cloned();
    }

    /// Runs `f` drawing through `shader`, then restores the previous one.
    pub fn with_shader<R>(&mut self, shader: &Shader, f: impl FnOnce(&mut Self) -> R) -> R {
        let old = self.cursor.shader.replace(shader.clone());
        let r = f(self);
        self.cursor.shader = old;
        r
    }

    /// Sets the camera later draws go through; `None` draws in view units.
    pub fn set_camera(&mut self, camera: Option<Camera2D>) {
        self.cursor.camera = camera;
        self.cursor.view = None;
    }

    /// The current camera.
    pub fn camera(&self) -> Option<Camera2D> {
        self.cursor.camera
    }

    /// Runs `f` drawing through `camera`, then restores the previous camera.
    pub fn with_camera<R>(&mut self, camera: &Camera2D, f: impl FnOnce(&mut Self) -> R) -> R {
        let old = self.cursor.camera.replace(*camera);
        self.cursor.view = None;
        let r = f(self);
        self.cursor.camera = old;
        self.cursor.view = None;
        r
    }

    /// Limits later drawing to `clip`, a rectangle in view units that the
    /// camera does not move; `None` removes the limit.
    pub fn set_clip(&mut self, clip: Option<Rect>) {
        self.cursor.clip = clip;
        self.cursor.view = None;
    }

    /// Runs `f` with drawing limited to `clip` (in view units) inside any
    /// clip already set, then restores the previous clip.
    pub fn with_clip<R>(&mut self, clip: Rect, f: impl FnOnce(&mut Self) -> R) -> R {
        let inner = match self.cursor.clip {
            Some(outer) => outer.intersection(&clip).unwrap_or(Rect::ZERO),
            None => clip,
        };
        let old = self.cursor.clip.replace(inner);
        self.cursor.view = None;
        let r = f(self);
        self.cursor.clip = old;
        self.cursor.view = None;
        r
    }

    /// The colour as the vertex stream stores it: linear, and premultiplied
    /// unless the blend mode takes premultiplied tints.
    pub(crate) fn vertex_color(&self, color: Color) -> [f32; 4] {
        if self.cursor.blend.premultiplies_tint() {
            color.to_linear_premultiplied()
        } else {
            color.to_linear()
        }
    }

    /// The drawing-space rectangle on screen for the current output and
    /// camera, cached with the view slot.
    pub(crate) fn cached_visible_area(&mut self) -> Rect {
        self.view_slot();
        self.cursor.visible
    }

    fn view_slot(&mut self) -> u32 {
        if let Some(v) = self.cursor.view {
            return v;
        }
        self.cursor.visible = self.visible_area();
        let m = self.current_list().mapping;
        let full = Rect::new(0.0, 0.0, m.view_size.x, m.view_size.y);
        let (area, view) = match &self.cursor.camera {
            Some(c) => {
                let vp = c.viewport_rect(m.view_size);
                (vp, c.view_matrix(vp.size()))
            }
            None => (full, Mat4::IDENTITY),
        };
        let viewport = m.rect_to_pixels(area);
        let out = m.output_size.as_vec2();
        let mut sc = viewport
            .intersection(&Rect::new(0.0, 0.0, out.x, out.y))
            .unwrap_or(Rect::ZERO);
        if let Some(clip) = self.cursor.clip {
            sc = sc
                .intersection(&m.rect_to_pixels(clip))
                .unwrap_or(Rect::ZERO);
        }
        let x0 = sc.x.max(0.0).floor();
        let y0 = sc.y.max(0.0).floor();
        let x1 = (sc.x + sc.w).min(out.x).ceil().max(x0);
        let y1 = (sc.y + sc.h).min(out.y).ceil().max(y0);
        let proj = ortho_2d(area.w, area.h);
        let entry = ViewEntry {
            uniform: ViewUniform {
                view_proj: (proj * view).to_cols_array_2d(),
                view_size: m.view_size.to_array(),
                pixel_scale: m.pixels_per_unit().x,
                time: self.time,
            },
            viewport,
            scissor: [x0 as u32, y0 as u32, (x1 - x0) as u32, (y1 - y0) as u32],
        };
        let slot = self.tables.push_view(entry);
        self.cursor.view = Some(slot);
        slot
    }

    pub(crate) fn draw_state(&mut self, texture: &Texture) -> DrawState {
        let view = self.view_slot();
        let texture = match self.cursor.last_texture {
            Some((id, slot)) if id == texture.id() => slot,
            _ => {
                let slot = self.tables.texture_slot(texture);
                self.cursor.last_texture = Some((texture.id(), slot));
                slot
            }
        };
        let shader = match &self.cursor.shader {
            Some(s) => self.tables.shader_slot(s),
            None => DEFAULT_SHADER,
        };
        DrawState {
            texture,
            shader,
            blend: self.cursor.blend,
            view,
        }
    }

    /// Queues triangles with `texture`; `indices` count from the first of
    /// `vertices`. Nothing is queued when the clip leaves no area.
    pub(crate) fn push_triangles(
        &mut self,
        texture: &Texture,
        vertices: &[Vertex2D],
        indices: &[u32],
    ) {
        self.push_triangles_with(texture, vertices, indices, None);
    }

    /// Like `push_triangles`, drawing through one of the engine's own
    /// shader variants (`SDF_SHADER`, `RADIAL_SHADER`) when `engine` is set.
    pub(crate) fn push_triangles_with(
        &mut self,
        texture: &Texture,
        vertices: &[Vertex2D],
        indices: &[u32],
        engine: Option<u32>,
    ) {
        let mut state = self.draw_state(texture);
        if let Some(shader) = engine {
            state.shader = shader;
        }
        let sc = self.tables.views[state.view as usize].scissor;
        if sc[2] == 0 || sc[3] == 0 {
            return;
        }
        let (layer, key) = (self.cursor.layer, self.cursor.sort_key);
        let i = self.current_index();
        if self.cursor.transform == Affine2::IDENTITY {
            self.lists[i].push(state, layer, key, vertices, indices);
        } else {
            let t = self.cursor.transform;
            let moved: Vec<Vertex2D> = vertices
                .iter()
                .map(|v| Vertex2D {
                    position: t.transform_point2(Vec2::from(v.position)).to_array(),
                    ..*v
                })
                .collect();
            self.lists[i].push(state, layer, key, &moved, indices);
        }
    }

    /// Queues one quad from four corners (clockwise from top-left) and
    /// their texture coordinates.
    pub(crate) fn push_quad(
        &mut self,
        texture: &Texture,
        corners: [Vec2; 4],
        uvs: [Vec2; 4],
        color: [f32; 4],
        mode: u32,
    ) {
        let v = std::array::from_fn::<Vertex2D, 4, _>(|i| Vertex2D {
            position: corners[i].to_array(),
            uv: uvs[i].to_array(),
            color,
            mode,
        });
        self.push_triangles(texture, &v, &[0, 1, 2, 0, 2, 3]);
    }
}
