//! Creating the wgpu instance, adapter, device and queue.

use crate::error::{GfxError, Result};

/// The GPU objects every part of the renderer shares.
#[derive(Debug)]
pub(crate) struct Gpu {
    pub(crate) instance: wgpu::Instance,
    pub(crate) adapter: wgpu::Adapter,
    pub(crate) device: wgpu::Device,
    pub(crate) queue: wgpu::Queue,
}

/// The backends Yowie renders with: Vulkan on Linux and Windows, Metal on
/// macOS, DX12 on Windows. `WGPU_BACKEND` narrows the set.
fn instance() -> wgpu::Instance {
    let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
    desc.backends = wgpu::Backends::VULKAN | wgpu::Backends::METAL | wgpu::Backends::DX12;
    wgpu::Instance::new(desc.with_env())
}

/// The features Yowie turns on when the adapter has them; each has a
/// fallback when it is missing.
fn optional_features() -> wgpu::Features {
    wgpu::Features::TEXTURE_COMPRESSION_BC
        | wgpu::Features::DEPTH32FLOAT_STENCIL8
        | wgpu::Features::TIMESTAMP_QUERY
        | wgpu::Features::TIMESTAMP_QUERY_INSIDE_PASSES
        | wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES
}

impl Gpu {
    /// Creates an instance and, from it, the surface `make_surface` builds,
    /// then picks an adapter that can present to that surface.
    pub(crate) async fn new<S>(
        make_surface: impl FnOnce(&wgpu::Instance) -> Result<S>,
        surface_of: impl Fn(&S) -> Option<&wgpu::Surface<'static>>,
    ) -> Result<(Gpu, S)> {
        let instance = instance();
        let surface = make_surface(&instance)?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: false,
                compatible_surface: surface_of(&surface),
                apply_limit_buckets: false,
            })
            .await
            .map_err(|e| GfxError::NoAdapter(e.to_string()))?;
        let info = adapter.get_info();
        log::info!(
            "yowie-gfx: adapter {} ({:?}, {:?})",
            info.name,
            info.backend,
            info.device_type
        );
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("yowie device"),
                // Block-compressed (BC) textures, 32-bit float depth with
                // stencil for the 3D scene, GPU timestamps and the
                // adapter's own multisample counts when the GPU has them.
                required_features: adapter.features() & optional_features(),
                required_limits: required_limits(&adapter.limits()),
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
                memory_hints: wgpu::MemoryHints::Performance,
                trace: wgpu::Trace::Off,
            })
            .await
            .map_err(|e| GfxError::RequestDevice(e.to_string()))?;
        device.on_uncaptured_error(std::sync::Arc::new(|e: wgpu::Error| {
            log::error!("yowie-gfx: uncaptured GPU error: {e}");
        }));
        Ok((
            Gpu {
                instance,
                adapter,
                device,
                queue,
            },
            surface,
        ))
    }

    /// Creates a device with no surface, for headless rendering.
    pub(crate) async fn headless() -> Result<Gpu> {
        let (gpu, ()) = Gpu::new(|_| Ok(()), |_| None).await?;
        Ok(gpu)
    }
}

/// The sampled textures per shader stage the 3D surface program binds: the
/// shadow atlas, two environment maps, the transmission copy and two decal
/// atlases, fourteen material textures, and room for later additions.
const SCENE_SAMPLED_TEXTURES: u32 = 32;

/// The WebGPU defaults raised to the adapter's resolution limits and to
/// the sampled texture count the 3D renderer needs, where the adapter
/// allows it. Every desktop Metal, Vulkan and DX12 driver allows at least
/// 31.
fn required_limits(adapter: &wgpu::Limits) -> wgpu::Limits {
    let mut limits = wgpu::Limits::default().using_resolution(adapter.clone());
    limits.max_sampled_textures_per_shader_stage = limits
        .max_sampled_textures_per_shader_stage
        .max(SCENE_SAMPLED_TEXTURES.min(adapter.max_sampled_textures_per_shader_stage));
    limits
}

/// Whether this machine has a GPU adapter Yowie can render with. Tests and
/// tools call it to skip GPU work on machines without a driver.
pub fn adapter_available() -> bool {
    default_adapter_info().is_some()
}

/// The name, backend and type of the adapter a headless renderer would
/// use, or `None` when there is none. The examples test compares golden
/// images only on the adapter they were recorded with.
pub fn default_adapter_info() -> Option<wgpu::AdapterInfo> {
    let instance = instance();
    let options = wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        ..Default::default()
    };
    pollster::block_on(instance.request_adapter(&options))
        .ok()
        .map(|a| a.get_info())
}
