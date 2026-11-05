//! Hot reload through the context: a changed file rewrites the texture and
//! shader the game already holds, and the headless mixer works.

use std::path::Path;
use std::time::{Duration, Instant};

use yowie_asset::AssetEvent;
use yowie_core::{UVec2, vec2};
use yowie_gfx::{Graphics, GraphicsSettings, Sprite, Texture, TextureOptions};

use crate::context::Host;
use crate::{Config, Context};

fn context(dir: &Path) -> Option<Context> {
    if !yowie_gfx::adapter_available() {
        println!("skipping: no GPU adapter");
        return None;
    }
    let size = UVec2::new(16, 16);
    let gfx = Graphics::new_headless(size, GraphicsSettings::default()).ok()?;
    let host = Host::Headless {
        title: "test".into(),
        fullscreen: false,
        size,
    };
    let mut config = Config::default().with_asset_dir(dir);
    config.hot_reload = true;
    Context::new(gfx, host, &config).ok()
}

fn png(rgba: [u8; 4]) -> Vec<u8> {
    let mut out = Vec::new();
    image::RgbaImage::from_pixel(2, 2, image::Rgba(rgba))
        .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
        .unwrap_or_else(|e| panic!("encode: {e}"));
    out
}

/// Draws `tex` over the whole output, optionally through a shader, and
/// returns the centre pixel.
fn centre(ctx: &mut Context, tex: &Texture, shader: Option<&yowie_gfx::Shader>) -> [u8; 4] {
    ctx.gfx.begin_frame();
    ctx.gfx.set_shader(shader);
    ctx.gfx
        .draw_sprite(tex, &Sprite::at(vec2(0.0, 0.0)).size(vec2(16.0, 16.0)));
    ctx.gfx.set_shader(None);
    ctx.gfx.end_frame().unwrap_or_else(|e| panic!("frame: {e}"));
    let img = ctx
        .gfx
        .read_output()
        .unwrap_or_else(|e| panic!("read: {e}"));
    img.get_pixel(8, 8).0
}

/// Calls the per-frame upkeep until `done` holds or ten seconds pass.
fn wait_for(ctx: &mut Context, mut done: impl FnMut(&mut Context) -> bool) -> bool {
    let end = Instant::now() + Duration::from_secs(10);
    while Instant::now() < end {
        ctx.begin_services();
        if done(ctx) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    false
}

#[test]
fn changed_files_replace_textures_and_shaders_in_place() {
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    let write = |name: &str, data: &[u8]| {
        std::fs::write(dir.path().join(name), data).unwrap_or_else(|e| panic!("write: {e}"));
    };
    write("tile.png", &png([255, 0, 0, 255]));
    let shader =
        |c: &str| format!("fn fragment(input: SpriteInput) -> vec4<f32> {{ return {c}; }}");
    write(
        "fx.wgsl",
        shader("vec4<f32>(0.0, 0.0, 1.0, 1.0)").as_bytes(),
    );
    let Some(mut ctx) = context(dir.path()) else {
        return;
    };

    let tex = ctx
        .load_texture("tile.png", TextureOptions::default())
        .unwrap_or_else(|e| panic!("load texture: {e}"));
    let held = tex.clone();
    assert_eq!(centre(&mut ctx, &held, None), [255, 0, 0, 255]);

    write("tile.png", &png([0, 255, 0, 255]));
    let mut reloaded = false;
    let seen = wait_for(&mut ctx, |ctx| {
        reloaded |= ctx
            .asset_events()
            .iter()
            .any(|e| matches!(e, AssetEvent::Reloaded { path, .. } if path == "tile.png"));
        reloaded
    });
    assert!(seen, "no reload event for tile.png");
    // The same Texture object now holds the new pixels.
    assert_eq!(centre(&mut ctx, &held, None), [0, 255, 0, 255]);
    assert_eq!(tex, held);

    let fx = ctx
        .load_shader("fx.wgsl")
        .unwrap_or_else(|e| panic!("load shader: {e}"));
    assert_eq!(centre(&mut ctx, &tex, Some(&fx)), [0, 0, 255, 255]);
    write(
        "fx.wgsl",
        shader("vec4<f32>(1.0, 1.0, 0.0, 1.0)").as_bytes(),
    );
    let changed = wait_for(&mut ctx, |ctx| {
        centre(ctx, &tex, Some(&fx)) == [255, 255, 0, 255]
    });
    assert!(changed, "shader did not reload");
    // A broken edit keeps the program that works.
    write("fx.wgsl", b"fn fragment(");
    // The file reloads as text; the shader rejects it and logs.
    let seen = wait_for(&mut ctx, |ctx| {
        ctx.asset_events()
            .iter()
            .any(|e| matches!(e, AssetEvent::Reloaded { path, .. } if path == "fx.wgsl"))
    });
    assert!(seen, "no reload event for fx.wgsl");
    assert_eq!(centre(&mut ctx, &tex, Some(&fx)), [255, 255, 0, 255]);
}

#[cfg(feature = "audio")]
#[test]
fn headless_context_has_a_working_mixer() {
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    let Some(mut ctx) = context(dir.path()) else {
        return;
    };
    assert!(ctx.headless());
    let sound = ctx
        .audio
        .sound(&yowie_audio::Pcm::sine(440.0, 0.05, 44_100))
        .unwrap_or_else(|e| panic!("sound: {e}"));
    let voice = ctx.audio.play(&sound, yowie_audio::PlayOptions::default());
    for _ in 0..12 {
        ctx.end_services(1.0 / 60.0);
    }
    assert!(voice.is_finished());
}
