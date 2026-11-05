//! A map in the Tiled editor's format, built into a drawable level:
//! layers, an external tileset, flipped and rotated tiles, an animated
//! pond and object outlines. The map, its tileset and the tileset's image
//! are made in code and served from memory, so the example needs no files;
//! `TiledLevel::load` reads them through the same `Vfs` it would use for a
//! game's asset directory. Arrow keys pan, Escape quits.

#[path = "common/art.rs"]
mod art;

use std::fmt::Write as _;

use art::{Canvas, hash, shade};
use yowie::asset::{MemorySource, Vfs};
use yowie::gfx::image::ImageFormat;
use yowie::gfx::{Camera2D, LevelLayerKind, TextStyle, TiledLevel};
use yowie::input::Key;
use yowie::{Color, Config, Context, Game, GameResult, Vec2, vec2};

const W: usize = 24;
const H: usize = 14;

const FLIP_H: u32 = 0x8000_0000;
const FLIP_V: u32 = 0x4000_0000;
const FLIP_D: u32 = 0x2000_0000;

// Tile ids in the tileset, which a layer writes plus one.
const GRASS: u32 = 1;
const DIRT: u32 = 2;
const WATER: u32 = 3;
const ARROW: u32 = 7;
const STONE: u32 = 8;
const FLOWER: u32 = 11;
const SHADOW: u32 = 12;

fn tileset_image() -> Vec<u8> {
    let mut c = Canvas::new(8 * 16, 2 * 16);
    c.paint(0, 0, 16, |x, y| {
        Some(shade(
            [84, 150, 74],
            0.9 + f32::from(hash(x, y, 1)) / 255.0 * 0.2,
        ))
    });
    c.paint(16, 0, 16, |x, y| {
        Some(shade(
            [158, 120, 78],
            0.9 + f32::from(hash(x, y, 2)) / 255.0 * 0.2,
        ))
    });
    for f in 0..3 {
        c.paint(32 + f * 16, 0, 16, |x, y| {
            let wave = (x + f * 5 + y * 2) % 8;
            Some(if wave < 2 {
                [126, 188, 238, 255]
            } else {
                shade(
                    [52, 112, 192],
                    0.95 + f32::from(hash(x, y, 3)) / 255.0 * 0.1,
                )
            })
        });
    }
    // An arrow pointing up with a dot in the top-left corner, so every
    // flip and turn is visible.
    c.rect(96, 0, 16, 16, [236, 226, 196, 255]);
    c.rect(96 + 7, 5, 2, 10, [190, 60, 50, 255]);
    for i in 0..5 {
        c.rect(96 + 8 - i, 2 + i, 2 * i + 1, 1, [190, 60, 50, 255]);
    }
    c.rect(96 + 1, 1, 3, 3, [40, 70, 160, 255]);
    c.paint(112, 0, 16, |x, y| {
        let mortar = y % 8 == 0 || (x + if (y / 8) % 2 == 0 { 0 } else { 8 }) % 16 == 0;
        Some(if mortar {
            [84, 86, 96, 255]
        } else {
            shade(
                [136, 138, 148],
                0.9 + f32::from(hash(x, y, 7)) / 255.0 * 0.2,
            )
        })
    });
    c.rect(0, 16, 16, 16, [0, 0, 0, 0]);
    c.rect(16 + 3, 16 + 4, 10, 9, [150, 100, 54, 255]);
    c.rect(16 + 3, 16 + 8, 10, 1, [96, 62, 32, 255]);
    c.disc(32.0 + 8.0, 16.0 + 8.0, 2.5, [250, 232, 110, 255]);
    c.disc(32.0 + 8.0, 16.0 + 4.0, 1.6, [244, 110, 140, 255]);
    c.disc(32.0 + 8.0, 16.0 + 12.0, 1.6, [244, 110, 140, 255]);
    c.paint(48, 16, 16, |_, _| Some([10, 20, 40, 255]));
    let mut png = std::io::Cursor::new(Vec::new());
    // Writing into memory cannot fail.
    let _ = c.img.write_to(&mut png, ImageFormat::Png);
    png.into_inner()
}

fn csv(rows: impl Fn(usize, usize) -> u32) -> String {
    let mut s = String::new();
    for y in 0..H {
        for x in 0..W {
            let _ = write!(s, "{}", rows(x, y));
            if x + 1 < W || y + 1 < H {
                s.push(',');
            }
        }
        s.push('\n');
    }
    s
}

