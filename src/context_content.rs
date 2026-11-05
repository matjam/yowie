//! Loading GPU objects from assets through the context.

use bytemuck::Pod;
use yowie_asset::{Handle, LoadState};
use yowie_gfx::{Font, ImageData, Shader, Texture, TextureOptions};

use crate::content::{FontData, ShaderSource};
use crate::{Context, Error, Result};

/// A texture that loads in the background. Create it with
/// [`Context::load_texture_async`] and call [`poll`](TextureLoad::poll) each
/// frame until it returns the texture.
#[derive(Debug)]
pub struct TextureLoad {
    handle: Handle<ImageData>,
    options: TextureOptions,
    texture: Option<Texture>,
}

impl TextureLoad {
    /// The asset path.
    pub fn path(&self) -> &str {
        self.handle.path()
    }

    /// Where the file is in loading. A reload after the first success never
    /// puts it back to loading.
    pub fn state(&self) -> LoadState {
        self.handle.state()
    }

    /// Why loading failed, once it has.
    pub fn error(&self) -> Option<String> {
        self.handle.error().map(|e| e.to_string())
    }

    /// The texture, once [`poll`](TextureLoad::poll) made it.
    pub fn texture(&self) -> Option<&Texture> {
        self.texture.as_ref()
    }

    /// Uploads the image if the background load has finished, and returns
    /// the texture once there is one. The texture is registered for hot
    /// reload like one from [`Context::load_texture`].
    pub fn poll(&mut self, ctx: &mut Context) -> Result<Option<&Texture>> {
        if self.texture.is_none()
            && let Some(data) = self.handle.get()
        {
            let tex = Texture::from_data(&ctx.gfx, &data, self.options)?;
            ctx.services.track_texture(self.handle.clone(), &tex);
            self.texture = Some(tex);
        }
        Ok(self.texture.as_ref())
    }
}

impl Context {
    /// Loads a PNG, JPEG, Radiance HDR or KTX2 file from the assets and
    /// uploads it, waiting for the read. With hot reload on, the texture's
    /// pixels are replaced in place when the file changes: every clone,
    /// sprite and material that uses the texture shows the new image.
    ///
    /// KTX2 files of BC1, BC3, BC4, BC5 and BC7 blocks need
    /// [`Graphics::supports_bc`](yowie_gfx::Graphics::supports_bc).
    pub fn load_texture(&mut self, path: &str, options: TextureOptions) -> Result<Texture> {
        let handle = self.assets.load_now::<ImageData>(path)?;
        let data = handle
            .get()
            .ok_or_else(|| Error::Window(format!("{path} is not ready")))?;
        let tex = Texture::from_data(&self.gfx, &data, options)?;
        self.services.track_texture(handle, &tex);
        Ok(tex)
    }

    /// Starts decoding an image on a worker thread and returns at once.
    /// Follow it with [`Assets::progress`](yowie_asset::Assets::progress)
    /// for a progress bar and [`TextureLoad::poll`] for the texture.
    pub fn load_texture_async(&mut self, path: &str, options: TextureOptions) -> TextureLoad {
        TextureLoad {
            handle: self.assets.load::<ImageData>(path),
            options,
            texture: None,
        }
    }

    /// Loads WGSL source from the assets and compiles it as a 2D fragment
    /// shader with no uniforms. With hot reload on, the shader is
    /// recompiled in place when the file changes; a source that fails to
    /// compile is logged and the previous program keeps running.
    pub fn load_shader(&mut self, path: &str) -> Result<Shader> {
        let (handle, source) = self.shader_source(path)?;
        let shader = self.gfx.compile_shader(&source.0)?;
        self.services.track_shader(handle, &shader);
        Ok(shader)
    }

    /// Like [`load_shader`](Context::load_shader) for a shader with a
    /// uniform block holding `initial`.
    pub fn load_shader_with_uniforms<T: Pod>(&mut self, path: &str, initial: &T) -> Result<Shader> {
        let (handle, source) = self.shader_source(path)?;
        let shader = self.gfx.compile_shader_with_uniforms(&source.0, initial)?;
        self.services.track_shader(handle, &shader);
        Ok(shader)
    }

    fn shader_source(
        &mut self,
        path: &str,
    ) -> Result<(Handle<ShaderSource>, std::sync::Arc<ShaderSource>)> {
        let handle = self.assets.load_now::<ShaderSource>(path)?;
        let source = handle
            .get()
            .ok_or_else(|| Error::Window(format!("{path} is not ready")))?;
        Ok((handle, source))
    }

    /// Loads a TrueType or OpenType font from the assets.
    pub fn load_font(&mut self, path: &str) -> Result<Font> {
        let handle = self.assets.load_now::<FontData>(path)?;
        let data = handle
            .get()
            .ok_or_else(|| Error::Window(format!("{path} is not ready")))?;
        Ok(Font::from_bytes(&mut self.gfx, data.0.clone())?)
    }

    /// Loads and decodes a WAV, Ogg Vorbis, MP3 or FLAC file from the
    /// assets as a sound at the mixer's rate. To play it, call
    /// `ctx.audio.play`. Long music belongs in
    /// [`Music`](yowie_audio::Music), which streams from a file.
    #[cfg(feature = "audio")]
    pub fn load_sound(&mut self, path: &str) -> Result<yowie_audio::Sound> {
        let handle = self.assets.load_now::<yowie_audio::Sound>(path)?;
        let sound = handle
            .get()
            .ok_or_else(|| Error::Window(format!("{path} is not ready")))?;
        Ok((*sound).clone())
    }

    /// Loads a Tiled map (TMX or TMJ) with its external tilesets. With hot
    /// reload on, the handle's `generation` increases when the map or a
    /// tileset changes; compare it each frame to rebuild what the map
    /// drives.
    #[cfg(feature = "tiled")]
    pub fn load_map(&mut self, path: &str) -> Result<Handle<yowie_tiled::Map>> {
        Ok(self.assets.load_now::<yowie_tiled::Map>(path)?)
    }
}
