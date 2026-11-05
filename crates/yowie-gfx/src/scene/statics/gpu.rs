//! The GPU side of static groups: each group's items, each view's culling
//! buffers, the depth pyramid, and the compute passes around the scene's
//! HDR pass (see the module documentation for the two phases).

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;
use yowie_core::Mat4;

use super::StaticUse;
use super::compute::{cull_group, pipelines, pyramid};
use super::state::{COPIED, FREE, Readback, StaticFrame};
use crate::scene::bounds::Frustum;
use crate::scene::frame_data::FrameUniform;
use crate::scene::record::FastMap;
use crate::scene::render::SceneRenderer;
use crate::scene::slot::SlotId;
use crate::scene::vertex::affine_rows;
use crate::shared::Shared;

/// Counter copies kept in flight per view.
const READBACKS: usize = 3;
/// Bytes per instance in the visible buffer (`GpuInstance`).
pub(crate) const INSTANCE_BYTES: u64 = 64;

/// Matches `Cull` in `cull.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
struct CullUniform {
    view_proj: [[f32; 4]; 4],
    planes: [[f32; 4]; 6],
    size: [f32; 4],
    counts: [u32; 4],
}

/// Matches `CullItem` in `cull.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
struct CullItem {
    rows: [[f32; 4]; 3],
    bmin: [f32; 4],
    bmax: [f32; 4],
    info: [u32; 4],
}

/// One group's items on the GPU and its per-view state.
#[derive(Debug)]
pub(crate) struct GroupGpu {
    pub(crate) used: u64,
    items: wgpu::Buffer,
    batch_first: wgpu::Buffer,
    /// Fresh draw arguments for both phases, then zeroed counters.
    template: wgpu::Buffer,
    pub(crate) n_items: u32,
    pub(crate) n_batches: u32,
    n_materials: u32,
    /// Each mesh's index count when built; a change rebuilds the group.
    index_counts: Vec<u32>,
    pub(crate) views: FastMap<SlotId, ViewGpu>,
}

/// What a bundle was recorded with; a difference records it again.
#[derive(Debug, PartialEq)]
pub(crate) struct BundleKey {
    pub(crate) frame: wgpu::BindGroup,
    pub(crate) materials: Vec<Option<wgpu::BindGroup>>,
    pub(crate) meshes: Vec<(wgpu::Buffer, wgpu::Buffer)>,
}

/// One group's culling state in one slot.
#[derive(Debug)]
pub(crate) struct ViewGpu {
    pub(super) uniform: wgpu::Buffer,
    pub(super) remap: wgpu::Buffer,
    pub(super) visibility: wgpu::Buffer,
    pub(crate) args: wgpu::Buffer,
    pub(crate) visible: wgpu::Buffer,
    group: Option<(wgpu::TextureView, wgpu::BindGroup)>,
    pub(crate) bundles: Option<(BundleKey, wgpu::RenderBundle, wgpu::RenderBundle)>,
    pub(crate) readbacks: Vec<Readback>,
    latest: [u32; 2],
}

impl GroupGpu {
    /// The byte offset of the counters in a view's argument buffer.
    pub(crate) fn counters_offset(&self) -> u64 {
        u64::from(self.n_batches) * 10 * 4
    }

