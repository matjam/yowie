//! Offscreen images that draw like the screen and sample like a texture.

use image::RgbaImage;
use yowie_core::{Color, UVec2, Vec2, uvec2};

use crate::Graphics;
use crate::batch::{DrawList, ListTarget};
use crate::error::{GfxError, Result};
use crate::output::OFFSCREEN_FORMAT;
use crate::readback;
use crate::texture::{Filter, Purpose, Texture, TextureOptions};
use crate::view::ViewMapping;

/// Options for a render texture.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RenderTextureOptions {
    /// Sampling when the result is drawn scaled; nearest keeps a
    /// low-resolution scene's pixels sharp.
    pub filter: Filter,
    /// The size of the coordinate space drawing into it uses, in view
    /// units; `None` means one unit per pixel.
    pub view_size: Option<Vec2>,
}

/// An offscreen image to draw into with [`Graphics::draw_to`] and then draw
/// with [`RenderTexture::texture`], for minimaps, mirrors, portraits or a
/// whole low-resolution scene. Its contents are premultiplied sRGB, like
/// any texture. Cloning shares the image.
#[derive(Clone, Debug)]
pub struct RenderTexture {
    texture: Texture,
    view_size: Option<Vec2>,
}

impl RenderTexture {
    /// Creates a transparent render texture of `width` by `height` pixels.
    pub fn new(
        gfx: &Graphics,
        width: u32,
        height: u32,
        options: RenderTextureOptions,
    ) -> Result<Self> {
        let texture = Texture::create(
            &gfx.shared,
            uvec2(width, height),
            OFFSCREEN_FORMAT,
            TextureOptions {
                filter: options.filter,
                premultiplied: true,
                ..TextureOptions::default()
            },
            Purpose::RenderTarget,
        )?;
        Ok(Self {
            texture,
            view_size: options.view_size,
        })
    }

    /// The texture to draw the result with.
    pub fn texture(&self) -> &Texture {
        &self.texture
    }

    /// The size in pixels.
    pub fn size(&self) -> UVec2 {
        self.texture.size()
    }

    fn mapping(&self) -> ViewMapping {
        ViewMapping::filling(self.texture.size(), self.view_size)
    }

    /// Copies the pixels back with straight alpha. Call it outside a frame
    /// being recorded, after [`Graphics::end_frame`] rendered the drawing.
    pub fn read(&self, gfx: &Graphics) -> Result<RgbaImage> {
        let device = gfx.shared.device();
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("yowie render texture readback"),
        });
        let pending = readback::record(device, &mut encoder, &self.texture.gpu().texture)?;
        gfx.shared.queue().submit([encoder.finish()]);
        pending.finish(device, true)
    }
}

impl Graphics {
    /// Runs `f` with `target` as the output, then returns to the previous
    /// one. With `clear`, the target starts this pass cleared to that
    /// colour; without, drawing goes over what it held. Inside, the layer,
    /// sort key, blend mode, shader, camera and clip start at their
    /// defaults, and the previous ones return afterwards.
    ///
    /// Render textures are rendered before the screen, in the order their
    /// `draw_to` scopes finish, so the result can be drawn in the same
    /// frame, and a target drawn inside another target's scope is ready
    /// before the outer one.
    pub fn draw_to<R>(
        &mut self,
        target: &RenderTexture,
        clear: Option<Color>,
        f: impl FnOnce(&mut Self) -> R,
    ) -> R {
        let saved = std::mem::take(&mut self.cursor);
        let index = self.lists.len();
        self.lists.push(DrawList::new(
            ListTarget::Texture(target.texture.clone()),
            target.mapping(),
            clear,
        ));
        self.list_stack.push(index);
        let r = f(self);
        self.list_stack.pop();
        self.encode_order.push(index);
        self.cursor = saved;
        r
    }

    /// Copies the frame's output back after the frame rendered. Headless
    /// renderers keep their last frame, so this works after
    /// [`end_frame`](Self::end_frame); a window surface needs
    /// [`request_capture`](Self::request_capture) instead.
    pub fn read_output(&self) -> Result<RgbaImage> {
        match &self.output {
            crate::output::Output::Headless { texture } => {
                let device = self.shared.device();
                let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("yowie output readback"),
                });
                let pending = readback::record(device, &mut encoder, texture)?;
                self.shared.queue().submit([encoder.finish()]);
                pending.finish(device, false)
            }
            crate::output::Output::Surface { .. } => Err(GfxError::FrameState(
                "a window's frames are read with request_capture and take_capture",
            )),
        }
    }
}
