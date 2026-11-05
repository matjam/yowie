//! Fragment shaders written by the game: a wave over a checkerboard, a
//! dissolve with a burning edge, a light that follows world position, and
//! the error a broken shader reports. Below them, every blend mode over the
//! same backdrop. R reloads the wave shader from its source; Escape quits.

use yowie::gfx::{BlendMode, Font, Shader, Sprite, TextStyle, Texture, TextureOptions, Wrap};
use yowie::input::Key;
use yowie::{Color, Config, Context, Game, GameResult, Rect, Vec2, vec2};

/// Bends the texture coordinate along a sine wave. `params.x` is the
/// amplitude and `params.y` the frequency.
const WAVE: &str = "
struct Params { v: vec4<f32> };
@group(2) @binding(0) var<uniform> params: Params;

fn fragment(input: SpriteInput) -> vec4<f32> {
    var uv = input.uv;
    uv.x += sin(uv.y * params.v.y + input.time * 3.0) * params.v.x;
    return sample_texture(uv) * input.color;
}
";

/// Burns the sprite away through value noise. `params.x` is the progress
/// from 0 to 1 and `params.y` the width of the glowing edge.
const DISSOLVE: &str = "
struct Params { v: vec4<f32> };
@group(2) @binding(0) var<uniform> params: Params;

fn hash(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2<f32>(127.1, 311.7))) * 43758.5453);
}

fn noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = mix(hash(i), hash(i + vec2<f32>(1.0, 0.0)), u.x);
    let b = mix(hash(i + vec2<f32>(0.0, 1.0)), hash(i + vec2<f32>(1.0, 1.0)), u.x);
    return mix(a, b, u.y);
}

fn fragment(input: SpriteInput) -> vec4<f32> {
    let c = sample_texture(input.uv) * input.color;
    let n = noise(input.uv * 8.0) * 0.7 + noise(input.uv * 23.0) * 0.3;
    let edge = params.v.y;
    let cut = params.v.x * (1.0 + edge);
    if n < cut - edge {
        return vec4<f32>(0.0);
    }
    let glow = 1.0 - clamp((n - (cut - edge)) / edge, 0.0, 1.0);
    let fire = vec3<f32>(1.0, 0.45, 0.1) * glow * 2.0 * c.a;
    return vec4<f32>(c.rgb + fire, c.a);
}
";

/// Lights the sprite from a point in drawing coordinates. `params.xy` is
/// the light's position and `params.z` its radius.
const SPOTLIGHT: &str = "
struct Params { v: vec4<f32> };
@group(2) @binding(0) var<uniform> params: Params;

fn fragment(input: SpriteInput) -> vec4<f32> {
    let d = distance(input.world, params.v.xy) / params.v.z;
    let light = clamp(1.0 - d * d, 0.0, 1.0) * 1.6 + 0.08;
    let c = sample_texture(input.uv) * input.color;
    return vec4<f32>(c.rgb * light, c.a);
}
";

/// A typo on line 3: `colour` is not a field of `SpriteInput`.
const BROKEN: &str = "
fn fragment(input: SpriteInput) -> vec4<f32> {
    return sample_texture(input.uv) * input.colour;
}
";

struct Shaders {
    font: Font,
    checker: Texture,
    wave: Shader,
    dissolve: Shader,
    spotlight: Shader,
    error: String,
}

fn checker_pixels(size: u32, cell: u32) -> Vec<u8> {
    let mut px = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let on = (x / cell + y / cell).is_multiple_of(2);
            let c = if on {
                [235, 200, 120, 255]
            } else {
                [60, 120, 170, 255]
            };
            px.extend_from_slice(&c);
        }
    }
    px
}

