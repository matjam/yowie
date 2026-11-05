//! Post-processing a whole 2D frame: the HDR image an output's scenes
//! and lower 2D layers draw into, and the finishing steps from it into
//! the output.

use std::collections::HashMap;

use yowie_core::{UVec2, Vec2};

use super::PostRenderer;
use super::chain::{Extras, Input, Output};
use super::pipelines::Blend;
use super::settings::PostSettings;
use super::targets::{HDR, Image, PostTargets};
use super::uniform::{FLAG_BLOOM, FLAG_FLAT, PostUniform};

/// Frames an output's 2D post state may go unused before it is dropped.
const KEEP_FRAMES: u64 = 120;

/// One output's 2D post state.
#[derive(Debug)]
struct FrameTarget {
    image: Image,
    targets: PostTargets,
    used: u64,
}

/// The 2D post state of every output that asked for it, by output key
/// (0 for the screen, a render texture's id otherwise).
#[derive(Debug, Default)]
pub(crate) struct FramePost {
    outputs: HashMap<u64, FrameTarget>,
    frame: u64,
}

impl FramePost {
    /// Starts a frame, dropping the state of outputs unused for a while.
    pub(crate) fn begin_frame(&mut self) {
        self.frame += 1;
        let frame = self.frame;
        self.outputs.retain(|_, o| frame - o.used < KEEP_FRAMES);
    }
}

impl crate::Graphics {
    /// Post-processes the current output (the screen, or the render
    /// texture of an enclosing [`draw_to`](crate::Graphics::draw_to)) for
    /// this frame; `None` (the default) draws straight to it.
    ///
    /// The output's scenes and the 2D layers below
    /// [`layers::UI`](crate::layers::UI) draw into an HDR image; bloom,
    /// the grade, the lens effects and FXAA run over it into the output;
    /// the interface layers then draw on top, untouched. A 2D frame is not
    /// exposed or tone mapped, so with no effect on the frame keeps the
    /// colours it drew; the effects that need a 3D scene's depth (ambient
    /// occlusion, reflections, temporal anti-aliasing, depth of field,
    /// motion blur, light shafts) belong to the scene's own
    /// [`PostSettings`]. Call it every frame while it should apply, like
    /// the rest of the drawing.
    ///
    /// ```no_run
    /// # use yowie_gfx::{Graphics, scene::PostSettings};
    /// # fn f(gfx: &mut Graphics) {
    /// gfx.set_frame_post(PostSettings {
    ///     bloom: 0.3,
    ///     bloom_threshold: 0.9,
    ///     vignette: 0.3,
    ///     ..PostSettings::default()
    /// });
    /// # }
    /// ```
    pub fn set_frame_post(&mut self, settings: impl Into<Option<PostSettings>>) {
        let li = self.current_index();
        self.lists[li].post = settings.into();
    }
}

impl PostRenderer {
    /// The HDR image output `key` draws into this frame, `size` pixels
    /// large, made or remade as needed.
    pub(crate) fn frame_image(&mut self, device: &wgpu::Device, key: u64, size: UVec2) -> &Image {
        let frame = self.frames.frame;
        let (common, linear) = (&self.pipes.common, &self.linear);
        let make = || FrameTarget {
            image: Image::target(device, "yowie frame post", size, HDR),
            targets: PostTargets::new(device, common, linear, size),
            used: frame,
        };
        let o = self.frames.outputs.entry(key).or_insert_with(make);
        if o.targets.size != size {
            *o = make();
        }
        o.used = frame;
        &o.image
    }

    /// Records the 2D post chain of output `key` from its HDR image into
    /// `view` (all of it, replacing what it holds). Returns the draws.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn run_frame(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        key: u64,
        settings: &PostSettings,
        view: &wgpu::TextureView,
        format: wgpu::TextureFormat,
        time: f32,
    ) -> u32 {
        let Some(mut o) = self.frames.outputs.remove(&key) else {
            return 0;
        };
        let size = o.targets.size;
        let mut u = PostUniform::new(
            settings,
            Vec2::new(size.x as f32, size.y as f32),
            [0.0; 4],
            time,
        );
        u.add_flags(FLAG_FLAT);
        if settings.wants_bloom() {
            u.add_flags(FLAG_BLOOM);
            u.bloom[0] /= super::chain::bloom_levels(size) as f32;
        }
        queue.write_buffer(&o.targets.uniform, 0, bytemuck::bytes_of(&u));
        let src_view = o.image.view.clone();
        let src = Input {
            view: &src_view,
            id: o.image.id,
        };
        let draws = self.finish(
            device,
            encoder,
            &mut o.targets,
            settings,
            src,
            &Extras::default(),
            Output {
                view,
                format,
                viewport: None,
                load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                blend: Blend::Replace,
            },
        );
        self.frames.outputs.insert(key, o);
        draws
    }
}
