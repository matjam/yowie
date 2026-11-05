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
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default().using_resolution(adapter.limits()),
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

/// Whether this machine has a GPU adapter Yowie can render with. Tests and
/// tools call it to skip GPU work on machines without a driver.
pub fn adapter_available() -> bool {
    let instance = instance();
    pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())).is_ok()
}