    fn build(device: &wgpu::Device, u: &StaticUse, frame: u64) -> Self {
        let l = u.data.layout();
        let items: Vec<CullItem> = l
            .order
            .iter()
            .enumerate()
            .map(|(pos, &i)| {
                let b = l.boxes[pos];
                CullItem {
                    rows: affine_rows(&u.data.items[i as usize].model),
                    bmin: b.min.extend(0.0).to_array(),
                    bmax: b.max.extend(0.0).to_array(),
                    info: [l.item_material[i as usize], l.order_batch[pos], 0, 0],
                }
            })
            .collect();
        let index_counts: Vec<u32> = l.meshes.iter().map(|m| m.index_count()).collect();
        let mut template: Vec<u32> = Vec::with_capacity(l.batches.len() * 10 + 4);
        for _ in 0..2 {
            for b in &l.batches {
                template.extend_from_slice(&[index_counts[b.mesh as usize], 0, 0, 0, 0]);
            }
        }
        template.extend_from_slice(&[0; 4]);
        let firsts: Vec<u32> = l.batches.iter().map(|b| b.first).collect();
        let init = |label, contents: &[u8], usage| {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(label),
                contents,
                usage,
            })
        };
        use wgpu::BufferUsages as U;
        Self {
            used: frame,
            items: init(
                "yowie static items",
                bytemuck::cast_slice(&items),
                U::STORAGE,
            ),
            batch_first: init(
                "yowie static batches",
                bytemuck::cast_slice(&firsts),
                U::STORAGE,
            ),
            template: init(
                "yowie static args",
                bytemuck::cast_slice(&template),
                U::COPY_SRC,
            ),
            n_items: items.len() as u32,
            n_batches: l.batches.len() as u32,
            n_materials: l.materials.len() as u32,
            index_counts,
            views: FastMap::default(),
        }
    }

    fn view(&self, device: &wgpu::Device) -> ViewGpu {
        use wgpu::BufferUsages as U;
        let buffer = |label, size: u64, usage| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: size.max(16),
                usage,
                mapped_at_creation: false,
            })
        };
        ViewGpu {
            uniform: buffer(
                "yowie cull",
                std::mem::size_of::<CullUniform>() as u64,
                U::UNIFORM | U::COPY_DST,
            ),
            remap: buffer(
                "yowie static materials",
                u64::from(self.n_materials) * 4,
                U::STORAGE | U::COPY_DST,
            ),
            visibility: buffer(
                "yowie static visibility",
                u64::from(self.n_items) * 4,
                U::STORAGE,
            ),
            args: buffer(
                "yowie static draws",
                self.template.size(),
                U::STORAGE | U::INDIRECT | U::COPY_DST | U::COPY_SRC,
            ),
            visible: buffer(
                "yowie static instances",
                u64::from(self.n_items) * 2 * INSTANCE_BYTES,
                U::STORAGE | U::VERTEX,
            ),
            group: None,
            bundles: None,
            readbacks: (0..READBACKS).map(|_| Readback::new(device)).collect(),
            latest: [0; 2],
        }
    }
}

impl SceneRenderer {
    /// Prepares record `ri`'s static groups for `slot` and records the
    /// early cull; returns the bundles to replay, or `None` when the
    /// record has none or the device cannot cull on the GPU.
    pub(crate) fn statics_early(
        &mut self,
        shared: &Shared,
        encoder: &mut wgpu::CommandEncoder,
        ri: usize,
        slot: SlotId,
        frame_group: &wgpu::BindGroup,
        frame: &FrameUniform,
    ) -> Option<StaticFrame> {
        if !self.statics.gpu_culling() || self.records[ri].statics.is_empty() {
            return None;
        }
        let (device, queue) = (shared.device(), shared.queue());
        if self.statics.polled != self.frame {
            // Lets last frames' counter maps finish without waiting.
            self.statics.polled = self.frame;
            if let Err(e) = device.poll(wgpu::PollType::Poll) {
                log::debug!("yowie-gfx: poll: {e}");
            }
        }
        let t = self.slot(slot).targets.as_ref()?;
        let (depth, size) = (t.depth.clone(), t.size);
        let pipes = self
            .statics
            .pipelines
            .get_or_insert_with(|| pipelines(device));
        let stale = self
            .statics
            .pyramids
            .get(&slot)
            .is_none_or(|p| p.depth != depth || p.size != (size.x, size.y));
        if stale {
            let p = pyramid(device, pipes, &depth, size);
            self.statics.pyramids.insert(slot, p);
        }
        let (pyramid_view, mips) = {
            let p = &self.statics.pyramids[&slot];
            (p.view.clone(), p.mips)
        };
        let vp = Mat4::from_cols_array_2d(&frame.view_proj);
        let planes = Frustum::from_view_proj(&vp).planes.map(|p| p.to_array());
        let uses = self.records[ri].statics.clone();
        let mut out = StaticFrame::default();
        for u in &uses {
            let id = u.data.id;
            self.ensure_group(device, u);
            let st = &mut self.statics;
            let pipes = st.pipelines.as_ref()?;
            let Some(g) = st.groups.get_mut(&id) else {
                continue;
            };
            if !g.views.contains_key(&slot) {
                let v = g.view(device);
                g.views.insert(slot, v);
            }
            let cull = CullUniform {
                view_proj: frame.view_proj,
                planes,
                size: [size.x as f32, size.y as f32, mips as f32, 0.0],
                counts: [g.n_items, g.n_batches, 0, 0],
            };
            let template = g.template.clone();
            let bufs = (g.items.clone(), g.batch_first.clone());
            let Some(v) = g.views.get_mut(&slot) else {
                continue;
            };
            queue.write_buffer(&v.uniform, 0, bytemuck::bytes_of(&cull));
            queue.write_buffer(&v.remap, 0, bytemuck::cast_slice(&u.materials));
            encoder.copy_buffer_to_buffer(&template, 0, &v.args, 0, template.size());
            if v.group.as_ref().is_none_or(|(pv, _)| *pv != pyramid_view) {
                let group = cull_group(device, pipes, v, &bufs.0, &bufs.1, &pyramid_view);
                v.group = Some((pyramid_view.clone(), group));
            }
            for r in &v.readbacks {
                if let Some(c) = r.take() {
                    v.latest = [c[0], c[1]];
                }
            }
            out.culled += v.latest[0];
            out.occluded += v.latest[1];
            out.groups.push(id);
            let draws = self.ensure_bundles(shared, u, slot, frame_group);
            out.draws += draws;
            if let Some((_, early, late)) = self
                .statics
                .groups
                .get(&id)
                .and_then(|g| g.views.get(&slot))
                .and_then(|v| v.bundles.as_ref())
            {
                out.early.push(early.clone());
                out.late.push(late.clone());
            }
        }
        let pipes = self.statics.pipelines.as_ref()?;
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("yowie static cull"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&pipes.early);
        for id in &out.groups {
            if let Some(g) = self.statics.groups.get(id)
                && let Some((_, group)) = g.views.get(&slot).and_then(|v| v.group.as_ref())
            {
                pass.set_bind_group(0, group, &[]);
                pass.dispatch_workgroups(g.n_items.div_ceil(64), 1, 1);
            }
        }
        Some(out)
    }

