//! The asset system: a directory of images loads on worker threads behind a
//! progress bar, a pack file supplies one more image, files that change on
//! disk reload in place (the example rewrites one every two seconds to
//! prove it), P packs the directory into a pack file for the next run, and a
//! settings slot remembers how often the example ran.
//!
//! `--dir <path>` picks the asset directory (default: a folder in the
//! temporary directory, created and filled on first run). Because the
//! example changes files, its headless run is only checked for a non-blank
//! frame.

#[path = "common/mod.rs"]
mod common;

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use yowie::asset::{AssetEvent, Compression, PackWriter, pack_dir};
use yowie::gfx::{Font, Sprite, Texture, TextureOptions};
use yowie::input::Key;
use yowie::rng::Rng;
use yowie::save::Store;
use yowie::{Color, Config, Context, Game, GameResult, Rect, TextureLoad, vec2};

const COUNT: usize = 12;
const PACK: &str = "assets.pak";

#[derive(Serialize, Deserialize, Default)]
struct Settings {
    runs: u32,
    dir: String,
}

struct Item {
    name: String,
    load: TextureLoad,
}

struct Assets {
    font: Font,
    items: Vec<Item>,
    badge: TextureLoad,
    dir: PathBuf,
    random: Rng,
    next_rewrite: f32,
    status: String,
    packed: String,
    seconds: f32,
}

/// Draws a shape on a coloured square, the content of every generated image.
fn shape_png(rng: &mut Rng) -> Vec<u8> {
    let bg = [
        rng.range(0, 200) as u8,
        rng.range(0, 200) as u8,
        rng.range(0, 200) as u8,
    ];
    let kind = rng.range(0, 3);
    let mut img = image::RgbaImage::new(64, 64);
    for (x, y, px) in img.enumerate_pixels_mut() {
        let (dx, dy) = (x as i32 - 32, y as i32 - 32);
        let inside = match kind {
            0 => dx * dx + dy * dy < 22 * 22,
            1 => dx.abs() < 20 && dy.abs() < 20,
            _ => dx.abs() + dy.abs() < 24,
        };
        *px = if inside {
            image::Rgba([255 - bg[0], 255 - bg[1], 255 - bg[2], 255])
        } else {
            image::Rgba([bg[0], bg[1], bg[2], 255])
        };
    }
    let mut out = Vec::new();
    // Encoding into memory cannot fail.
    let _ = img.write_to(
        &mut std::io::Cursor::new(&mut out),
        image::ImageFormat::Png,
    );
    out
}

fn image_name(i: usize) -> String {
    format!("images/shape{i:02}.png")
}

/// Creates the asset directory with generated images, and the pack file with
/// the badge image, when they are missing.
fn seed(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir.join("images"))?;
    let mut rng = Rng::new(5);
    for i in 0..COUNT {
        let path = dir.join(image_name(i));
        if !path.exists() {
            std::fs::write(path, shape_png(&mut rng))?;
        }
    }
    let pack = badge_pack(dir);
    if !pack.exists() {
        let file = std::fs::File::create(&pack)?;
        let mut writer = PackWriter::new(file, Compression::None)
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        writer
            .add("badge.png", &shape_png(&mut Rng::new(77)))
            .and_then(|()| writer.finish().map(|_| ()))
            .map_err(|e| std::io::Error::other(e.to_string()))?;
    }
    Ok(())
}

fn arg_value(name: &str) -> Option<String> {
    let mut args = std::env::args();
    while let Some(a) = args.next() {
        if a == name {
            return args.next();
        }
    }
    None
}

fn asset_dir() -> PathBuf {
    arg_value("--dir")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("yowie-assets-example").join("assets"))
}

/// The pack file the example seeds with the badge image.
fn badge_pack(dir: &Path) -> PathBuf {
    dir.parent().unwrap_or(dir).join("badge.pak")
}

/// The pack file P writes from the directory.
fn full_pack(dir: &Path) -> PathBuf {
    dir.parent().unwrap_or(dir).join(PACK)
}

