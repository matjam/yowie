//! Drawing a GPU system through the renderer's custom draw hook.

use std::sync::Mutex;

use yowie_gfx::{Draw2D, Draw3D, GpuDraw, PrepareContext};

use super::pool::Core;

/// The part of a [`GpuSystem`](super::GpuSystem) the renderer holds on to
/// between `draw` and the end of the frame.
#[derive(Debug)]
pub(crate) struct Inner {
    pub(crate) core: Mutex<Core>,
    /// Whether the device can draw with arguments written by the GPU.
    pub(crate) indirect: bool,
}

impl Inner {
    pub(crate) fn lock(&self) -> std::sync::MutexGuard<'_, Core> {
        self.core.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn record_draw(&self, pass: &mut wgpu::RenderPass<'_>, core: &Core) {
        pass.set_bind_group(1, &core.data_group, &[]);
        if self.indirect {
            pass.draw_indirect(&core.header, 0);
        } else {
            // Without indirect draws every slot gets an instance; the
            // vertex program collapses the ones past the alive count.
            pass.draw(0..6, 0..core.capacity);
        }
    }
}

impl GpuDraw for Inner {
    fn prepare(&self, ctx: &mut PrepareContext<'_>) {
        self.lock().record(ctx.encoder);
    }

    fn draw_2d(&self, pass: &mut wgpu::RenderPass<'_>, ctx: &Draw2D<'_>) {
        let core = self.lock();
        let pipeline =
            core.programs
                .pipeline(&core.device, ctx.view_layout, false, ctx.format, core.blend);
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, ctx.view_group, &[ctx.view_offset]);
        self.record_draw(pass, &core);
    }

    fn draw_3d(&self, pass: &mut wgpu::RenderPass<'_>, ctx: &Draw3D<'_>) {
        let core = self.lock();
        let pipeline = core.programs.pipeline(
            &core.device,
            ctx.frame_layout,
            true,
            ctx.color_format,
            core.blend,
        );
        pass.set_pipeline(&pipeline);
        self.record_draw(pass, &core);
    }
}
