//! Generating a texture's mip chain on the GPU, each level filtered from the
//! one above it.

use std::collections::HashMap;

use parking_lot::Mutex;

/// Blit pipelines per texture format, built the first time a format needs
/// mipmaps.
#[derive(Debug)]
pub(crate) struct MipmapGenerator {
    module: wgpu::ShaderModule,
    layout: wgpu::BindGroupLayout,
    pipeline_layout: wgpu::PipelineLayout,
    sampler: wgpu::Sampler,
    pipelines: Mutex<HashMap<wgpu::TextureFormat, wgpu::RenderPipeline>>,
}

impl MipmapGenerator {
    pub(crate) fn new(device: &wgpu::Device, layout: &wgpu::BindGroupLayout) -> Self {
        let module = crate::pipelines::trusted_module(
            device,
            "yowie blit",
            include_str!("shaders/blit.wgsl"),
        );
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("yowie blit layout"),
            bind_group_layouts: &[Some(layout)],
            immediate_size: 0,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("yowie blit sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Self {
            module,
            layout: layout.clone(),
            pipeline_layout,
            sampler,
            pipelines: Mutex::new(HashMap::new()),
        }
    }

    fn pipeline(&self, device: &wgpu::Device, format: wgpu::TextureFormat) -> wgpu::RenderPipeline {
        let mut pipelines = self.pipelines.lock();
        pipelines
            .entry(format)
            .or_insert_with(|| {
                device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("yowie blit"),
                    layout: Some(&self.pipeline_layout),
                    vertex: wgpu::VertexState {
                        module: &self.module,
                        entry_point: Some("vs_main"),
                        compilation_options: Default::default(),
                        buffers: &[],
                    },
                    primitive: wgpu::PrimitiveState::default(),
                    depth_stencil: None,
                    multisample: wgpu::MultisampleState::default(),
                    fragment: Some(wgpu::FragmentState {
                        module: &self.module,
                        entry_point: Some("fs_main"),
                        compilation_options: Default::default(),
                        targets: &[Some(format.into())],
                    }),
                    multiview_mask: None,
                    cache: None,
                })
            })
            .clone()
    }

    /// Fills levels `1..levels` of `texture` from level 0 and submits the
    /// work. Level 0 must already hold the picture.
    pub(crate) fn generate(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        texture: &wgpu::Texture,
    ) {
        let levels = texture.mip_level_count();
        if levels < 2 {
            return;
        }
        let format = texture.format();
        let pipeline = self.pipeline(device, format);
        let level_view = |level| {
            texture.create_view(&wgpu::TextureViewDescriptor {
                label: Some("yowie mip level"),
                base_mip_level: level,
                mip_level_count: Some(1),
                ..Default::default()
            })
        };
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("yowie mipmaps"),
        });
        for level in 1..levels {
            let src = level_view(level - 1);
            let dst = level_view(level);
            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("yowie mip source"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&src),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                ],
            });
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("yowie mip level"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &dst,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        queue.submit([encoder.finish()]);
    }
}