impl Game for Assets {
    fn init(ctx: &mut Context) -> GameResult<Self> {
        let dir = asset_dir();
        let store = if ctx.headless() {
            Store::at(dir.parent().unwrap_or(&dir).join("settings"))?
        } else {
            Store::open("rs", "yowie", "assets-example")?
        };
        let mut settings: Settings = store.load_or("settings", Settings::default())?;
        settings.runs += 1;
        settings.dir = dir.display().to_string();
        store.write("settings", &settings)?;

        let items = (0..COUNT)
            .map(|i| Item {
                name: image_name(i),
                load: ctx.load_texture_async(&image_name(i), TextureOptions::default()),
            })
            .collect();
        let badge = ctx.load_texture_async("badge.png", TextureOptions::default());
        if ctx.headless() {
            // A headless run draws one frame, so wait for the workers.
            let end = std::time::Instant::now() + std::time::Duration::from_secs(10);
            while !ctx.assets.progress().is_complete() && std::time::Instant::now() < end {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        }
        Ok(Assets {
            font: ctx.gfx.default_font(),
            items,
            badge,
            status: format!("Run {} of this example; assets in {}", settings.runs, settings.dir),
            dir,
            random: Rng::new(5),
            next_rewrite: 2.0,
            packed: String::new(),
            seconds: 0.0,
        })
    }

    fn update(&mut self, ctx: &mut Context) -> GameResult {
        self.seconds += ctx.time.delta();
        if ctx.input.key_pressed(Key::Escape) {
            ctx.quit();
        }
        // Finished loads become textures here, on the game thread.
        for item in &mut self.items {
            item.load.poll(ctx)?;
        }
        self.badge.poll(ctx)?;
        for event in ctx.asset_events() {
            match event {
                AssetEvent::Reloaded { path, .. } => self.status = format!("Reloaded {path}"),
                AssetEvent::Failed { path, error, .. } => {
                    self.status = format!("Failed {path}: {error}");
                }
                _ => {}
            }
        }
        // Rewrite a random image now and then; the watcher notices, and the
        // texture the game already holds changes in place.
        if self.seconds >= self.next_rewrite {
            self.next_rewrite += 2.0;
            let i = self.random.range(0, COUNT as i32) as usize;
            let png = shape_png(&mut self.random);
            match std::fs::write(self.dir.join(image_name(i)), png) {
                Ok(()) => self.status = format!("Rewrote {} on disk", image_name(i)),
                Err(e) => self.status = e.to_string(),
            }
        }
        if ctx.input.key_pressed(Key::P) {
            let out = full_pack(&self.dir);
            self.packed = match pack_dir(&self.dir, &out, Compression::Zstd(9)) {
                Ok(()) => {
                    let size = std::fs::metadata(&out).map_or(0, |m| m.len());
                    format!("Packed {size} bytes into {}", out.display())
                }
                Err(e) => e.to_string(),
            };
        }
        Ok(())
    }

    fn draw(&mut self, ctx: &mut Context) -> GameResult {
        for (i, item) in self.items.iter().enumerate() {
            let x = 40.0 + (i % 6) as f32 * 130.0;
            let y = 140.0 + (i / 6) as f32 * 130.0;
            match item.load.texture() {
                Some(tex) => draw_image(ctx, tex, x, y),
                None => ctx
                    .gfx
                    .fill_rect(Rect::new(x, y, 96.0, 96.0), Color::rgb8(40, 40, 50)),
            }
            let name = item.name.rsplit('/').next().unwrap_or(&item.name);
            common::label(ctx, &self.font, name, vec2(x, y + 100.0), 14.0, common::DIM);
        }
        if let Some(tex) = self.badge.texture() {
            draw_image(ctx, tex, 760.0, 16.0);
            common::label(ctx, &self.font, "from pack", vec2(760.0, 116.0), 12.0, common::DIM);
        }
        common::panel(ctx, Rect::new(16.0, 16.0, 720.0, 110.0));
        let p = ctx.assets.progress();
        common::label(
            ctx,
            &self.font,
            &format!("Loaded {} of {}", p.done, p.total),
            vec2(28.0, 24.0),
            15.0,
            common::TEXT,
        );
        common::bar(ctx, Rect::new(28.0, 46.0, 696.0, 14.0), p.fraction(), common::ACCENT);
        common::label(ctx, &self.font, &self.status, vec2(28.0, 70.0), 14.0, common::TEXT);
        let hint = if self.packed.is_empty() {
            "P packs the directory into assets.pak, mounted behind the loose files on the next run."
                .to_string()
        } else {
            self.packed.clone()
        };
        common::label(ctx, &self.font, &hint, vec2(28.0, 92.0), 13.0, common::DIM);
        Ok(())
    }
}

fn draw_image(ctx: &mut Context, tex: &Texture, x: f32, y: f32) {
    ctx.gfx
        .draw_sprite(tex, &Sprite::at(vec2(x, y)).size(vec2(96.0, 96.0)));
}

fn main() -> Result<(), yowie::Error> {
    let dir = asset_dir();
    seed(&dir).map_err(|e| yowie::Error::Window(format!("cannot prepare {}: {e}", dir.display())))?;
    let mut config = Config::new("Yowie assets")
        .with_size(870, 440)
        .with_asset_dir(&dir)
        .with_pack(badge_pack(&dir))
        .with_pack(full_pack(&dir));
    config.hot_reload = true;
    yowie::run::<Assets>(config)
}
