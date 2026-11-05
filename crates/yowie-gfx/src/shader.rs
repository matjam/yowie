//! Game-written 2D fragment shaders, composed with the engine's prelude.

use std::sync::Arc;

use bytemuck::Pod;
use parking_lot::Mutex;

use crate::Graphics;
use crate::blend::BlendMode;
use crate::error::{GfxError, Result};
use crate::pipelines::sprite_pipeline;
use crate::shared::Shared;
use crate::texture::next_id;

/// The engine's half of every 2D shader.
pub(crate) const PRELUDE: &str = include_str!("shaders/sprite_prelude.wgsl");

/// The fragment function the engine draws with when no game shader is set.
pub(crate) const DEFAULT_FRAGMENT: &str = "fn fragment(input: SpriteInput) -> vec4<f32> {
    return sample_texture(input.uv) * input.color;
}
";

/// The prelude followed by the game's source. The prelude ends with a
/// newline, so the game's first line is the line after the prelude's last.
pub(crate) fn compose(source: &str) -> String {
    format!("{PRELUDE}{source}")
}

/// Lines before the game's first line in the composed source.
fn prelude_lines() -> u32 {
    PRELUDE.lines().count() as u32
}

#[derive(Debug)]
pub(crate) struct ShaderState {
    pub(crate) module: wgpu::ShaderModule,
    pub(crate) generation: u32,
    pub(crate) uniforms: Vec<u8>,
    pub(crate) uniform_version: u32,
}

#[derive(Debug)]
pub(crate) struct ShaderInner {
    pub(crate) id: u64,
    pub(crate) uniform_size: usize,
    pub(crate) state: Mutex<ShaderState>,
}

/// A game-written fragment shader for 2D drawing.
///
/// The source defines `fn fragment(input: SpriteInput) -> vec4<f32>`,
/// returning premultiplied colour in linear light. The engine's prelude
/// supplies `SpriteInput` (`uv`, `color`, `world`, `time`, `view_size`,
/// `pixel_scale`) and `sample_texture(uv)`, which samples the drawn
/// texture. A shader made with
/// [`Graphics::compile_shader_with_uniforms`] also declares its block:
///
/// ```wgsl
/// struct Params { amplitude: f32, frequency: f32, _pad: vec2<f32> };
/// @group(2) @binding(0) var<uniform> params: Params;
///
/// fn fragment(input: SpriteInput) -> vec4<f32> {
///     var uv = input.uv;
///     uv.x += sin(uv.y * params.frequency + input.time) * params.amplitude;
///     return sample_texture(uv) * input.color;
/// }
/// ```
///
/// The Rust struct passed as uniforms must match WGSL's uniform layout:
/// 16-byte alignment for `vec3` and `vec4`, and a total size that is a
/// multiple of 16. Cloning shares the shader.
#[derive(Clone, Debug)]
pub struct Shader {
    pub(crate) inner: Arc<ShaderInner>,
}

/// Compiles `source` behind the prelude and builds one pipeline from it, so
/// both naga's errors and interface mismatches come back as an error.
fn compile(
    shared: &Shared,
    source: &str,
    has_uniforms: bool,
    format: wgpu::TextureFormat,
) -> Result<wgpu::ShaderModule> {
    let device = shared.device();
    let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("yowie game shader"),
        source: wgpu::ShaderSource::Wgsl(compose(source).into()),
    });
    let info = pollster::block_on(module.get_compilation_info());
    let messages: Vec<String> = info
        .messages
        .iter()
        .filter(|m| m.message_type == wgpu::CompilationMessageType::Error)
        .map(describe)
        .collect();
    if messages.is_empty() {
        let _pipeline = sprite_pipeline(
            device,
            &shared.layouts,
            &module,
            has_uniforms,
            BlendMode::Alpha,
            format,
        );
    }
    let scoped = pollster::block_on(scope.pop());
    if !messages.is_empty() {
        return Err(GfxError::Shader(messages.join("\n")));
    }
    if let Some(e) = scoped {
        return Err(GfxError::Shader(e.to_string()));
    }
    Ok(module)
}

/// Formats a compiler message with its line counted in the game's source.
fn describe(m: &wgpu::CompilationMessage) -> String {
    match &m.location {
        Some(loc) if loc.line_number > prelude_lines() => format!(
            "line {}, column {}: {}",
            loc.line_number - prelude_lines(),
            loc.line_position,
            m.message
        ),
        Some(loc) => format!("engine prelude line {}: {}", loc.line_number, m.message),
        None => m.message.clone(),
    }
}

impl Graphics {
    /// Compiles a 2D fragment shader with no uniform block. Errors carry
    /// naga's message with line numbers in `source`.
    pub fn compile_shader(&self, source: &str) -> Result<Shader> {
        self.new_shader(source, Vec::new())
    }

    /// Compiles a 2D fragment shader whose uniform block at group 2,
    /// binding 0 holds a `T`, starting with `initial`.
    pub fn compile_shader_with_uniforms<T: Pod>(
        &self,
        source: &str,
        initial: &T,
    ) -> Result<Shader> {
        self.new_shader(source, bytemuck::bytes_of(initial).to_vec())
    }

    fn new_shader(&self, source: &str, uniforms: Vec<u8>) -> Result<Shader> {
        let module = compile(
            &self.shared,
            source,
            !uniforms.is_empty(),
            self.output_format(),
        )?;
        Ok(Shader {
            inner: Arc::new(ShaderInner {
                id: next_id(),
                uniform_size: uniforms.len(),
                state: Mutex::new(ShaderState {
                    module,
                    generation: 0,
                    uniforms,
                    uniform_version: 0,
                }),
            }),
        })
    }
}

impl Shader {
    /// Recompiles the shader from new source, keeping its uniform values.
    /// On error the previous program stays in use and the error says why.
    pub fn reload_source(&self, gfx: &Graphics, source: &str) -> Result<()> {
        let module = compile(
            &gfx.shared,
            source,
            self.inner.uniform_size > 0,
            gfx.output_format(),
        )?;
        let mut state = self.inner.state.lock();
        state.module = module;
        state.generation = state.generation.wrapping_add(1);
        Ok(())
    }

    /// Replaces the uniform values. Draws queued before the call keep the
    /// values they were queued with. `T` must be the size the shader was
    /// created with.
    pub fn set_uniforms<T: Pod>(&self, value: &T) -> Result<()> {
        let bytes = bytemuck::bytes_of(value);
        if bytes.len() != self.inner.uniform_size {
            return Err(GfxError::UniformSize {
                expected: self.inner.uniform_size,
                got: bytes.len(),
            });
        }
        let mut state = self.inner.state.lock();
        state.uniforms.copy_from_slice(bytes);
        state.uniform_version = state.uniform_version.wrapping_add(1);
        Ok(())
    }

    /// How many clones of this shader exist, itself included. A registry
    /// that keeps a clone to reload it can drop the entry when the count
    /// falls to one.
    pub fn share_count(&self) -> usize {
        Arc::strong_count(&self.inner)
    }

    /// The size of the uniform block in bytes; zero for none.
    pub fn uniform_size(&self) -> usize {
        self.inner.uniform_size
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_fragment_follows_the_prelude() {
        let composed = compose(DEFAULT_FRAGMENT);
        assert!(composed.starts_with(PRELUDE));
        assert_eq!(
            composed.lines().nth(prelude_lines() as usize),
            DEFAULT_FRAGMENT.lines().next()
        );
    }
}