impl Game for Shaders {
    fn init(ctx: &mut Context) -> GameResult<Self> {
        let options = TextureOptions {
            wrap: Wrap::Repeat,
            ..TextureOptions::smooth()
        };
        let checker = Texture::from_pixels(&ctx.gfx, 128, 128, &checker_pixels(128, 16), options)?;
        let error = match ctx.gfx.compile_shader(BROKEN) {
            Ok(_) => "the broken shader compiled".to_owned(),
            Err(e) => e.to_string(),
        };
        Ok(Shaders {
            font: ctx.gfx.default_font(),
            checker,
            wave: ctx
                .gfx
                .compile_shader_with_uniforms(WAVE, &[0.03f32, 18.0, 0.0, 0.0])?,
            dissolve: ctx
                .gfx
                .compile_shader_with_uniforms(DISSOLVE, &[0.0f32, 0.12, 0.0, 0.0])?,
            spotlight: ctx
                .gfx
                .compile_shader_with_uniforms(SPOTLIGHT, &[0.0f32, 0.0, 120.0, 0.0])?,
            error,
        })
    }

    fn update(&mut self, ctx: &mut Context) -> GameResult {
        if ctx.input.key_pressed(Key::Escape) {
            ctx.quit();
        }
        if ctx.input.key_pressed(Key::R) {
            self.wave.reload_source(&ctx.gfx, WAVE)?;
        }
        Ok(())
    }

    fn draw(&mut self, ctx: &mut Context) -> GameResult {
        let t = ctx.time.elapsed() as f32;
        let g = &mut ctx.gfx;
        let label = TextStyle::new(14.0).color(Color::rgb8(225, 225, 235));
        let size = Vec2::splat(200.0);
        let tiles = [vec2(30.0, 40.0), vec2(260.0, 40.0), vec2(490.0, 40.0)];

        let progress = (t * 0.4).fract();
        self.dissolve.set_uniforms(&[progress, 0.12, 0.0, 0.0])?;
        let light = tiles[2] + size * 0.5 + Vec2::from_angle(t * 1.5) * 60.0;
        self.spotlight
            .set_uniforms(&[light.x, light.y, 120.0, 0.0])?;

        let shaders = [
            (&self.wave, "wave"),
            (&self.dissolve, "dissolve"),
            (&self.spotlight, "world light"),
        ];
        for ((shader, name), at) in shaders.into_iter().zip(tiles) {
            g.draw_text(&self.font, name, at - vec2(0.0, 22.0), &label);
            g.with_shader(shader, |g| {
                g.draw_sprite(&self.checker, &Sprite::at(at).size(size))
            });
        }
        let err_style = TextStyle::new(13.0)
            .color(Color::rgb8(255, 140, 120))
            .width(220.0);
        g.draw_text(&self.font, "compile error", vec2(720.0, 18.0), &label);
        g.draw_text(&self.font, &self.error, vec2(720.0, 40.0), &err_style);

        // Blend modes: a translucent orange disc over a striped backdrop.
        let top = 300.0;
        for (i, mode) in BlendMode::ALL.into_iter().enumerate() {
            let x = 30.0 + i as f32 * 132.0;
            g.fill_rect(Rect::new(x, top, 120.0, 60.0), Color::rgb8(40, 40, 50));
            g.fill_rect(
                Rect::new(x, top + 60.0, 120.0, 60.0),
                Color::rgb8(200, 200, 210),
            );
            let tint = match mode {
                BlendMode::PremultipliedAlpha => Color::rgba(1.0, 0.5, 0.1, 0.0),
                _ => Color::rgba(1.0, 0.5, 0.1, 0.7),
            };
            g.with_blend(mode, |g| {
                g.fill_circle(vec2(x + 60.0, top + 60.0), 42.0, tint)
            });
            g.draw_text(
                &self.font,
                &format!("{mode:?}"),
                vec2(x, top + 130.0),
                &label,
            );
        }
        Ok(())
    }
}

fn main() -> Result<(), yowie::Error> {
    let config = Config {
        clear_color: Color::rgb8(18, 20, 28),
        ..Config::new("Yowie shaders").with_size(960, 540)
    };
    yowie::run::<Shaders>(config)
}
