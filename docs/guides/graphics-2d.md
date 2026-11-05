---
title: 2D graphics
group: Graphics
order: 1
summary: coordinates and colour, textures, sprites, sheets and atlases, nine-slices, shapes, cameras, tilemaps and autotiling, Tiled maps, text, vector paths, particles, 2D lights, render textures, screenshots and batching
---

# 2D graphics

All 2D drawing goes through `ctx.gfx`, a `yowie::gfx::Graphics`. Every
`draw_*`, `fill_*` and `stroke_*` call queues triangles; the engine renders
the frame after your `Game::draw` returns. Call order matters only inside
one layer and sort key, as the section on draw order explains.

This guide covers the 2D half of `yowie::gfx` with `yowie::particle` and
`yowie::tiled`, in the order a game meets them. The 3D renderer and
post-processing are in [3D graphics](graphics-3d.md), game-written shaders
in [shaders](shaders.md), frame-by-frame and keyframe animation in
[animation](animation.md), and the widget toolkit in [the
interface](ui.md). For the full API, run `cargo doc --open`.

These examples show the features below. Each one accepts `--shot
<file.png>` to render one frame headless and `--seconds <n>` to stop after
n seconds:

| Example | Shows |
|---|---|
| `cargo run --example clear` | the smallest program: a clear colour that changes over time |
| `cargo run --example sprites` | tinted, rotated sprites, shapes, blend modes and a lit floor with shadows; `--post` adds 2D post-processing |
| `cargo run --example tiles` | a sprite sheet, layered tilemaps, a camera rig with dead zone and shake, a walk cycle and a nine-slice HUD |
| `cargo run --example autotile` | blob, edge, Wang and hexagonal autotiling that you paint with the mouse |
| `cargo run --example tiled` | a Tiled map with an external tileset, flipped tiles, an animated tile and object outlines |
| `cargo run --example text` | shaping, fallback fonts, wrapping, hyphenation, style markup, vertical and distance-field text, hit testing |
| `cargo run --example vector` | paths, fill rules, caps, joins, dashes, gradients, textured fills and every blend mode |
| `cargo run --example particles` | fire, smoke, rain, sparks and confetti on the CPU and a fountain on the GPU; `--gpu` starts every effect on the GPU |
| `cargo run --example roguelike` | a turn-based game drawn with text glyphs; `--autoplay` plays itself |
| `cargo run --example gallery` | the interface toolkit, nine-slice skins and a particle editor that saves JSON |

## Coordinates and the view

2D positions are view units with the origin at the top-left of the view
and +Y pointing down. Angles are radians, and a positive angle turns
clockwise on screen. Sizes, line widths and font sizes are view units
too.

By default a view unit is one logical point of the window, so a window
of 1280 by 720 points is 1280 by 720 view units whatever the display's
pixel density. To read the current size, call `ctx.gfx.view_size()`. It
changes when the player resizes the window.

To design at a fixed resolution instead, give the configuration a fixed
view with `Config::with_view`. The view then keeps that size in view
units, and the engine scales it into the window by the `ScaleMode` you
choose:

| `ScaleMode` | Behaviour |
|---|---|
| `Letterbox` (default) | The largest scale that keeps the aspect ratio, centred, with black bars on two sides. |
| `Integer` | The largest whole-number scale that fits, centred, with bars around it. Keeps pixel art crisp. |
| `Stretch` | Fills the window and distorts the aspect ratio. |

```rust,no_run
use yowie::gfx::{Sprite, Texture, TextureOptions};
use yowie::{Config, Context, Game, GameResult, ScaleMode, vec2};

struct Shooter {
    ship: Texture,
}

impl Game for Shooter {
    fn init(ctx: &mut Context) -> GameResult<Self> {
        let ship = ctx.load_texture("sprites/ship.png", TextureOptions::default())?;
        Ok(Shooter { ship })
    }

    fn draw(&mut self, ctx: &mut Context) -> GameResult {
        // The view is 320 by 180 units at any window size.
        let view = ctx.gfx.view_size();
        let at = vec2(view.x * 0.5, view.y - 24.0);
        ctx.gfx.draw_sprite(&self.ship, &Sprite::at(at).centered());
        Ok(())
    }
}

fn main() -> Result<(), yowie::Error> {
    yowie::run::<Shooter>(Config::new("Shooter").with_view(320, 180, ScaleMode::Integer))
}
```

A fixed view still renders at the window's full pixel resolution. A
sprite scaled by 3 shows each texel as a 3 by 3 block, and text and
vector paths stay sharp because they rasterise at output pixels. To find
how many output pixels one view unit covers, call
`ctx.gfx.pixels_per_unit()`.

The pointer is in view units as well: `ctx.input.mouse()` needs no
conversion. Over the letterbox bars it lies outside the view rectangle.
To convert a position in window pixels yourself, call
`ctx.gfx.window_to_view`. The [window guide](window.md) covers window
size, full screen and display density.

## Colour

`yowie::Color` holds sRGB components from 0 to 1 with straight (not
premultiplied) alpha, which is how colour pickers and image editors give
colours. Every constructor takes sRGB: `Color::rgb`, `rgba`, `rgb8`,
`rgba8`, `hex` (`0xRRGGBB`), `hexa` (`0xRRGGBBAA`), `parse_hex`
(`"#rgb"` to `"#rrggbbaa"`), `hsv` (hue in degrees) and the constants
such as `Color::WHITE` and `Color::ORANGE`. The one exception is
`Color::from_linear`, which takes linear-light components.

```rust
use yowie::Color;

let ember = Color::rgb8(255, 120, 40);
let glass = Color::hex(0x88c0d0).with_alpha(0.4);
let sky = Color::hsv(210.0, 0.5, 0.9);
assert!(Color::parse_hex("#ff7828").is_some());

// sRGB mid-grey is about 0.21 in linear light.
let [r, _, _, _] = Color::rgb(0.5, 0.5, 0.5).to_linear();
assert!(r > 0.2 && r < 0.22);
assert!(ember.a == 1.0 && glass.a < 1.0 && sky.b > sky.r);
```

The renderer converts every colour to linear light and premultiplies it
by alpha, blends in linear light, and stores textures premultiplied.
Images are decoded as sRGB with straight alpha and premultiplied on
upload. Two texture options change that, as the next section shows:
`premultiplied` for pixels that are already premultiplied, and `data`
for pixels that are not colour at all (masks and normal maps). A
game-written fragment shader returns premultiplied colour in linear light;
see [shaders](shaders.md).

`Color::lerp` mixes in sRGB space. Gradients on vector paths, by
contrast, mix in linear light.

## Loading textures

To load an image from the game's assets, call `ctx.load_texture` with a
path and `TextureOptions`. It reads PNG, JPEG, Radiance HDR and KTX2
files from the `assets` directory (in the working directory and beside
the executable), from `Config::asset_dir` and from pack files. With
`Config::hot_reload`, which is on in debug builds, saving the file
replaces the texture's pixels in place, and every sprite that uses it
shows the new image.

```rust,no_run
use yowie::gfx::{Texture, TextureOptions, Wrap};
use yowie::{Context, GameResult};

fn load_art(ctx: &mut Context) -> GameResult<(Texture, Texture)> {
    // Pixel art: the defaults, nearest filtering and clamped edges.
    let hero = ctx.load_texture("sprites/hero.png", TextureOptions::default())?;
    // A background drawn scaled and tiled: smooth filtering with mipmaps.
    let clouds = ctx.load_texture(
        "backgrounds/clouds.png",
        TextureOptions {
            wrap: Wrap::Repeat,
            ..TextureOptions::smooth()
        },
    )?;
    Ok((hero, clouds))
}
```