fn map_tmx() -> String {
    let ground = csv(|x, y| {
        let pond = (x as f32 - 17.0).powi(2) / 25.0 + (y as f32 - 9.0).powi(2) / 9.0 < 1.0;
        if pond {
            WATER
        } else if y == 6 || (x == 5 && y > 6) {
            DIRT
        } else if hash(x as i32, y as i32, 5) > 238 {
            STONE
        } else {
            GRASS
        }
    });
    // Eight copies of the arrow with every combination of the three flip
    // flags, in a row.
    let flags = [
        0,
        FLIP_H,
        FLIP_V,
        FLIP_H | FLIP_V,
        FLIP_D,
        FLIP_D | FLIP_H,
        FLIP_D | FLIP_V,
        FLIP_D | FLIP_H | FLIP_V,
    ];
    let decor = csv(|x, y| {
        if y == 2 && (2..10).contains(&x) {
            return ARROW | flags[x - 2];
        }
        if y > 7 && x < 4 && hash(x as i32, y as i32, 11) > 150 {
            return FLOWER;
        }
        0
    });
    let shade_layer = csv(|x, y| {
        if (10..14).contains(&x) && (1..4).contains(&y) {
            SHADOW
        } else {
            0
        }
    });
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<map version="1.10" orientation="orthogonal" renderorder="right-down" width="{W}" height="{H}" tilewidth="16" tileheight="16" infinite="0" nextlayerid="6" nextobjectid="6">
 <tileset firstgid="1" source="tiles/terrain.tsx"/>
 <layer id="1" name="ground" width="{W}" height="{H}"><data encoding="csv">
{ground}</data></layer>
 <layer id="2" name="decor" width="{W}" height="{H}"><data encoding="csv">
{decor}</data></layer>
 <layer id="3" name="shade" opacity="0.45" offsetx="6" offsety="5" width="{W}" height="{H}"><data encoding="csv">
{shade_layer}</data></layer>
 <objectgroup id="4" name="objects">
  <object id="1" name="spawn" x="32" y="64" width="48" height="32"/>
  <object id="2" name="zone" x="228" y="24" width="72" height="44"><ellipse/></object>
  <object id="3" name="route" x="112" y="176"><polyline points="0,0 40,-30 88,10 130,-20"/></object>
  <object id="4" name="fence" x="44" y="132"><polygon points="0,0 56,0 76,28 18,36"/></object>
  <object id="5" name="flag" x="340" y="180"><point/></object>
 </objectgroup>
</map>
"#
    )
}

fn tileset_tsx() -> String {
    r#"<?xml version="1.0" encoding="UTF-8"?>
<tileset version="1.10" name="terrain" tilewidth="16" tileheight="16" tilecount="16" columns="8">
 <image source="terrain.png" width="128" height="32"/>
 <tile id="2"><animation><frame tileid="2" duration="300"/><frame tileid="3" duration="300"/><frame tileid="4" duration="300"/><frame tileid="3" duration="300"/></animation></tile>
</tileset>
"#
    .to_string()
}

struct TiledExample {
    level: TiledLevel,
    camera: Camera2D,
    font: yowie::gfx::Font,
}

impl Game for TiledExample {
    fn init(ctx: &mut Context) -> GameResult<Self> {
        let files = MemorySource::new()
            .with("maps/level.tmx", map_tmx().into_bytes())?
            .with("maps/tiles/terrain.tsx", tileset_tsx().into_bytes())?
            .with("maps/tiles/terrain.png", tileset_image())?;
        let mut vfs = Vfs::new();
        vfs.mount_memory(files);
        let level = TiledLevel::load(&ctx.gfx, &vfs, "maps/level.tmx")?;
        let b = level.bounds();
        Ok(TiledExample {
            camera: Camera2D::new(b.center()).with_zoom(2.2),
            level,
            font: ctx.gfx.default_font(),
        })
    }

    fn update(&mut self, ctx: &mut Context) -> GameResult {
        let dt = ctx.time.delta();
        if ctx.input.key_pressed(Key::Escape) {
            ctx.quit();
        }
        let mut pan = Vec2::ZERO;
        for (key, d) in [
            (Key::Left, vec2(-1.0, 0.0)),
            (Key::Right, vec2(1.0, 0.0)),
            (Key::Up, vec2(0.0, -1.0)),
            (Key::Down, vec2(0.0, 1.0)),
        ] {
            if ctx.input.key_down(key) {
                pan += d;
            }
        }
        self.camera.position += pan * 160.0 * dt;
        self.level.advance(dt);
        Ok(())
    }

    fn draw(&mut self, ctx: &mut Context) -> GameResult {
        let g = &mut ctx.gfx;
        g.with_camera(&self.camera, |g| {
            self.level.draw(g);
            g.set_layer(1);
            self.level.draw_objects(g, 1.5, Color::rgb8(255, 224, 80));
        });
        g.set_layer(yowie::gfx::layers::UI);
        let style = TextStyle::new(15.0).color(Color::WHITE);
        let mut y = 12.0;
        for layer in self.level.layers() {
            let what = match &layer.kind {
                LevelLayerKind::Tiles(maps) => format!("tile layer, {} draw call(s)", maps.len()),
                LevelLayerKind::Objects(o) => format!("object layer, {} objects", o.len()),
                LevelLayerKind::Image { .. } => "image layer".to_string(),
            };
            let text = format!(
                "{}: {} (opacity {:.2}, offset {:.0},{:.0})",
                layer.name, what, layer.opacity, layer.offset.x, layer.offset.y
            );
            g.draw_text(&self.font, &text, vec2(12.0, y), &style);
            y += 20.0;
        }
        g.draw_text(
            &self.font,
            "Eight arrows: every combination of the horizontal, vertical and diagonal flip flags",
            vec2(12.0, 500.0),
            &TextStyle::new(14.0).color(Color::rgb8(255, 224, 120)),
        );
        Ok(())
    }
}

fn main() -> Result<(), yowie::Error> {
    let config = Config {
        clear_color: Color::rgb8(18, 22, 30),
        ..Config::new("Yowie tiled").with_size(960, 540)
    };
    yowie::run::<TiledExample>(config)
}
