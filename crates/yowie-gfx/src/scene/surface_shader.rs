//! Game-written surface programs: WGSL that computes the surface (base
//! colour, normal, roughness, metallic, emissive, alpha and the layered
//! features) which the engine then lights.

use std::sync::Arc;

use bytemuck::Pod;
use parking_lot::Mutex;

use super::layouts::SceneLayouts;
use super::pipelines::{MeshKey, MeshPass, build_mesh, mesh_source};
use crate::Graphics;
use crate::error::{GfxError, Result};
use crate::shared::Shared;
use crate::texture::next_id;

#[derive(Debug)]
pub(crate) struct SurfaceState {
    pub(crate) module: wgpu::ShaderModule,
    pub(crate) generation: u32,
    pub(crate) uniforms: Vec<u8>,
    pub(crate) uniform_version: u32,
}

#[derive(Debug)]
pub(crate) struct SurfaceShaderInner {
    pub(crate) id: u64,
    pub(crate) uniform_size: usize,
    pub(crate) state: Mutex<SurfaceState>,
}

/// A game-written surface program for 3D materials. Set it as
/// [`Material::shader`](super::Material::shader).
///
/// The source defines `fn material(s: Surface) -> Surface`. The engine
/// fills `s` from the material's factors and textures (and decals), calls
/// the function, and lights what it returns with the scene's lights,
/// shadows, environment and fog, in every pass the material draws in. The
/// fields a program usually sets are `base` (linear colour), `alpha`,
/// `normal` (world space, unit length), `roughness`, `metallic`,
/// `emissive` (linear) and `occlusion`; it can read `world` (position),
/// `view` (unit vector towards the eye), `uv`, `uv2`, `color` (the vertex
/// colour), `geometric` (the mesh normal) and `layers` (the layered
/// features, such as `s.layers.clearcoat`). `scene_time()` returns the
/// scene's time in seconds. A program made with
/// [`Graphics::compile_surface_shader_with_uniforms`] declares its block at
/// group 3, binding 0:
///
/// ```wgsl
/// struct Params { heat: f32, _pad: vec3<f32> };
/// @group(3) @binding(0) var<uniform> params: Params;
///
/// fn material(input: Surface) -> Surface {
///     var s = input;
///     let glow = 0.5 + 0.5 * sin(scene_time() * 2.0 + s.world.x * 3.0);
///     s.base = vec3<f32>(0.05, 0.04, 0.04);
///     s.roughness = 0.9;
///     s.emissive += vec3<f32>(1.0, 0.35, 0.05) * glow * params.heat;
///     return s;
/// }
/// ```
///
/// The shadow pass ignores the program: cut-out shadows follow the
/// material's own alpha. The Rust struct passed as uniforms must match
/// WGSL's uniform layout (16-byte alignment for `vec3` and `vec4`, a size
/// that is a multiple of 16). Cloning shares the shader.
#[derive(Clone, Debug)]
pub struct SurfaceShader {
    pub(crate) inner: Arc<SurfaceShaderInner>,
}

impl PartialEq for SurfaceShader {
    fn eq(&self, other: &Self) -> bool {
        self.inner.id == other.inner.id
    }
}

impl SurfaceShader {
    /// The shader's id, which names it in pipeline and material keys.
    pub(crate) fn id(&self) -> u64 {
        self.inner.id
    }

    /// The current module and its generation.
    pub(crate) fn module(&self) -> (wgpu::ShaderModule, u32) {
        let s = self.inner.state.lock();
        (s.module.clone(), s.generation)
    }

    /// The uniform values and their version.
    pub(crate) fn uniforms(&self) -> (u32, Vec<u8>) {
        let s = self.inner.state.lock();
        (s.uniform_version, s.uniforms.clone())
    }

    /// The uniform version alone, for interning materials cheaply.
    pub(crate) fn uniform_version(&self) -> u32 {
        self.inner.state.lock().uniform_version
    }