`TextureOptions` has five fields. The default suits pixel art;
`TextureOptions::smooth()` turns on linear filtering and mipmaps.

| Field | Default | Effect |
|---|---|---|
| `filter` | `Filter::Nearest` | `Nearest` keeps texels square; `Linear` blends neighbouring texels when the texture is scaled or rotated. |
| `wrap` | `Wrap::Clamp` | What sampling outside the texture returns: the edge texel, the texture repeated (`Repeat`) or repeated and mirrored (`Mirror`). |
| `mipmaps` | `false` | Builds a mip chain so a texture drawn smaller than its size does not shimmer. The chain is rebuilt on every pixel update. |
| `premultiplied` | `false` | Says the pixels are already premultiplied, so the engine does not premultiply them again. |
| `data` | `false` | Stores the pixels without colour conversion, for masks and normal maps. |

To load without waiting, call `ctx.load_texture_async`. It decodes on a
worker thread and returns a `TextureLoad`; call `poll` each frame until
it hands back the texture.

```rust,no_run
use yowie::gfx::TextureOptions;
use yowie::{Context, Game, GameResult, TextureLoad, Vec2};

struct Title {
    backdrop: TextureLoad,
}

impl Game for Title {
    fn init(ctx: &mut Context) -> GameResult<Self> {
        let backdrop = ctx.load_texture_async("backgrounds/title.png", TextureOptions::smooth());
        Ok(Title { backdrop })
    }

    fn draw(&mut self, ctx: &mut Context) -> GameResult {
        if let Some(texture) = self.backdrop.poll(ctx)? {
            ctx.gfx.draw_texture(texture, Vec2::ZERO);
        }
        Ok(())
    }
}
```

To make a texture from pixels the game generates, call
`Texture::from_pixels` with the width, height and RGBA8 bytes, row by
row from the top-left. `Texture::from_image` takes an
`image::RgbaImage` (re-exported as `yowie::gfx::image`),
`Texture::from_bytes` decodes PNG or JPEG bytes, and
`Texture::from_file_bytes` also accepts HDR and KTX2. KTX2 files of BC1,
BC3, BC4, BC5 and BC7 blocks need `ctx.gfx.supports_bc()`.

To change part of a texture later, call `Texture::update_pixels` with a
rectangle and its RGBA8 bytes. Draws queued earlier in the same frame see
the new pixels. To swap in a whole new image, even of another size, call
`Texture::replace_data`; the texture keeps its identity, so every clone
and sprite shows the new image.

Cloning a `Texture` shares the image. The GPU memory is freed when the
last clone and every queued draw that uses it are gone.

## Sprites

To draw a texture, call `Graphics::draw_sprite` with a `Sprite`. Start
from `Sprite::at(position)` and chain the builder methods for the rest.

```rust,no_run
use yowie::gfx::{Graphics, Sprite, Texture};
use yowie::{Color, Rect, vec2};

fn draw_ship(gfx: &mut Graphics, ships: &Texture, heading: f32) {
    let sprite = Sprite::at(vec2(160.0, 90.0))
        .source(Rect::new(32.0, 0.0, 32.0, 32.0))
        .centered()
        .rotation(heading)
        .scale_by(2.0)
        .tint(Color::rgb8(255, 220, 200));
    gfx.draw_sprite(ships, &sprite);
}
```

| Field | Builder | Default | Meaning |
|---|---|---|---|
| `position` | `Sprite::at` | origin | Where the pivot lands, in drawing coordinates. |
| `size` | `size` | `None` | The drawn size before scaling; `None` uses the source rectangle's size in texels. |
| `scale` | `scale`, `scale_by` | `(1, 1)` | Scale about the pivot. |
| `rotation` | `rotation` | 0 | Clockwise turn about the pivot, in radians. |
| `origin` | `origin`, `centered` | `(0, 0)` | The pivot as a fraction of the size: `(0, 0)` is the top-left, `(0.5, 0.5)` the centre, `(0.5, 1.0)` the bottom middle. |
| `source` | `source` | `None` | The texel rectangle to draw; `None` draws the whole texture. |
| `tint` | `tint` | white | Multiplies the texture's colour, sRGB with straight alpha. A tint with alpha fades the sprite. |
| `flip_x`, `flip_y` | `flip_x`, `flip_y` | `false` | Mirrors the image. |

Two shorter calls cover common cases: `draw_texture(&texture, position)`
draws the whole texture at its own size, and `draw_texture_rect(&texture,
dest, tint)` stretches it over a rectangle.

Sprites wholly outside the visible area are dropped before they reach
the vertex stream, so drawing a level larger than the screen costs only
the test for the sprites off screen.

To find where a sprite lands, call `Sprite::corners` or `Sprite::bounds`
with the texture's size in texels. Both work without a GPU, so they suit
selection and broad collision checks. They do not look at transparent
texels.

```rust
use yowie::gfx::Sprite;
use yowie::{Rect, vec2};

let sprite = Sprite::at(vec2(100.0, 100.0)).size(vec2(32.0, 16.0)).centered();
let bounds = sprite.bounds(vec2(64.0, 64.0));
assert_eq!(bounds, Rect::new(84.0, 92.0, 32.0, 16.0));
```

## Draw order and state

Every draw carries a layer and a sort key. The frame draws layers in
ascending order; within a layer it draws lower sort keys first, and draws
with the same key in call order. Both default to 0.

To put later draws on a layer, call `set_layer`. To draw one stretch on a
layer and return to the previous one, call `with_layer`. To order sprites
within a layer by depth, such as actors in a top-down game sorted by their
feet, call `set_sort_key` before each draw.

```rust,no_run
use yowie::gfx::{Graphics, Sprite, Texture};
use yowie::{Vec2, vec2};

fn draw_world(gfx: &mut Graphics, ground: &Texture, actor: &Texture, feet: &[Vec2]) {
    gfx.with_layer(0, |g| g.draw_texture(ground, Vec2::ZERO));
    gfx.set_layer(1);
    for &foot in feet {
        gfx.set_sort_key(foot.y);
        gfx.draw_sprite(actor, &Sprite::at(foot).origin(vec2(0.5, 1.0)));
    }
    gfx.set_sort_key(0.0);
    gfx.set_layer(0);
}
```

The engine's own drawing uses high layers in `yowie::gfx::layers`: the
interface draws on `layers::UI` (`1 << 24`), the debug console on
`layers::CONSOLE` and the frame-timing overlay on `layers::DEBUG`. A game
layer below `layers::UI` draws under the interface.

Besides the layer and sort key, `Graphics` keeps a blend mode, a shader,
a camera, a clip rectangle and a transform. Each setter (`set_layer`,
`set_sort_key`, `set_blend`, `set_shader`, `set_camera`, `set_clip`,
`set_transform`) applies to every later draw until you change it again.
Each `with_*` method applies its setting for the closure and restores the
previous one. All of this state returns to its defaults at the start of
each frame.

To set the colour each frame starts with, set `Config::clear_color` or
call `ctx.gfx.set_clear_color`.

## Sprite sheets, atlases and Aseprite files

To cut a texture into a grid of equal frames, make a `SpriteSheet`.
Frames are numbered row by row from the top-left.
`SpriteSheet::with_spacing` handles a margin around the grid and spacing
between frames.

```rust,no_run
use yowie::gfx::{Graphics, Sprite, SpriteSheet, Texture};
use yowie::{uvec2, vec2};

fn make_coins(texture: Texture) -> SpriteSheet {
    SpriteSheet::new(texture, uvec2(16, 16))
}

fn draw_coin(gfx: &mut Graphics, coins: &SpriteSheet, frame: u32) {
    coins.draw_frame(gfx, frame, &Sprite::at(vec2(40.0, 40.0)).centered());
}
```