    /// After the early draws: builds `slot`'s depth pyramid, records the
    /// late cull, and copies the counters for reading back.
    pub(crate) fn statics_late(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        slot: SlotId,
        st: &StaticFrame,
    ) {
        let s = &self.statics;
        let (Some(pipes), Some(pyr)) = (s.pipelines.as_ref(), s.pyramids.get(&slot)) else {
            return;
        };
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("yowie depth pyramid"),
                timestamp_writes: None,
            });
            for (i, (group, (w, h))) in pyr.steps.iter().enumerate() {
                pass.set_pipeline(if i == 0 { &pipes.first } else { &pipes.down });
                pass.set_bind_group(0, group, &[]);
                pass.dispatch_workgroups(w.div_ceil(8), h.div_ceil(8), 1);
            }
            pass.set_pipeline(&pipes.late);
            for id in &st.groups {
                if let Some(g) = s.groups.get(id)
                    && let Some((_, group)) = g.views.get(&slot).and_then(|v| v.group.as_ref())
                {
                    pass.set_bind_group(0, group, &[]);
                    pass.dispatch_workgroups(g.n_items.div_ceil(64), 1, 1);
                }
            }
        }
        for id in &st.groups {
            let Some(g) = s.groups.get(id) else {
                continue;
            };
            let Some(v) = g.views.get(&slot) else {
                continue;
            };
            if let Some(r) = v
                .readbacks
                .iter()
                .find(|r| r.state.load(std::sync::atomic::Ordering::Acquire) == FREE)
            {
                encoder.copy_buffer_to_buffer(&v.args, g.counters_offset(), &r.buffer, 0, 16);
                r.state.store(COPIED, std::sync::atomic::Ordering::Release);
            }
        }
    }

    /// Builds group `u`'s GPU data when missing or when one of its meshes
    /// changed its index count, and marks it used.
    fn ensure_group(&mut self, device: &wgpu::Device, u: &StaticUse) {
        let frame = self.frame;
        let fresh = self.statics.groups.get(&u.data.id).is_none_or(|g| {
            let l = u.data.layout();
            g.index_counts.len() != l.meshes.len()
                || g.index_counts
                    .iter()
                    .zip(&l.meshes)
                    .any(|(c, m)| *c != m.index_count())
        });
        if fresh {
            let g = GroupGpu::build(device, u, frame);
            self.statics.groups.insert(u.data.id, g);
        }
        if let Some(g) = self.statics.groups.get_mut(&u.data.id) {
            g.used = frame;
        }
    }
}
