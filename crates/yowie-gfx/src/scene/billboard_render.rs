//! Drawing recorded billboards: culling, grouping by style into instance
//! runs, the style bind groups (cached across frames), and recording one
//! draw call per style and pass.

use std::collections::HashMap;
use std::sync::Arc;

use bytemuck::{Pod, Zeroable};

use super::billboard::{GpuBillboard, Sheet, SheetKey};
use super::large::{LargeRecord, PrepareContext, instance_buffer};
use super::large_pipelines::LargePipelines;
use super::slot::GrowBuffer;
use crate::graphics::FrameStats;
use crate::texture_gpu::TextureGpu;

/// Frames a style's bind group may go unused before it is dropped.
const KEEP_FRAMES: u64 = 120;

/// The style block the shaders read; matches `Sheet` in
/// `shaders/billboard.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
struct SheetGpu {
    /// Views, columns, rows, pitch of an impostor atlas.
    grid: [f32; 4],
    /// Alpha cutoff (negative for none), roughness, 1 when lit, 1 when
    /// there is a normal map.
    params: [f32; 4],
}

#[derive(Debug)]
struct CachedSheet {
    group: wgpu::BindGroup,
    images: [Option<Arc<TextureGpu>>; 2],
    used: u64,
}

/// A run of instances of one style.
#[derive(Clone, Debug)]
struct SheetBatch {
    group: wgpu::BindGroup,
    sheet: u32,
    first: u32,
    count: u32,
    blend: bool,
    /// The shadow view, or `usize::MAX` for the scene pass.
    view: usize,
}

/// One slot's billboard instances, batches and style bind groups.
#[derive(Debug)]
pub(crate) struct BillboardFrame {
    instances: GrowBuffer,
    batches: Vec<SheetBatch>,
    cache: HashMap<SheetKey, CachedSheet>,
    frame: u64,
}

impl BillboardFrame {
    pub(crate) fn new(device: &wgpu::Device) -> Self {
        Self {
            instances: instance_buffer(device, "yowie billboards"),
            batches: Vec::new(),
            cache: HashMap::new(),
            frame: 0,
        }
    }

    fn group(&mut self, ctx: &PrepareContext<'_>, sheet: &Sheet) -> wgpu::BindGroup {
        let key = sheet.key();
        let images = [
            sheet.albedo.as_ref().map(|t| t.gpu()),
            sheet.normal.as_ref().map(|t| t.gpu()),
        ];
        let same = |a: &Option<Arc<TextureGpu>>, b: &Option<Arc<TextureGpu>>| match (a, b) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        };
        if let Some(c) = self.cache.get_mut(&key)
            && c.images.iter().zip(&images).all(|(a, b)| same(a, b))
        {
            c.used = self.frame;
            return c.group.clone();
        }
        let device = ctx.shared.device();
        let g = sheet.grid;
        let params = SheetGpu {
            grid: g.map_or([0.0; 4], |g| {
                [g.views as f32, g.cols as f32, g.rows as f32, g.pitch]
            }),
            params: [
                sheet.cutoff,
                sheet.roughness.clamp(0.03, 1.0),
                if sheet.lit { 1.0 } else { 0.0 },
                if sheet.normal.is_some() { 1.0 } else { 0.0 },
            ],
        };
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("yowie billboard sheet"),
            size: std::mem::size_of::<SheetGpu>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        ctx.shared
            .queue()
            .write_buffer(&buffer, 0, bytemuck::bytes_of(&params));
        let albedo = images[0].as_ref().map_or(ctx.white, |g| &g.view);
        let normal = images[1].as_ref().map_or(ctx.flat_normal, |g| &g.view);
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("yowie billboard sheet"),
            layout: &ctx.pipelines.sheet_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 32,
                    resource: wgpu::BindingResource::TextureView(albedo),
                },
                wgpu::BindGroupEntry {
                    binding: 33,
                    resource: wgpu::BindingResource::TextureView(normal),
                },
                wgpu::BindGroupEntry {
                    binding: 34,
                    resource: wgpu::BindingResource::Sampler(ctx.clamp),
                },
                wgpu::BindGroupEntry {
                    binding: 35,
                    resource: buffer.as_entire_binding(),
                },
            ],
        });
        self.cache.insert(
            key,
            CachedSheet {
                group: group.clone(),
                images,
                used: self.frame,
            },
        );
        group
    }
}

/// A conservative sphere around a billboard whatever way it faces.
fn sphere(b: &GpuBillboard) -> (yowie_core::Vec3, f32) {
    let size = yowie_core::Vec2::new(b.size_pivot[0], b.size_pivot[1]);
    (yowie_core::Vec3::from_array(b.position), size.length())
}