`draw_frame` replaces the sprite's source rectangle with the frame, and
uses the frame's size when the sprite has none. An index past the last
frame draws nothing. `frame_rect` and `frame_uv` give a frame's texel
rectangle and texture coordinates for drawing it some other way.

To pack many separate images into one texture at run time, use
`AtlasBuilder`. Add each image under a name, then call `build`. The
builder leaves `padding` texels (default 1) around each image and copies
the edge texels into that padding, so linear filtering at a region's
border never reads its neighbour. The atlas starts at 64 texels square
and doubles until everything fits or `max_size` (default 4096) is
reached.

```rust,no_run
use yowie::gfx::image::RgbaImage;
use yowie::gfx::{Atlas, AtlasBuilder, Graphics, Sprite, TextureOptions};
use yowie::{GameResult, vec2};

fn build_icons(gfx: &Graphics, icons: Vec<(String, RgbaImage)>) -> GameResult<Atlas> {
    let mut builder = AtlasBuilder::new();
    for (name, image) in icons {
        builder.add(name, image);
    }
    Ok(builder.build(gfx, TextureOptions::default())?)
}

fn draw_icon(gfx: &mut Graphics, icons: &Atlas) {
    icons.draw(gfx, "sword", &Sprite::at(vec2(8.0, 8.0)));
}
```

`Atlas::region(name)` returns a region's texel rectangle, and
`Atlas::new` wraps a texture you packed elsewhere with its regions. To
pack rectangles yourself, `AtlasPacker` places them in a fixed area and
never overlaps them:

```rust
use yowie::gfx::AtlasPacker;
use yowie::uvec2;

let mut packer = AtlasPacker::new(uvec2(256, 256));
if let Some(slot) = packer.pack(uvec2(48, 32)) {
    assert!(slot.x + slot.w <= 256 && slot.y + slot.h <= 256);
}
```

To use art from Aseprite, load the `.ase` or `.aseprite` file with
`AsepriteSheet::load`. It flattens the visible layers of every frame,
packs the frames into one sheet and keeps the file's tags, slices and
per-frame durations in `sheet.data`. `AsepriteSheet::flipbook` turns the
tags into a flipbook for `yowie::anim::FlipbookPlayer`, whose `frame()`
is the index to pass to `draw_frame`. The [animation guide](animation.md)
covers flipbook playback.

```rust,no_run
use std::sync::Arc;

use yowie::anim::{FlipbookPlayer, LoopMode};
use yowie::gfx::{AsepriteSheet, Graphics, Sprite, TextureOptions};
use yowie::{Context, GameResult, Vec2};

fn load_hero(ctx: &mut Context) -> GameResult<(AsepriteSheet, FlipbookPlayer)> {
    let path = "sprites/hero.aseprite";
    let hero = AsepriteSheet::load(&ctx.gfx, ctx.assets.vfs(), path, TextureOptions::default())?;
    let mut player = FlipbookPlayer::new(Arc::new(hero.flipbook()));
    player.play_tag("run", LoopMode::Loop);
    Ok((hero, player))
}

fn draw_hero(gfx: &mut Graphics, hero: &AsepriteSheet, player: &FlipbookPlayer, feet: Vec2) {
    if let Some(frame) = player.frame() {
        hero.draw_frame(gfx, frame, &Sprite::at(feet).origin(Vec2::new(0.5, 1.0)));
    }
}
```

A slice marks a region of the sprite, such as a hit box. To read it for
the frame on screen, call `hero.data.slice("hitbox")` and then
`key_at(frame)`; the key's `bounds` are in sprite pixels from the frame's
top-left, with an optional `pivot` and nine-slice `center`.

## Nine-slices

To draw a framed panel or speech bubble of any size from one small image,
make a `NineSlice` and call `draw_nine_slice`. The four border widths cut
the image into a 3 by 3 grid: the corners draw at their own size, the
edges stretch along one axis and the centre stretches along both.

```rust,no_run
use yowie::gfx::{Graphics, NineSlice, Texture};
use yowie::{Color, Rect};

fn make_frame(texture: Texture) -> NineSlice {
    NineSlice::uniform(texture, 6.0)
}

fn draw_dialog_box(gfx: &mut Graphics, frame: &NineSlice) {
    gfx.draw_nine_slice(frame, Rect::new(16.0, 120.0, 288.0, 48.0), Color::WHITE);
}
```

`NineSlice::uniform` uses one border width on every side. For different
widths, or a frame that is part of a larger texture, fill in the fields
(`source`, `left`, `top`, `right`, `bottom`; all in texels). Borders draw
at one view unit per texel. When the destination is smaller than two
borders, the borders shrink together. The interface toolkit draws its
skins with nine-slices; see [the interface](ui.md).

## Shapes

To draw flat shapes, call the shape methods on `Graphics`. They draw
through the same batch as sprites, so they sort, clip and transform the
same way. Widths and radii are view units.

| Method | Draws |
|---|---|
| `fill_rect(rect, color)` | a filled rectangle |
| `stroke_rect(rect, thickness, color)` | a border lying inside the rectangle |
| `line(a, b, thickness, color)` | a segment with square ends |
| `fill_triangle(a, b, c, color)` | a filled triangle |
| `fill_polygon(points, color)` | a filled convex polygon; concave polygons fill incorrectly |
| `stroke_polygon(points, thickness, color)` | a closed outline with mitred corners |
| `stroke_polyline(points, thickness, color)` | connected segments with mitred joins and square ends |
| `fill_circle(center, radius, color)` | a filled circle |
| `stroke_circle(center, radius, thickness, color)` | a ring centred on the radius |

```rust,no_run
use yowie::gfx::Graphics;
use yowie::{Color, Rect};

fn draw_health(gfx: &mut Graphics, fraction: f32) {
    let bar = Rect::new(8.0, 8.0, 100.0, 10.0);
    gfx.fill_rect(bar, Color::rgb8(40, 20, 20));
    let filled = Rect::new(bar.x, bar.y, bar.w * fraction.clamp(0.0, 1.0), bar.h);
    gfx.fill_rect(filled, Color::rgb8(220, 60, 60));
    gfx.stroke_rect(bar, 1.0, Color::WHITE);
}
```

Circles use enough segments to stay within a quarter pixel of the true
curve at the current zoom. Shapes have hard edges. For anti-aliased
edges, concave shapes, curves, dashes and gradients, use vector paths.

## Your own triangles

To draw geometry the game builds, such as a trail or a deformed sprite,
call `draw_triangles` with a texture (or `None` for vertex colours alone),
a slice of `Vertex` and indices, three per triangle. Texture coordinates
run from 0 to 1 across the texture; vertex colours are sRGB with straight
alpha and multiply the texture.

```rust,no_run
use yowie::gfx::{Graphics, Vertex};
use yowie::{Color, vec2};

fn draw_gradient_quad(gfx: &mut Graphics) {
    let top = Color::rgb8(40, 60, 140);
    let bottom = Color::rgb8(220, 120, 60);
    let vertices = [
        Vertex::new(vec2(0.0, 0.0), vec2(0.0, 0.0), top),
        Vertex::new(vec2(320.0, 0.0), vec2(1.0, 0.0), top),
        Vertex::new(vec2(320.0, 180.0), vec2(1.0, 1.0), bottom),
        Vertex::new(vec2(0.0, 180.0), vec2(0.0, 1.0), bottom),
    ];
    gfx.draw_triangles(None, &vertices, &[0, 1, 2, 0, 2, 3]);
}
```

The vertices are copied into the frame on every call; there is no
geometry kept on the GPU between frames. Triangles with an index past
the end of the vertices are dropped.

## Blend modes, tints and colour matrices

