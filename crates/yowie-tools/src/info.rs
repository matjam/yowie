//! A description of the GPU stack, produced without opening a window.

use std::fmt::Write;
use wgpu::{Adapter, Backends, Features, Instance, InstanceDescriptor};

/// The version of wgpu this build links, from the workspace lock file.
pub const WGPU_VERSION: &str = env!("YOWIE_WGPU_VERSION");

/// Describes every adapter wgpu finds on any backend: name, backend, device
/// type, driver, supported features, key limits, and whether BC texture
/// compression, timestamp queries and immediates (push constants) are
/// available. Returns the report as text, one section per adapter.
pub fn report() -> String {
    let instance = Instance::new(InstanceDescriptor::new_without_display_handle());
    let adapters = pollster::block_on(instance.enumerate_adapters(Backends::all()));
    let mut out = format!("wgpu {WGPU_VERSION}\n{} adapter(s)\n", adapters.len());
    for (i, adapter) in adapters.iter().enumerate() {
        let _ = describe(&mut out, i, adapter);
    }
    out
}

fn describe(out: &mut String, index: usize, adapter: &Adapter) -> std::fmt::Result {
    let info = adapter.get_info();
    let features = adapter.features();
    let limits = adapter.limits();
    writeln!(out, "\nadapter {index}: {}", info.name)?;
    writeln!(out, "  backend:  {:?}", info.backend)?;
    writeln!(out, "  type:     {:?}", info.device_type)?;
    writeln!(out, "  driver:   {} {}", info.driver, info.driver_info)?;
    writeln!(
        out,
        "  ids:      vendor {:#06x}, device {:#06x}",
        info.vendor, info.device
    )?;
    writeln!(out, "  capabilities")?;
    for (label, feature) in [
        ("BC texture compression", Features::TEXTURE_COMPRESSION_BC),
        ("timestamp queries", Features::TIMESTAMP_QUERY),
        ("immediates", Features::IMMEDIATES),
    ] {
        let state = if features.contains(feature) {
            "yes"
        } else {
            "no"
        };
        writeln!(out, "    {label}: {state}")?;
    }
    writeln!(out, "  limits")?;
    let rows: [(&str, u64); 11] = [
        (
            "max texture dimension 2D",
            limits.max_texture_dimension_2d.into(),
        ),
        ("max bind groups", limits.max_bind_groups.into()),
        ("max color attachments", limits.max_color_attachments.into()),
        (
            "max sampled textures per stage",
            limits.max_sampled_textures_per_shader_stage.into(),
        ),
        (
            "max uniform buffer binding size",
            limits.max_uniform_buffer_binding_size,
        ),
        (
            "max storage buffer binding size",
            limits.max_storage_buffer_binding_size,
        ),
        ("max buffer size", limits.max_buffer_size),
        (
            "max compute invocations per workgroup",
            limits.max_compute_invocations_per_workgroup.into(),
        ),
        (
            "max compute workgroup size x",
            limits.max_compute_workgroup_size_x.into(),
        ),
        ("max immediate size", limits.max_immediate_size.into()),
        (
            "max bind groups + vertex buffers",
            limits.max_bind_groups_plus_vertex_buffers.into(),
        ),
    ];
    for (name, value) in rows {
        writeln!(out, "    {name}: {value}")?;
    }
    let names: Vec<&str> = features.iter_names().map(|(n, _)| n).collect();
    writeln!(out, "  features ({}): {}", names.len(), names.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_names_the_version() {
        let text = report();
        assert!(text.starts_with("wgpu "), "{text}");
        assert!(text.contains("adapter(s)"));
        assert_ne!(WGPU_VERSION, "");
    }
}
