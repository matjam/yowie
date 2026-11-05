//! GPU timestamps around the frame's passes, read back a few frames later
//! through a ring of query sets so the processor never waits for them.

use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

/// How long the GPU spent on each group of passes in a recent frame, in
/// microseconds. Where a group spans several passes (all the post passes,
/// or the passes of several scenes) its time is the sum.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GpuTimes {
    /// The shadow atlas passes.
    pub shadows_us: u32,
    /// Opaque and cut-out meshes and the sky.
    pub opaque_us: u32,
    /// Blended meshes, lines and particles.
    pub blended_us: u32,
    /// The post-processing passes.
    pub post_us: u32,
    /// The 2D passes, interface and console included.
    pub overlay_us: u32,
}

impl GpuTimes {
    /// The sum of the groups.
    pub fn total_us(&self) -> u32 {
        self.shadows_us + self.opaque_us + self.blended_us + self.post_us + self.overlay_us
    }
}

/// A group of passes the timer measures.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Timed {
    Shadows,
    Opaque,
    Blended,
    Post,
    Overlay,
}

/// Queries per frame: begin and end of up to 64 timed spans.
const QUERIES: u32 = 128;
/// Frames in flight before a frame's queries are reused.
const RING: usize = 3;

const FREE: u8 = 0;
const MAPPING: u8 = 1;
const READY: u8 = 2;

#[derive(Debug)]
struct TimerFrame {
    set: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    readback: wgpu::Buffer,
    spans: Vec<Timed>,
    state: Arc<AtomicU8>,
}

/// A begin and end query of the current frame's set.
#[derive(Clone, Debug)]
pub(crate) struct Span {
    pub(crate) set: wgpu::QuerySet,
    pub(crate) begin: u32,
    pub(crate) end: u32,
}

impl Span {
    /// Timestamp writes for one pass that is the whole span.
    pub(crate) fn whole(&self) -> wgpu::RenderPassTimestampWrites<'_> {
        wgpu::RenderPassTimestampWrites {
            query_set: &self.set,
            beginning_of_pass_write_index: Some(self.begin),
            end_of_pass_write_index: Some(self.end),
        }
    }

    /// Timestamp writes for a pass that begins the span (`begin`), ends
    /// it (`end`), or both; `None` when neither.
    pub(crate) fn part(
        &self,
        begin: bool,
        end: bool,
    ) -> Option<wgpu::RenderPassTimestampWrites<'_>> {
        (begin || end).then(|| wgpu::RenderPassTimestampWrites {
            query_set: &self.set,
            beginning_of_pass_write_index: begin.then_some(self.begin),
            end_of_pass_write_index: end.then_some(self.end),
        })
    }

    /// Writes the span's first timestamp (`begin`) or its last with an
    /// empty compute pass, for a span over passes that a variable number
    /// of steps record.
    pub(crate) fn mark(&self, encoder: &mut wgpu::CommandEncoder, begin: bool) {
        let _ = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("yowie timestamp"),
            timestamp_writes: Some(wgpu::ComputePassTimestampWrites {
                query_set: &self.set,
                beginning_of_pass_write_index: begin.then_some(self.begin),
                end_of_pass_write_index: (!begin).then_some(self.end),
            }),
        });
    }
}

/// The renderer's GPU timer. Off until asked for, and inert on devices
/// without timestamp queries.
#[derive(Debug, Default)]
pub(crate) struct GpuTimer {
    on: bool,
    supported: bool,
    /// Timestamps can be written inside a pass, so the HDR pass need not
    /// split in two to time its halves.
    pub(crate) inside_passes: bool,
    period_ns: f32,
    frames: Vec<TimerFrame>,
    current: Option<usize>,
    latest: Option<GpuTimes>,
}