To change how drawing combines with what is behind it, set a
`BlendMode` with `set_blend` or, for a stretch of drawing, `with_blend`.
In the table, `S` is the source colour (premultiplied), `Sa` its alpha
and `D` the destination.

| `BlendMode` | Result | Use |
|---|---|---|
| `Alpha` (default) | `S + D * (1 - Sa)` | ordinary transparency |
| `PremultipliedAlpha` | the same equation, with the tint taken as already premultiplied | a tint with zero alpha and some colour adds light without darkening |
| `Additive` | `D + S` | fire, glows, explosions |
| `Multiply` | `D * S + D * (1 - Sa)` | shadows, tinted glass |
| `Screen` | `S + D * (1 - S)` | brightening without clipping as fast as adding |
| `Subtract` | `D - S` | darkening by subtraction |
| `Replace` | `S` | writing straight to the target, ignoring what is there |

```rust,no_run
use yowie::gfx::{BlendMode, Graphics, Sprite, Texture};
use yowie::{Color, vec2};

fn draw_muzzle_flash(gfx: &mut Graphics, glow: &Texture) {
    gfx.with_blend(BlendMode::Additive, |g| {
        let sprite = Sprite::at(vec2(100.0, 80.0))
            .centered()
            .tint(Color::rgba(1.0, 0.6, 0.2, 0.8));
        g.draw_sprite(glow, &sprite);
    });
}
```

A sprite's tint multiplies its texture, which covers team colours, fading
and a damage flash. For grading beyond a multiply, build a `ColorMatrix`
and draw through a `ColorMatrixShader`. The constructors are
`saturation`, `hue_rotate` (radians), `brightness`, `contrast`, `sepia`,
`invert`, `tint` and `from_rows` for a custom 4 by 5 matrix; `then` joins
two. Changing the matrix with `set` affects only draws queued afterwards.

```rust,no_run
use yowie::gfx::{ColorMatrix, ColorMatrixShader, Graphics, Sprite, Texture};
use yowie::{Color, GameResult, vec2};

fn make_frozen(gfx: &Graphics) -> GameResult<ColorMatrixShader> {
    let matrix = ColorMatrix::saturation(0.2).then(&ColorMatrix::tint(Color::rgb8(170, 210, 255)));
    Ok(ColorMatrixShader::new(gfx, &matrix)?)
}

fn draw_frozen_enemy(gfx: &mut Graphics, frozen: &ColorMatrixShader, enemy: &Texture) {
    gfx.with_shader(frozen.shader(), |g| {
        g.draw_sprite(enemy, &Sprite::at(vec2(60.0, 60.0)));
    });
}
```

`with_shader` and `set_shader` take any 2D fragment shader, including
your own; [shaders](shaders.md) explains how to write one.

## Transforms

To move, turn, scale or shear a group of draws together, use the
transform stack. `push_transform` saves the current transform,
`translate`, `rotate`, `scale` and `shear` change it, and
`pop_transform` restores the saved one. `with_transform` does the same
for a closure, and `set_transform` and `reset_transform` replace it
outright. The transform maps drawing coordinates before the camera, and
the stack is cleared at the start of each frame.

```rust,no_run
use yowie::gfx::Graphics;
use yowie::{Color, Rect, Vec2};

fn draw_turret(gfx: &mut Graphics, base: Vec2, aim: f32) {
    gfx.push_transform();
    gfx.translate(base);
    gfx.rotate(aim);
    gfx.fill_rect(Rect::new(-8.0, -8.0, 16.0, 16.0), Color::GRAY);
    gfx.fill_rect(Rect::new(0.0, -2.0, 20.0, 4.0), Color::WHITE);
    gfx.pop_transform();
}
```

The transform applies to sprites, shapes, text, paths and triangles. The
clip rectangle stays in view units. 2D lights and occluders are not
moved by it.

## Cameras

To draw a world larger than the view, draw it through a `Camera2D`. Its
`position` is the world point at the centre of the view, `zoom` scales
the world (2 shows half as much), and `rotation` turns the camera
clockwise, so the world appears turned the other way. Draw inside
`with_camera`, then draw the HUD after the closure, where coordinates are
view units again.

```rust,no_run
use yowie::gfx::{Camera2D, Sprite, Texture};
use yowie::{Color, Context, Rect, Vec2};

fn draw_level(ctx: &mut Context, camera: &Camera2D, player: &Texture, player_at: Vec2) {
    // The world point under the pointer, for aiming.
    let aim = camera.screen_to_world(ctx.input.mouse(), ctx.gfx.view_size());

    ctx.gfx.with_camera(camera, |g| {
        g.draw_sprite(player, &Sprite::at(player_at).centered());
        g.fill_circle(aim, 2.0, Color::RED);
    });

    // The HUD, in view units.
    ctx.gfx.fill_rect(Rect::new(4.0, 4.0, 60.0, 6.0), Color::GREEN);
}
```

`screen_to_world` and `world_to_screen` convert between view units and
world positions; pass `ctx.gfx.view_size()` as the view size.
`visible_rect` returns the world rectangle on screen, which is also what
sprite culling tests against. `Camera2D::clamp_to(bounds, view_size)`
moves the camera so the view stays inside a world rectangle, and centres
on an axis where the bounds are smaller than the view.

A camera does not combine with an outer one: `with_camera` replaces the
current camera for its closure and restores it afterwards, and
`set_camera(None)` returns to view units. The transform stack, by
contrast, applies inside the camera's world.

For split screens and minimaps, give a camera a viewport, a rectangle of
the view in view units. Drawing through it is clipped to that rectangle
and the camera's position sits at its centre.

```rust,no_run
use yowie::gfx::{Camera2D, Graphics};
use yowie::{Rect, Vec2};

fn draw_split(gfx: &mut Graphics, p1: Vec2, p2: Vec2, draw_world: impl Fn(&mut Graphics)) {
    let view = gfx.view_size();
    let left = Camera2D::new(p1).with_viewport(Rect::new(0.0, 0.0, view.x * 0.5, view.y));
    let right = Camera2D::new(p2).with_viewport(Rect::new(view.x * 0.5, 0.0, view.x * 0.5, view.y));
    gfx.with_camera(&left, |g| draw_world(g));
    gfx.with_camera(&right, |g| draw_world(g));
}
```

To keep drawing inside a rectangle without a camera, such as a scrolling
list in a panel, call `with_clip` (or `set_clip`). The rectangle is in
view units, the camera does not move it, and a nested `with_clip` clips
to the intersection of both rectangles.

### Following a target

`CameraRig` drives a `Camera2D` for a game that follows a player. It
moves the camera after a target with a dead zone and smoothing, keeps the
view inside bounds, and adds screen shake from trauma. Call `follow` and
`update` once per update, and draw through `camera()`.

```rust,no_run
use yowie::gfx::CameraRig;
use yowie::{Rect, Vec2, vec2};

fn make_rig(start: Vec2, view: Vec2, level: Rect) -> CameraRig {
    let mut rig = CameraRig::new(start, view);
    rig.set_zoom(2.0);
    rig.dead_zone = vec2(64.0, 40.0);
    rig.smoothing = 5.0;
    rig.bounds = Some(level);
    rig.snap_to(start);
    rig
}

fn update_rig(rig: &mut CameraRig, player: Vec2, hit: bool, dt: f32) {
    if hit {
        rig.add_trauma(0.5);
    }
    rig.follow(player, dt);
    rig.update(dt);
}
```