    /// Recompiles the program from new source, keeping its uniform values.
    /// On error the previous program stays in use and the error says why,
    /// with line numbers in `source`.
    pub fn reload_source(&self, gfx: &Graphics, source: &str) -> Result<()> {
        let module = compile(&gfx.shared, source, self.inner.uniform_size > 0)?;
        let mut state = self.inner.state.lock();
        state.module = module;
        state.generation = state.generation.wrapping_add(1);
        Ok(())
    }

    /// Replaces the uniform values. Draws recorded before the call keep the
    /// values they were recorded with. `T` must be the size the shader was
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

    /// The size of the uniform block in bytes; zero for none.
    pub fn uniform_size(&self) -> usize {
        self.inner.uniform_size
    }

    /// How many clones of this shader exist, itself included. A registry
    /// that keeps a clone to reload it can drop the entry when the count
    /// falls to one.
    pub fn share_count(&self) -> usize {
        Arc::strong_count(&self.inner)
    }
}

/// The number of engine lines before the game's first line.
fn engine_lines() -> u32 {
    mesh_source("").lines().count() as u32
}

/// Formats a compiler message with its line counted in the game's source.
fn describe(m: &wgpu::CompilationMessage) -> String {
    let base = engine_lines();
    match &m.location {
        Some(loc) if loc.line_number > base => format!(
            "line {}, column {}: {}",
            loc.line_number - base,
            loc.line_position,
            m.message
        ),
        Some(loc) => format!("engine line {}: {}", loc.line_number, m.message),
        None => m.message.clone(),
    }
}

/// Compiles `source` after the engine's surface program and builds one
/// pipeline from it, so naga's errors and interface mismatches come back
/// as an error.
fn compile(shared: &Shared, source: &str, has_uniforms: bool) -> Result<wgpu::ShaderModule> {
    let device = shared.device();
    let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("yowie surface shader"),
        source: wgpu::ShaderSource::Wgsl(mesh_source(source).into()),
    });
    let info = pollster::block_on(module.get_compilation_info());
    let messages: Vec<String> = info
        .messages
        .iter()
        .filter(|m| m.message_type == wgpu::CompilationMessageType::Error)
        .map(describe)
        .collect();
    if messages.is_empty() {
        let layouts = SceneLayouts::new(device);
        let key = MeshKey {
            pass: MeshPass::Opaque,
            shader: (1, has_uniforms),
            ..MeshKey::default()
        };
        let _pipeline = build_mesh(device, &layouts, &module, key);
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

impl Graphics {
    /// Compiles a surface program with no uniform block. Errors carry
    /// naga's message with line numbers in `source`.
    pub fn compile_surface_shader(&self, source: &str) -> Result<SurfaceShader> {
        self.new_surface_shader(source, Vec::new())
    }

    /// Compiles a surface program whose uniform block at group 3, binding
    /// 0 holds a `T`, starting with `initial`.
    pub fn compile_surface_shader_with_uniforms<T: Pod>(
        &self,
        source: &str,
        initial: &T,
    ) -> Result<SurfaceShader> {
        self.new_surface_shader(source, bytemuck::bytes_of(initial).to_vec())
    }

    fn new_surface_shader(&self, source: &str, uniforms: Vec<u8>) -> Result<SurfaceShader> {
        let module = compile(&self.shared, source, !uniforms.is_empty())?;
        Ok(SurfaceShader {
            inner: Arc::new(SurfaceShaderInner {
                id: next_id(),
                uniform_size: uniforms.len(),
                state: Mutex::new(SurfaceState {
                    module,
                    generation: 0,
                    uniforms,
                    uniform_version: 0,
                }),
            }),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn game_lines_follow_the_engine() {
        let game = "fn material(s: Surface) -> Surface {\n    return s;\n}\n";
        let composed = mesh_source(game);
        assert_eq!(
            composed.lines().nth(engine_lines() as usize),
            game.lines().next()
        );
    }
}