/// Culls the recorded billboards and uploads their instances.
pub(crate) fn prepare(
    ctx: &PrepareContext<'_>,
    rec: &LargeRecord,
    frame: &mut BillboardFrame,
    stats: &mut FrameStats,
) {
    frame.batches.clear();
    frame.frame += 1;
    let now = frame.frame;
    if now.is_multiple_of(60) {
        frame.cache.retain(|_, c| now - c.used <= KEEP_FRAMES);
    }
    let r = &rec.billboards;
    if r.instances.is_empty() {
        return;
    }
    let groups: Vec<wgpu::BindGroup> = r.sheets.iter().map(|s| frame.group(ctx, s)).collect();
    let mut visible: Vec<(u32, f32, usize)> = Vec::with_capacity(r.instances.len());
    for (i, (sheet, b)) in r.instances.iter().enumerate() {
        let (c, rad) = sphere(b);
        if ctx.view.intersects_sphere(c, rad) {
            // Blended styles draw back to front.
            let d = if r.sheets[*sheet as usize].blend {
                -(c - ctx.eye).length()
            } else {
                0.0
            };
            visible.push((*sheet, d, i));
        } else {
            stats.culled_3d += 1;
        }
    }
    visible.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)));
    stats.billboards += visible.len() as u32;
    let mut out: Vec<GpuBillboard> = Vec::with_capacity(visible.len());
    let push_run = |out: &mut Vec<GpuBillboard>,
                    batches: &mut Vec<SheetBatch>,
                    sheet: u32,
                    view: usize,
                    b: GpuBillboard| {
        let at = out.len() as u32;
        out.push(b);
        match batches.last_mut() {
            Some(l) if l.view == view && l.first + l.count == at && l.sheet == sheet => {
                l.count += 1;
            }
            _ => batches.push(SheetBatch {
                group: groups[sheet as usize].clone(),
                sheet,
                first: at,
                count: 1,
                blend: r.sheets[sheet as usize].blend,
                view,
            }),
        }
    };
    for (sheet, _, i) in &visible {
        push_run(
            &mut out,
            &mut frame.batches,
            *sheet,
            usize::MAX,
            r.instances[*i].1,
        );
    }
    for (view, frustum) in ctx.shadows {
        for (sheet, b) in &r.instances {
            if !r.sheets[*sheet as usize].shadows {
                continue;
            }
            let (c, rad) = sphere(b);
            if frustum.intersects_sphere(c, rad) {
                let mut b = *b;
                b.view = [*view as u32, 0];
                push_run(&mut out, &mut frame.batches, *sheet, *view, b);
            }
        }
    }
    frame.instances.write(
        ctx.shared.device(),
        ctx.shared.queue(),
        bytemuck::cast_slice(&out),
    );
}

fn draw(
    pass: &mut wgpu::RenderPass<'_>,
    frame: &BillboardFrame,
    pick: impl Fn(&SheetBatch) -> bool,
) -> u32 {
    let mut calls = 0;
    for b in frame.batches.iter().filter(|b| pick(b)) {
        pass.set_bind_group(1, &b.group, &[]);
        pass.draw(0..6, b.first..b.first + b.count);
        calls += 1;
    }
    calls
}

/// Which of the scene pass's billboards a call records, and how.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SheetPass {
    /// Cut-out billboards, with the opaque draws.
    Cutout,
    /// Blended billboards over the scene, after the sorted blended meshes.
    Blend,
    /// Blended billboards into the order-independent transparency targets.
    Oit,
}

/// Whether the scene pass has blended billboards to draw.
pub(crate) fn has_blend(frame: &BillboardFrame) -> bool {
    frame
        .batches
        .iter()
        .any(|b| b.view == usize::MAX && b.blend)
}

/// Records the scene pass's cut-out or blended billboards.
pub(crate) fn encode(
    p: &LargePipelines,
    frame: &BillboardFrame,
    pass: &mut wgpu::RenderPass<'_>,
    which: SheetPass,
) -> u32 {
    let blend = which != SheetPass::Cutout;
    let pick = |b: &SheetBatch| b.view == usize::MAX && b.blend == blend;
    if !frame.batches.iter().any(pick) {
        return 0;
    }
    pass.set_pipeline(match which {
        SheetPass::Cutout => &p.billboard_cutout,
        SheetPass::Blend => &p.billboard_blend,
        SheetPass::Oit => &p.billboard_oit,
    });
    pass.set_vertex_buffer(0, frame.instances.buffer.slice(..));
    draw(pass, frame, pick)
}

/// Records the billboard casters of shadow view `view`.
pub(crate) fn encode_shadow(
    p: &LargePipelines,
    frame: &BillboardFrame,
    pass: &mut wgpu::RenderPass<'_>,
    view: usize,
) -> u32 {
    let pick = |b: &SheetBatch| b.view == view;
    if !frame.batches.iter().any(pick) {
        return 0;
    }
    pass.set_pipeline(&p.billboard_shadow);
    pass.set_vertex_buffer(0, frame.instances.buffer.slice(..));
    draw(pass, frame, pick)
}