| Field | Default | Meaning |
|---|---|---|
| `view_size` | from `new` | The view's size in view units, used for the bounds. |
| `dead_zone` | `(0, 0)` | A box around the camera centre, in world units, inside which the target moves without moving the camera. |
| `smoothing` | 0 | How fast the camera closes on its goal, per second; 0 or less snaps. |
| `bounds` | `None` | A world rectangle the view stays inside. |
| `trauma_decay` | 1 | Trauma lost per second. |
| `max_offset` | `(16, 16)` | The largest shake offset in world units. |
| `max_angle` | 0.04 | The largest shake rotation in radians. |
| `shake_frequency` | 22 | Shake noise frequency in cycles per second. |

Trauma runs from 0 to 1, and the shake strength is its square, so small
hits barely move the view. `base()` is the camera without the shake; use
it to convert the pointer to world positions so aiming does not jitter.
The `tiles` example drives a rig with zoom, sway and shake.

## Tilemaps

To draw a grid of tiles, make a `Tilemap` from a `GridLayout`, a size in
cells and a `SpriteSheet`. Each cell holds a `Tile`: a sheet frame and a
`TileFlip`. One tilemap is one texture, so it draws as one draw call
however many tiles are visible, and only cells that can reach the
visible area are visited. For several layers, make one tilemap per layer
and draw them on different `set_layer` values.

```rust,no_run
use yowie::gfx::{
    Graphics, GridLayout, SpriteSheet, Tile, TileAnimation, TileFlip, Tilemap,
};
use yowie::{uvec2, vec2};

const GRASS: u32 = 0;
const WATER: u32 = 4;
const ARROW: u32 = 7;

fn build_ground(sheet: SpriteSheet) -> Tilemap {
    let layout = GridLayout::square(vec2(16.0, 16.0));
    let mut map = Tilemap::new(layout, uvec2(64, 48), sheet);
    map.fill(Tile::new(GRASS));
    map.set(10, 8, Tile::new(WATER));
    map.set(12, 8, Tile::new(ARROW).flipped(TileFlip::ROTATE_90));
    // Cells showing frame 4 cycle through frames 4, 5, 6 and 5.
    map.animate(WATER, TileAnimation::uniform([4, 5, 6, 5], 0.35));
    map
}

fn update_ground(map: &mut Tilemap, dt: f32) {
    map.advance(dt);
}

fn draw_ground(gfx: &mut Graphics, map: &Tilemap) {
    gfx.set_layer(0);
    map.draw(gfx);
}
```

`fill_with` sets every cell from a closure of its coordinates, `get`
reads a cell (an empty tile outside the map), `set_frame` takes an
`Option<u32>`, and `clear` empties the map. `cell_at` returns the cell
under a world position, `cell_center` the middle of a cell, and `bounds`
the world rectangle the map covers, which suits `CameraRig::bounds`.

A tile's picture is drawn at `tile_size` (the sheet's frame size by
default), anchored to the bottom middle of its cell, so a tree taller
than a cell rises above it. `offset` moves the whole map, `tint`
multiplies every tile, and `draw_shifted` draws with an extra offset for
parallax layers.

`GridLayout` places cells for every grid shape a map editor offers.
Cell coordinates convert to world positions and back on all of them:

| Constructor | Grid |
|---|---|
| `GridLayout::square(cell)` | rows and columns of rectangles |
| `GridLayout::isometric(cell)` | diamonds; `x` runs down and right, `y` down and left |
| `GridLayout::hex_pointy(cell, stagger)` | pointy-top hexagons in staggered rows |
| `GridLayout::hex_flat(cell, stagger)` | flat-top hexagons in staggered columns |
| `GridLayout::diamond_rows(cell, stagger)` | diamonds in staggered rows |
| `GridLayout::diamond_cols(cell, stagger)` | diamonds in staggered columns |

`Stagger::Odd` or `Stagger::Even` says which rows or columns are shifted
by half a cell. `cell_corners` returns a cell's outline (four or six
points) for drawing a cursor over it.

`TileFlip` follows the flags of map editors: `diagonal` transposes the
picture first, then `horizontal` and `vertical` mirror it, and
`rotate_120` turns it for hexagonal maps. The constants `NONE`,
`ROTATE_90`, `ROTATE_180` and `ROTATE_270` cover the turns.

## Autotiling

Autotiling picks tile frames from a terrain grid, so a map of plain
terrain ids draws with matching edges, corners and transitions. The game
keeps one integer per cell; a `yowie::gfx::autotile::Mapper` turns those
into frames. It needs no GPU, and its frames are `Option<u32>`, which
`Tilemap::set` takes through `.into()`.

Pick a rule set to match the tileset:

| `Rules` | Matches | Tiles |
|---|---|---|
| `edge16(terrain, frames)` | the four edge neighbours: walls, pipes, fences | 16, indexed by a mask of north 1, east 2, south 4, west 8 |
| `edge64(terrain, frames)` | the six neighbours of a hexagon | 64 |
| `blob47(terrain, frames)` | all eight neighbours, reduced to 47 cases: the standard blob terrain set | 47 in canonical order |
| `corner16(terrain, frames)` | the four cells around each corner (a dual grid) | 16; the output grid is one cell wider and taller, drawn shifted up and left by half a tile |
| `wang(kind, tiles)` | terrain colours on tile edges, corners or both | any Wang set, with transitions between several terrains |

```rust,no_run
use yowie::gfx::Tilemap;
use yowie::gfx::autotile::{Mapper, Rules};

const WALL: i32 = 2;

fn make_wall_mapper() -> Mapper {
    // Frame i of the wall sheet is drawn for neighbour mask i.
    Mapper::new(Rules::edge16(WALL, std::array::from_fn(|i| Some(i as u32))))
}

fn paint_walls(walls: &mut Tilemap, mapper: &Mapper, terrain: &[i32], w: i32, h: i32) {
    let at = |x: i32, y: i32| terrain[(y * w + x) as usize];
    mapper.apply(w, h, at, |x, y, frame| walls.set(x, y, frame.into()));
}

fn repaint_cell(walls: &mut Tilemap, mapper: &Mapper, terrain: &[i32], w: i32, h: i32, x: i32, y: i32) {
    let at = |x: i32, y: i32| terrain[(y * w + x) as usize];
    mapper.cell(x, y, w, h, at, |x, y, frame| walls.set(x, y, frame.into()));
}
```

`apply` computes every cell, `cell` recomputes only what one changed
cell can affect, and `region` does the same for a changed rectangle.
`apply_grid` takes a terrain slice and returns the frame grid.

By default the map border continues each cell's own terrain, so edges
connect off the map; `Mapper::outside(terrain)` sets a fixed terrain
instead. `Rules::connect` makes other terrains count as connected, so
grass joins up against road cells. `Rules::variant(mask, frame, weight)`
adds an alternative frame for one case, chosen by a hash of the cell
position and `Mapper::seed`, so reapplying gives the same picture.

`expand_blob(&template, tile)` composes the 47 blob tiles from a
six-tile template that an artist can draw by hand. A hexagonal or
isometric map needs the matching neighbour rules: pass
`grid_layout.autotile_layout()` to `Mapper::layout`. Hexagonal layouts
have six neighbours and no diagonals, so they take `edge64` or Wang
rules. `wang_from_tiled` converts a Wang set from the Tiled editor. Run
`cargo run --example autotile` to paint with blob, edge and Wang rules.

## Maps from the Tiled editor

`yowie::tiled` reads TMX, TSX, TMJ and TSJ files into plain data:
tile layers with flip flags, infinite maps, object layers, image and
group layers, tilesets with animations and collision shapes, Wang sets,
object templates and typed custom properties. To draw one, build a
`TiledLevel`, which uploads the tilesets and turns each tile layer into
tilemaps (one per tileset the layer uses, so one draw call each).