impl GpuTimer {
    pub(crate) fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let f = device.features();
        Self {
            supported: f.contains(wgpu::Features::TIMESTAMP_QUERY),
            inside_passes: f.contains(wgpu::Features::TIMESTAMP_QUERY_INSIDE_PASSES),
            period_ns: queue.get_timestamp_period(),
            ..Self::default()
        }
    }

    /// Whether the device has timestamp queries.
    pub(crate) fn supported(&self) -> bool {
        self.supported
    }

    /// Turns timing on or off; turning it off forgets the last reading.
    pub(crate) fn set_on(&mut self, on: bool) {
        self.on = on && self.supported;
        if !self.on {
            self.latest = None;
        }
    }

    /// The most recent frame read back.
    pub(crate) fn latest(&self) -> Option<GpuTimes> {
        self.latest
    }

    /// Reads back frames the GPU has finished and picks a free query set
    /// for this frame; with none free this frame is not timed.
    pub(crate) fn begin_frame(&mut self, device: &wgpu::Device) {
        self.current = None;
        if !self.on {
            return;
        }
        if self.frames.is_empty() {
            for _ in 0..RING {
                self.frames.push(TimerFrame::new(device));
            }
        }
        // Mapping callbacks run during polling; this one never waits.
        let _ = device.poll(wgpu::PollType::Poll);
        for i in 0..self.frames.len() {
            if self.frames[i].state.load(Ordering::Acquire) == READY {
                self.read(i);
            }
        }
        self.current = self
            .frames
            .iter()
            .position(|f| f.state.load(Ordering::Acquire) == FREE);
        if let Some(i) = self.current {
            self.frames[i].spans.clear();
        }
    }

    /// A span of this frame's queries for `what`, or `None` when the frame
    /// is not timed or its queries are used up.
    pub(crate) fn span(&mut self, what: Timed) -> Option<Span> {
        let f = &mut self.frames[self.current?];
        let n = f.spans.len() as u32;
        if (n + 1) * 2 > QUERIES {
            return None;
        }
        f.spans.push(what);
        Some(Span {
            set: f.set.clone(),
            begin: n * 2,
            end: n * 2 + 1,
        })
    }

    /// Records the copy of this frame's timestamps into its readback
    /// buffer. Call before the encoder is finished.
    pub(crate) fn resolve(&mut self, encoder: &mut wgpu::CommandEncoder) {
        let Some(i) = self.current else {
            return;
        };
        let f = &self.frames[i];
        let n = f.spans.len() as u32 * 2;
        if n == 0 {
            return;
        }
        encoder.resolve_query_set(&f.set, 0..n, &f.resolve, 0);
        encoder.copy_buffer_to_buffer(&f.resolve, 0, &f.readback, 0, u64::from(n) * 8);
    }

    /// Starts mapping this frame's readback once its commands are
    /// submitted.
    pub(crate) fn after_submit(&mut self) {
        let Some(i) = self.current.take() else {
            return;
        };
        let f = &self.frames[i];
        if f.spans.is_empty() {
            return;
        }
        f.state.store(MAPPING, Ordering::Release);
        let state = f.state.clone();
        let bytes = f.spans.len() as u64 * 16;
        f.readback
            .map_async(wgpu::MapMode::Read, ..bytes, move |r| {
                state.store(if r.is_ok() { READY } else { FREE }, Ordering::Release);
            });
    }

    fn read(&mut self, i: usize) {
        let f = &self.frames[i];
        let bytes = f.spans.len() as u64 * 16;
        let mut times = GpuTimes::default();
        if let Ok(view) = f.readback.get_mapped_range(..bytes) {
            let stamps: Vec<u64> = view
                .as_chunks::<8>()
                .0
                .iter()
                .map(|w| u64::from_le_bytes(*w))
                .collect();
            for (k, what) in f.spans.iter().enumerate() {
                let (b, e) = (stamps[k * 2], stamps[k * 2 + 1]);
                let us = (e.saturating_sub(b) as f64 * f64::from(self.period_ns) / 1000.0) as u32;
                let slot = match what {
                    Timed::Shadows => &mut times.shadows_us,
                    Timed::Opaque => &mut times.opaque_us,
                    Timed::Blended => &mut times.blended_us,
                    Timed::Post => &mut times.post_us,
                    Timed::Overlay => &mut times.overlay_us,
                };
                *slot += us;
            }
        }
        f.readback.unmap();
        f.state.store(FREE, Ordering::Release);
        if self.on {
            self.latest = Some(times);
        }
    }
}

impl TimerFrame {
    fn new(device: &wgpu::Device) -> Self {
        let size = u64::from(QUERIES) * 8;
        Self {
            set: device.create_query_set(&wgpu::QuerySetDescriptor {
                label: Some("yowie timestamps"),
                ty: wgpu::QueryType::Timestamp,
                count: QUERIES,
            }),
            resolve: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("yowie timestamps resolve"),
                size,
                usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            }),
            readback: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("yowie timestamps readback"),
                size,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            spans: Vec::new(),
            state: Arc::new(AtomicU8::new(FREE)),
        }
    }
}
