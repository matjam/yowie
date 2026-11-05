//! Per-frame upkeep of the services behind a [`Context`]: audio, asset
//! events and live reload of GPU objects.

use yowie_asset::{AssetEvent, Assets, Handle};
use yowie_gfx::{Graphics, ImageData, Shader, Texture};

use crate::Context;
use crate::content::ShaderSource;

/// A texture built from an asset, and the generation it shows.
#[derive(Debug)]
struct TrackedTexture {
    handle: Handle<ImageData>,
    generation: u64,
    texture: Texture,
}

/// A shader built from an asset, and the generation it runs.
#[derive(Debug)]
struct TrackedShader {
    handle: Handle<ShaderSource>,
    generation: u64,
    shader: Shader,
}

/// The state the engine keeps for its services.
#[derive(Debug)]
pub(crate) struct Services {
    #[cfg(feature = "audio")]
    audio: crate::audio_backend::AudioBackend,
    textures: Vec<TrackedTexture>,
    shaders: Vec<TrackedShader>,
    pub(crate) events: Vec<AssetEvent>,
    pub(crate) schedule_stats: Vec<yowie_ecs::SystemStat>,
    pub(crate) schedule_updates: u64,
}

impl Services {
    pub(crate) fn new(#[cfg(feature = "audio")] audio: crate::audio_backend::AudioBackend) -> Self {
        Self {
            #[cfg(feature = "audio")]
            audio,
            textures: Vec::new(),
            shaders: Vec::new(),
            events: Vec::new(),
            schedule_stats: Vec::new(),
            schedule_updates: 0,
        }
    }

    /// Keeps `texture` showing the newest value of `handle`.
    pub(crate) fn track_texture(&mut self, handle: Handle<ImageData>, texture: &Texture) {
        self.textures.push(TrackedTexture {
            generation: handle.generation(),
            handle,
            texture: texture.clone(),
        });
    }

    /// Keeps `shader` running the newest source of `handle`.
    pub(crate) fn track_shader(&mut self, handle: Handle<ShaderSource>, shader: &Shader) {
        self.shaders.push(TrackedShader {
            generation: handle.generation(),
            handle,
            shader: shader.clone(),
        });
    }

    /// Collects asset events and rewrites the GPU objects whose assets
    /// were reloaded. A failed rewrite keeps the old contents and logs.
    fn reload(&mut self, assets: &Assets, gfx: &Graphics) {
        self.events = assets.poll();
        self.textures.retain(|t| t.texture.share_count() > 1);
        self.shaders.retain(|s| s.shader.share_count() > 1);
        for t in &mut self.textures {
            let generation = t.handle.generation();
            if generation == t.generation {
                continue;
            }
            t.generation = generation;
            let Some(data) = t.handle.get() else { continue };
            match t.texture.replace_data(gfx, &data) {
                Ok(()) => log::info!("reloaded texture {}", t.handle.path()),
                Err(e) => log::warn!("texture {} not reloaded: {e}", t.handle.path()),
            }
        }
        for s in &mut self.shaders {
            let generation = s.handle.generation();
            if generation == s.generation {
                continue;
            }
            s.generation = generation;
            let Some(source) = s.handle.get() else {
                continue;
            };
            match s.shader.reload_source(gfx, &source.0) {
                Ok(()) => log::info!("reloaded shader {}", s.handle.path()),
                Err(e) => log::warn!("shader {} not reloaded: {e}", s.handle.path()),
            }
        }
    }
}

impl Context {
    /// Runs before a frame's updates: collects asset events and reloads
    /// changed textures and shaders in place.
    pub(crate) fn begin_services(&mut self) {
        self.services.reload(&self.assets, &self.gfx);
    }

    /// Runs after a frame's updates with the game time they advanced.
    pub(crate) fn end_services(&mut self, seconds: f64) {
        #[cfg(feature = "audio")]
        self.services.audio.update(&self.audio, seconds);
        #[cfg(not(feature = "audio"))]
        let _ = seconds;
    }

    /// The asset events of the current frame: finished loads, changed
    /// files, reloads and failures, as [`Assets::poll`] returned them
    /// before the frame's updates.
    pub fn asset_events(&self) -> &[AssetEvent] {
        &self.services.events
    }
}