```rust,no_run
use yowie::gfx::{Camera2D, TiledLevel};
use yowie::{Color, Context, GameResult};

fn load_level(ctx: &mut Context) -> GameResult<(TiledLevel, Camera2D)> {
    let level = TiledLevel::load(&ctx.gfx, ctx.assets.vfs(), "maps/level.tmx")?;
    let camera = Camera2D::new(level.bounds().center()).with_zoom(2.0);
    Ok((level, camera))
}

fn draw_level(ctx: &mut Context, level: &TiledLevel, camera: &Camera2D) {
    ctx.gfx.with_camera(camera, |g| {
        level.draw(g);
        g.set_layer(1);
        level.draw_objects(g, 1.5, Color::YELLOW);
    });
}
```

Call `advance(dt)` each update to play animated tiles. `draw` applies
each layer's visibility, offset, opacity and parallax; call it inside the
camera scope, because parallax reads the current camera.
`draw_objects` outlines every visible object, which helps while laying
out collision shapes and triggers. Orthogonal, isometric, staggered and
hexagonal maps all work; `tile_center` and `tile_at` convert between
tiles and world positions on any of them, before layer offsets.

To read gameplay data, use `level.map()`, the parsed `yowie::tiled::Map`.
`find_layer`, `walk_layers` and `object_layers` reach the layers;
objects carry their `name`, `class`, rectangle, `shape` and
`properties`, whose typed getters (`string`, `int`, `float`, `bool`,
`color`, `file`, `class`) return `None` for a missing property or one of
another type.

For hot reload, load the map with `ctx.load_map(path)`. The returned
handle's `generation()` increases each time the map or one of its
tilesets changes on disk; when it does, rebuild the level with
`TiledLevel::build(&ctx.gfx, ctx.assets.vfs(), map, dir)`, where `dir` is
the map file's directory.

## Text

### Fonts

The engine bundles the Go fonts, so text works with no files.
`ctx.gfx.default_font()` returns Go Regular (with its bold and italic
faces, which markup uses) and `default_mono_font()` returns Go Mono. To
use your own TrueType or OpenType font, call `ctx.load_font(path)` for an
asset, or `Font::from_bytes(&mut ctx.gfx, bytes)` and
`Font::from_file`. Colour fonts (COLR and bitmap emoji fonts) draw in
colour.

No system fonts load unless you ask, so start-up is quick and text looks
the same on every machine. `load_system_fonts()` scans the system's font
directories (call it once, at start-up), `system_font(family)` returns
an installed family, and `system_emoji_font()` loads the platform's
colour emoji font when it is installed.

To cover characters a font lacks, chain fallbacks with
`font.with_fallback(&[arabic, cjk])`. Each character uses the first font
in the chain that has a glyph for it.

### Drawing and styling

To draw text, call `draw_text` with a font, the string, the top-left
corner of its box and a `TextStyle`.

```rust,no_run
use yowie::gfx::{Font, Graphics, Language, TextAlign, TextStyle};
use yowie::{Color, vec2};

fn draw_story(gfx: &mut Graphics, font: &Font, story: &str) {
    let title = TextStyle::new(24.0).color(Color::rgb8(255, 220, 120));
    gfx.draw_text(font, "Chapter One", vec2(20.0, 20.0), &title);

    let body = TextStyle::new(15.0)
        .color(Color::rgb8(232, 236, 250))
        .width(280.0)
        .align(TextAlign::Justified)
        .hyphenate(Language::English);
    gfx.draw_text(font, story, vec2(20.0, 56.0), &body);
}
```

| Field | Builder | Default | Meaning |
|---|---|---|---|
| `size` | `TextStyle::new(size)` | 16 | Font size in view units. |
| `line_height` | `line_height` | 1.25 | Line height as a multiple of the size. |
| `color` | `color` | white | Text colour, sRGB with straight alpha. |
| `width` | `width` | `None` | Wraps lines to this width in view units; `None` breaks only at newlines. A word too long for a line breaks inside the word. |
| `align` | `align` | `Left` | `Left`, `Center`, `Right` or `Justified` within the width (or the widest line). |
| `hyphenate` | `hyphenate` | `None` | Breaks long words at hyphenation points when wrapping. |
| `orientation` | `orientation` | `Horizontal` | `Upright` stacks upright glyphs in columns read downwards, new columns to the left; `Sideways` turns each line a quarter turn clockwise. |
| `sdf` | `sdf` | `false` | Draws glyphs from signed distance fields, sharp at any scale and under any transform, with a little less detail on small text. |
| `underline`, `strikethrough` | `underline()`, `strikethrough()` | `false` | Lines under or through the whole text. |

Hyphenation has patterns for `Danish`, `German`, `English`, `Spanish`,
`Finnish`, `French`, `Italian`, `Norwegian`, `Dutch`, `Polish`,
`Portuguese`, `Russian` and `Swedish`; `Language::from_tag("pt-BR")`
picks one from a language tag. Text is shaped with kerning and
ligatures, Arabic joins, and right-to-left and mixed-direction lines lay
out in their reading order without any setting.

Glyphs rasterise at the size they cover in output pixels, which includes
the display's density, a fixed view's scale, the camera's zoom and the
transform's scale, so text is sharp in all of those cases. Without a
camera or transform, text also snaps to whole pixels. Each new size
rasterises new glyphs, so for text that turns, shears or changes scale
every frame, set `sdf(true)`: distance-field glyphs are made once and
stay sharp under any transform.

### Rich text

To mix styles in one string, call `draw_markup` with bracket markup:

| Markup | Effect |
|---|---|
| `[b]..[/b]`, `[i]..[/i]` | bold, italic |
| `[u]..[/u]`, `[s]..[/s]` | underline, strikethrough |
| `[#ff8800]..[/#]`, `[color=#ff8800]..[/color]` | colour |
| `[size=24]..[/size]` | font size in view units |
| `[font=Family]..[/font]` | a loaded font family |
| `[link=name]..[/link]` | a link, drawn underlined |
| `[[` | a literal `[` |

Tags nest, and any closing tag ends the innermost open one. An unknown
or malformed tag stays in the text as written.

```rust,no_run
use yowie::gfx::{Font, Graphics, TextStyle};
use yowie::vec2;

fn draw_quest(gfx: &mut Graphics, font: &Font) {
    let style = TextStyle::new(16.0).width(300.0);
    gfx.draw_markup(
        font,
        "Bring [b]three[/b] [#60d0ff]moon pearls[/#] to the [link=keeper]keeper[/link].",
        vec2(12.0, 12.0),
        &style,
    );
}
```

To build styled text in code instead, push spans onto a `RichText`, each
with a `SpanStyle` (`color`, `bold`, `italic`, `underline`,
`strikethrough`, `size`, `font`, `link`), and lay it out with
`layout_rich`.

### Layouts, measuring and hit testing

`layout_text` (and `layout_markup`) returns a `TextLayout` you can draw
many times with `draw_layout`, measure and query. Layouts are cached by
content, so calling `draw_text` with the same text every frame does not
lay it out again; holding the layout saves even the lookup. `layout_rich`
is not cached, so hold its result.

`measure_text` returns the size of the layout box: the widest line (or
the wrap width, for aligned text) and the height of all lines.
`TextLayout` also has `line_count`, `line_range`, `hit_test(point)`,
which returns the byte offset nearest a point (for placing a caret where
the player clicks), `caret(offset)`, which returns the caret's position
and height, and `link_at(point)`, which returns the link name under a
point. Positions are view units from the layout's top-left corner.

```rust,no_run
use yowie::gfx::{Font, Graphics, TextStyle};
use yowie::Vec2;

/// Draws the quest text and returns the link the player clicked, if any.
fn quest_link(gfx: &mut Graphics, font: &Font, at: Vec2, click: Option<Vec2>) -> Option<String> {
    let style = TextStyle::new(16.0).width(300.0);
    let layout = gfx.layout_markup(font, "Talk to the [link=keeper]keeper[/link].", &style);
    gfx.draw_layout(&layout, at);
    let link = click.and_then(|p| layout.link_at(p - at));
    link.map(str::to_owned)
}
```

Text draws through the sprite batch from glyph atlases, so it sorts,
clips and transforms like any other 2D draw. The `text` example shows
every feature in this section; `-- --font file.ttf` sets its body text
in another font.

## Vector paths

To draw anti-aliased shapes with curves, holes, dashes or gradients,
build a `Path` and call `fill_path` or `stroke_path`. Paths are
tessellated with lyon. Coordinates are drawing units, so the camera and
transform stack apply.

```rust,no_run
use yowie::gfx::{Cap, Dash, Fill, Gradient, Graphics, Join, Paint, Path, Stroke};
use yowie::{Color, Rect, vec2};

fn draw_badge(gfx: &mut Graphics) {
    let mut b = Path::builder();
    b.move_to(vec2(40.0, 10.0))
        .line_to(vec2(70.0, 70.0))
        .quad_to(vec2(40.0, 90.0), vec2(10.0, 70.0))
        .close();
    let shield = b.build();

    let shine = Gradient::two(Color::rgb8(255, 220, 120), Color::rgb8(200, 90, 30));
    gfx.fill_path(&shield, &Fill::new(Paint::linear(vec2(40.0, 10.0), vec2(40.0, 90.0), shine)));
    gfx.stroke_path(&shield, &Stroke::new(Color::WHITE, 3.0).join(Join::Round));

    let frame = Path::rounded_rect(Rect::new(0.0, 0.0, 80.0, 100.0), 12.0);
    let dashed = Stroke::new(Color::GRAY, 2.0).cap(Cap::Round).dash(Dash::new(6.0, 4.0));
    gfx.stroke_path(&frame, &dashed);
}
```

`PathBuilder` has `move_to`, `line_to`, `quad_to`, `cubic_to`, `arc`
(about a centre, by a sweep in radians, positive clockwise), `arc_to`
(SVG-style, to an end point), `close`, and `add_rect`,
`add_rounded_rect`, `add_circle` and `add_ellipse` for closed subpaths.
`Path::rect`, `rounded_rect`, `circle` and `ellipse` build one shape
directly, and `Path::bounds` encloses every point of a path.

A `Fill` has a paint, a `FillRule` (`NonZero` by default, or `EvenOdd`,
under which nested contours cut holes) and `antialias`. A `Stroke` has a
paint, a width, a `Cap` (`Butt`, `Round`, `Square`), a `Join` (`Miter`,
`Round`, `Bevel`), a `miter_limit` (default 4, as a multiple of half the
width), an optional `Dash` and `antialias`. Open subpaths are closed for
filling and capped for stroking.

A `Paint` is a solid colour (any `Color` converts into one), a linear
gradient (`Paint::linear(start, end, gradient)`), a radial gradient
(`Paint::radial(center, radius, gradient)`) or a texture
(`Paint::Texture(TexturePaint::new(&texture, origin))`, whose `scale` is
drawing units per texel; give the texture `Wrap::Repeat` to tile it). A
`Gradient` holds `GradientStop`s from 0 to 1, mixed in linear light;
`spread` sets what happens past the ends (`Wrap::Clamp`, `Repeat` or
`Mirror`).

Anti-aliasing adds a fringe about one pixel wide around the shape. Curves
are flattened to within a fraction of a pixel at the current zoom and
transform. Each `fill_path` and `stroke_path` call tessellates the path
again, so for a shape drawn every frame that never changes, a sprite
from a render texture costs less. The `vector` example shows every fill
rule, cap, join, dash, paint and blend mode.

## Particles

`yowie::particle` runs particle effects. An `Emitter` is plain data that
describes how particles are born (`shape`, `rate`, `burst`, `bursts`),
how they move (`speed`, `direction`, `spread`, `gravity`, `drag`,
radial and tangential acceleration, `attractors`) and how they look over
their life (`size_over_life`, `color_over_life`, `alpha_over_life`,
`frames`, `blend`). Units are world units, seconds and radians, with 0
pointing right and angles clockwise, so straight up is `-PI / 2`.

Start from a preset in `yowie::particle::presets` (`fire`, `smoke`,
`sparks`, `rain`, `confetti`) and change the fields you need, or read
an emitter from JSON with `Emitter::from_json`. Every field has a
default, so a JSON file names only what it changes, and
`Emitter::to_json` writes one back.

To run an effect, make a `Particles` with a `Backend`, call `update`
once per update and `draw` once per frame. `Backend::Cpu` runs a few
thousand particles with exact counts; `Backend::Gpu` runs hundreds of
thousands with compute shaders. A device that cannot run the GPU path
falls back to the CPU, and `backend()` says which is running.
`switch_to` moves a running effect between the two.

```rust,no_run
use yowie::gfx::{Texture, TextureOptions};
use yowie::particle::{Backend, Emitter, Particles, presets};
use yowie::{Context, Game, GameResult, vec2};

struct Camp {
    fire: Particles,
}

impl Game for Camp {
    fn init(ctx: &mut Context) -> GameResult<Self> {
        let soft = Texture::from_image(&ctx.gfx, &presets::soft_circle(64), TextureOptions::smooth())?;
        let emitter = Emitter {
            rate: 120.0,
            prewarm: 1.0,
            ..presets::fire()
        };
        let mut fire = Particles::at(&ctx.gfx, emitter, vec2(480.0, 430.0), Backend::Cpu);
        fire.set_texture(soft);
        Ok(Camp { fire })
    }

    fn update(&mut self, ctx: &mut Context) -> GameResult {
        self.fire.update(ctx.time.delta());
        Ok(())
    }

    fn draw(&mut self, ctx: &mut Context) -> GameResult {
        self.fire.draw(&mut ctx.gfx);
        Ok(())
    }
}
```

Each effect draws as one draw call, on the emitter's `layer` with its
`blend` mode (both restored afterwards), through the current camera.
Particles outside the visible area are skipped. Without a texture,
particles draw as solid squares; `set_sheet` draws them from the frames
of a sprite sheet.

`set_position` moves the effect. With `world_space` set, live particles
stay where they were born (smoke, sparks); without it they move with the
effect (a flame on a torch). `burst(n)` adds particles at once, `stop`
ends emission while live particles die out, `finished()` says when a
one-shot effect can be dropped, and `start` restarts it. On the GPU,
`alive()` lags a few frames behind. `yowie::particle::System` and
`GpuSystem` are the two backends on their own, with the same methods.

`Particles::draw_3d` draws a GPU effect in a 3D scene on a plane set
with `GpuSystem::set_plane`; see [3D graphics](graphics-3d.md). The
`gallery` example includes a particle editor that saves its emitter as
JSON.

## 2D lights and shadows

To light sprites, describe the lighting once per frame during `draw`,
then draw the lit sprites with `draw_lit`:

- `set_lights_2d(ambient, &lights)` sets the ambient colour (sRGB) and
  up to `MAX_LIGHTS_2D` (16) lights. Lights past the limit are dropped,
  so pass the nearest first.
- `add_occluder(&points)`, `add_occluder_rect(rect)` and
  `add_occluder_sprite(&texture, &sprite)` add shapes that block lights
  that cast shadows.
- `draw_lit(&texture, normal, &sprite)` draws a sprite shaded by the
  lights, with an optional tangent-space normal map drawn over the same
  rectangle.

```rust,no_run
use yowie::gfx::{Graphics, Light2D, Sprite, Texture};
use yowie::{Color, Rect, Vec2, vec2};

fn draw_cellar(gfx: &mut Graphics, floor: &Texture, floor_normals: &Texture, lamp: Vec2) {
    gfx.set_lights_2d(
        Color::rgb8(24, 26, 44),
        &[Light2D::point(lamp, 250.0, Color::rgb8(255, 196, 120))
            .height(46.0)
            .intensity(1.5)
            .shadows(true)
            .softness(10.0)],
    );
    gfx.add_occluder_rect(Rect::new(200.0, 140.0, 40.0, 40.0));
    gfx.draw_lit(floor, Some(floor_normals), &Sprite::at(vec2(0.0, 0.0)));
}
```

`Light2D::point(position, radius, color)` lights every direction;
`Light2D::spot(position, radius, color, cone)` limits it to a `Spot2D`
cone (`direction` in radians with 0 pointing right, full opening
`angle`, and `softness` of the edge from 0 to 1).

| Field | Builder | Default | Meaning |
|---|---|---|---|
| `height` | `height` | 40 | Distance above the sprite plane in view units. Normal maps shade by the angle to this point. |
| `radius` | constructor | | Distance at which the light fades to nothing. |
| `color` | constructor | | Light colour, sRGB. |
| `intensity` | `intensity` | 1 | Multiplies the colour; above 1 over-brightens. |
| `falloff` | `falloff` | 2 | Exponent of the fade: 1 is linear, 2 softer near the edge. |
| `casts_shadows` | `shadows` | `false` | Whether occluders block the light. |
| `softness` | `softness` | 8 | Width of a shadow's soft edge in view units. |

Make normal maps with `TextureOptions { data: true, .. }` so their
values are not colour converted. Without a normal map, `draw_lit` lights
the sprite as a flat surface facing the viewer. Lights, occluders and
lit sprites all use drawing coordinates (world units under a camera).

Some limits to plan around:

- The lighting resets every frame. Every lit sprite in a frame sees the
  same lights, whatever the order of the calls.
- Lit sprites blend as ordinary transparency whatever the blend mode, and
  ignore the current shader.
- Normals are not turned with a rotated sprite.
- The transform stack moves a lit sprite but not lights or occluders.
- Shadows are built on the CPU: each shadowed light gets a map of 512
  directions around it. The cost grows with the occluder edges near each
  shadowed light times the number of shadowed lights, so add the walls
  near the action rather than a whole level. A frame whose shadowed
  lights and occluders match the previous frame's reuses its maps.

Lit and unlit sprites mix in one frame and sort together by layer and
sort key. The `sprites` example lights a brick floor with a normal map
and three crates that cast shadows.

## Render textures

To draw into an offscreen image and then draw that image, make a
`RenderTexture` and draw into it inside `draw_to`. Use it for minimaps,
portraits, mirrors, or a whole low-resolution scene scaled up.

```rust,no_run
use yowie::gfx::{Filter, RenderTexture, RenderTextureOptions};
use yowie::{Color, Context, Game, GameResult, Rect, vec2};

struct Retro {
    canvas: RenderTexture,
}

impl Game for Retro {
    fn init(ctx: &mut Context) -> GameResult<Self> {
        let options = RenderTextureOptions {
            filter: Filter::Nearest,
            view_size: None,
        };
        let canvas = RenderTexture::new(&ctx.gfx, 160, 90, options)?;
        Ok(Retro { canvas })
    }

    fn draw(&mut self, ctx: &mut Context) -> GameResult {
        ctx.gfx.draw_to(&self.canvas, Some(Color::rgb8(20, 24, 40)), |g| {
            // Inside, the view is 160 by 90 units.
            g.fill_circle(vec2(80.0, 45.0), 20.0, Color::ORANGE);
        });
        let view = ctx.gfx.view_size();
        ctx.gfx.draw_texture_rect(
            self.canvas.texture(),
            Rect::new(0.0, 0.0, view.x, view.y),
            Color::WHITE,
        );
        Ok(())
    }
}
```

Inside `draw_to`, the view is the render texture's size in pixels, or
`RenderTextureOptions::view_size` in view units when set. The layer, sort
key, blend mode, shader, camera and clip start at their defaults inside
the closure and return afterwards. With `Some(color)` the target is
cleared first; with `None` the drawing goes over what it held from
earlier frames.

Render textures render before the screen, in the order their `draw_to`
scopes finish, so the result can be drawn in the same frame. A target
drawn inside another target's scope is ready before the outer one. Its
contents are premultiplied, like any texture. To copy the pixels back,
call `RenderTexture::read` after the frame has rendered.

## Post-processing a 2D frame

To run bloom, a colour grade, a vignette and the other screen effects
over a 2D frame, call `ctx.gfx.set_frame_post(PostSettings { .. })`
every frame the effects should apply. The layers below `layers::UI`
draw into an HDR image, the effects run over it, and the interface
layers draw on top untouched. A 2D frame is not exposed or tone mapped,
so with no effect enabled it keeps the colours it drew. Inside `draw_to`
the call applies to that render texture. The settings and what each one
does are in [3D graphics](graphics-3d.md); `cargo run --example sprites --
--post` shows a light grade.

## Screenshots

To save a frame without a window, run the game with `--shot
<file.png>`; add `--seconds <n>` to render n seconds of game time first,
and set `YOWIE_FIXED_CLOCK=1` for the same picture on every machine.

To capture the window from game code, call `ctx.gfx.request_capture()`
during `draw`, then collect the image with `ctx.gfx.take_capture()` in a
later callback. It returns `Some(Ok(image))`, an `RgbaImage` in sRGB.

```rust,no_run
use yowie::input::Key;
use yowie::{Context, GameResult};

fn screenshot_on_f12(ctx: &mut Context) -> GameResult {
    if let Some(result) = ctx.gfx.take_capture() {
        result?.save("screenshot.png")?;
    }
    if ctx.input.key_pressed(Key::F12) {
        ctx.gfx.request_capture();
    }
    Ok(())
}
```

Call that from `draw`: the request is made during one frame's drawing and
the image arrives after that frame renders. A headless renderer can also
read its last frame with `read_output`. The debug console's
`screenshot` command writes a file the same way; see [the
console](console.md).

## Draw calls and batching

Every 2D draw (sprites, shapes, nine-slices, glyphs, paths, tilemaps and
particles) appends triangles to one vertex stream per output. At the end
of the frame the stream is sorted by layer, then sort key, then call
order, and neighbouring runs that share a texture, shader, blend mode and
view become one draw call. The view covers the camera, its viewport and
the clip rectangle.

To see what a frame cost, call `ctx.gfx.stats()`. `FrameStats` reports
`draw_calls`, `vertices` and `indices` for 2D drawing, and `outputs` (the
screen plus each render texture drawn to). F3 toggles the engine's
frame-timing overlay.

These change the state and so end a run:

- a different texture. Shapes share one white texture; text draws from
  the glyph atlases.
- a different shader, or the same shader with different uniform values.
- a different blend mode.
- a different camera, viewport or clip rectangle.
- a lit sprite's normal map or texture (lit sprites batch with lit
  sprites that share both).

To keep the count low:

- Put sprites that are drawn together into one sheet or atlas.
- Keep text on a layer of its own. Sprite, label, sprite, label on one
  layer alternates textures and costs a draw call each.
- Give draws that should batch the same sort key. Sorting by a different
  key per sprite (the foot position) interleaves textures again, so draw
  depth-sorted actors from one sheet.
- Draw a tile layer as a `Tilemap` and an effect as one `Particles`; each
  is one draw call.

Work that records its own GPU commands, such as a compute pass and an
instanced draw, implements `yowie::gfx::GpuDraw` and is queued with
`Graphics::draw_gpu`. It sorts by layer and sort key like a sprite. The
GPU particle backend is built on it; `cargo doc --open` documents the
trait.
