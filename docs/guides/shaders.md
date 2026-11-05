---
title: Shaders
group: Graphics
order: 3
summary: game-written WGSL for 2D sprites and 3D materials, the engine preludes, uniforms and their layout, compile errors, hot reload, and custom GPU draws
---

# Shaders

Yowie runs two kinds of game-written shader, both in WGSL:

| Kind | Type | Compile with | You write | Uniform block |
|---|---|---|---|---|
| 2D fragment shader | `yowie::gfx::Shader` | `Graphics::compile_shader`, `compile_shader_with_uniforms`, or `Context::load_shader` | `fn fragment(input: SpriteInput) -> vec4<f32>` | group 2, binding 0 |
| 3D surface shader | `yowie::gfx::scene::SurfaceShader` | `Graphics::compile_surface_shader`, `compile_surface_shader_with_uniforms` | `fn material(s: Surface) -> Surface` | group 3, binding 0 |

The engine compiles WGSL at run time through wgpu, which uses the naga
compiler, so there is no build step, no offline compiler and no shader
files to ship beyond the WGSL source. Compiling happens when you call a
compile function; do it while loading, not every frame. The engine builds
a pipeline for each blend mode, pass and target format the first time a
draw needs one, so the first frame that uses a shader in a new way takes
longer than the frames after it.

For anything these two hooks do not cover, a renderer extension can record
its own GPU commands; see [Custom GPU draws](#custom-gpu-draws).

Run `cargo run --example shaders` to see three 2D shaders, the error a
broken shader reports, and a 3D surface shader on a sphere, a torus and a
floor. R reloads two of them from their sources. The `materials` example
and [3D graphics](graphics-3d.md) cover the materials a surface shader
plugs into.

## How a game shader is put together

You write one function and any helpers, structs and constants it needs.
You do not write `@vertex` or `@fragment` entry points. The engine puts its
own source (the prelude) in front of yours, so the two form one WGSL
module: the prelude declares the bindings and the entry points, and its
entry point calls your function.

Because the module is shared, your source sees every name the prelude
declares and must not declare those names again. The 2D prelude's names
start with `yowie_` or `Yowie`, plus `SpriteInput`, `sample_texture` and
your `fragment`. The 3D prelude is the engine's whole mesh program, with
many function and constant names (`PI`, `sqr`, `Material`, `Light` and
others), so give your own helpers a prefix, as the `shaders` example does
with `lava_hash` and `lava_noise`.

Colours in a game shader are linear light. A 2D shader returns
premultiplied colour; a surface shader works on straight colour, and the
engine handles alpha.

## 2D fragment shaders

### The 2D prelude

A 2D shader runs for every pixel of every sprite, shape and glyph drawn
through it. The prelude gives your `fragment` function this input:

```wgsl
// What the fragment function receives for each pixel.
struct SpriteInput {
    // Texture coordinate, 0 to 1 across the texture.
    uv: vec2<f32>,
    // The tint in linear light, premultiplied by its alpha.
    color: vec4<f32>,
    // The position in drawing coordinates: world units under a camera,
    // view units otherwise.
    world: vec2<f32>,
    // Seconds of game time.
    time: f32,
    // The size of the current view in view units.
    view_size: vec2<f32>,
    // Output pixels per view unit.
    pixel_scale: f32,
};

var<private> yowie_mask: bool;

// Samples the drawn texture at uv, premultiplied and linear. Glyph masks
// come back as white with the coverage in every channel.
fn sample_texture(uv: vec2<f32>) -> vec4<f32> {
    let t = textureSample(yowie_texture, yowie_sampler, uv);
    return select(t, vec4<f32>(t.r), yowie_mask);
}
```

This is an excerpt. The full prelude is
`crates/yowie-gfx/src/shaders/sprite_prelude.wgsl` in the Yowie source.

| Group, binding | Name | What it holds |
|---|---|---|
| 0, 0 | `yowie_texture` | The texture being drawn, as `texture_2d<f32>`. |
| 0, 1 | `yowie_sampler` | Its sampler, which follows the texture's `filter` and `wrap` options. |
| 1, 0 | `yowie_view` | A `YowieView` uniform: `view_proj: mat4x4<f32>`, `view_size: vec2<f32>`, `pixel_scale: f32`, `time: f32`. |
| 2, 0 | yours | Your uniform block, when you compile with uniforms. |

The engine's entry points are `yowie_vs` and `yowie_fs`. `yowie_fs` builds
the `SpriteInput` and returns what your `fragment` returns. Return
premultiplied colour in linear light; the draw's blend mode then applies
as usual.

Prefer `sample_texture` to sampling `yowie_texture` directly: it turns
glyph masks into white with coverage, so the same shader works on text.
Textures are stored premultiplied, so a sampled texel is already
premultiplied.

### Compiling and drawing

To compile a shader, call `Graphics::compile_shader(source)`, or
`compile_shader_with_uniforms(source, &initial)` for one with a uniform
block. To draw through it, call `Graphics::with_shader(&shader, |g| ...)`,
which restores the previous shader afterwards, or `set_shader(Some(&shader))`
and later `set_shader(None)`. [2D graphics](graphics-2d.md) covers
sprites, blend modes and layers.

```rust,no_run
use yowie::gfx::{Graphics, Shader, Sprite, Texture};
use yowie::{GameResult, vec2};

// Bends the texture coordinate along a sine wave: `params.v.x` is the
// amplitude and `params.v.y` the frequency.
const WAVE: &str = "
struct Params { v: vec4<f32> };
@group(2) @binding(0) var<uniform> params: Params;

fn fragment(input: SpriteInput) -> vec4<f32> {
    var uv = input.uv;
    uv.x += sin(uv.y * params.v.y + input.time * 3.0) * params.v.x;
    return sample_texture(uv) * input.color;
}
";

fn wave_shader(gfx: &Graphics) -> GameResult<Shader> {
    Ok(gfx.compile_shader_with_uniforms(WAVE, &[0.03f32, 18.0, 0.0, 0.0])?)
}

fn draw_flag(gfx: &mut Graphics, wave: &Shader, flag: &Texture) {
    let sprite = Sprite::at(vec2(100.0, 80.0)).size(vec2(200.0, 120.0));
    gfx.with_shader(wave, |g| g.draw_sprite(flag, &sprite));
}
```

A shader applies to every 2D draw inside its scope: sprites, shapes,
paths, nine-slices, tilemaps and text. Distance-field text and radial
gradient fills are the exceptions; they keep the engine's own programs.
Draws through the same shader, texture, blend mode and view batch into one
draw call like any other 2D draw. Cloning a `Shader` shares it.

`input.world` makes effects that follow positions in the game world rather
than the sprite: this shader lights everything drawn through it from a
point, as the `shaders` example's third tile does.

```wgsl
struct Params { v: vec4<f32> };
@group(2) @binding(0) var<uniform> params: Params;

fn fragment(input: SpriteInput) -> vec4<f32> {
    let d = distance(input.world, params.v.xy) / params.v.z;
    let light = clamp(1.0 - d * d, 0.0, 1.0) * 1.6 + 0.08;
    let c = sample_texture(input.uv) * input.color;
    return vec4<f32>(c.rgb * light, c.a);
}
```

`yowie::gfx::ColorMatrixShader` is a ready-made 2D shader that applies a
`yowie::gfx::ColorMatrix`: saturation, hue rotation, brightness,
contrast, sepia, invert, tint, or your own 4×5 matrix. Create it with
`ColorMatrixShader::new(&gfx, &matrix)`, draw through
`color_shader.shader()`, and change the matrix with `set`.

### One texture per draw

A 2D shader reads one texture: the one being drawn. To give a shader more
image data, pack it into the same texture (an atlas, with the second image
beside the first) or into the uniform block.

### Effects over a whole scene

There is no hook for game code inside the post-processing chain. To run
your own full-screen pass, render the scene into a
`yowie::gfx::RenderTexture` and draw that texture through a 2D shader:

```rust,no_run
use yowie::gfx::scene::{Camera3D, Material, Mesh};
use yowie::gfx::{Graphics, RenderTexture, Shader, Sprite};
use yowie::{Color, Vec2, Vec3};

fn draw_through_crt(
    gfx: &mut Graphics,
    target: &RenderTexture,
    crt: &Shader,
    camera: &Camera3D,
    level: &Mesh,
    stone: &Material,
) {
    gfx.draw_to(target, Some(Color::BLACK), |g| {
        g.scene(camera, |s| {
            s.draw(level, stone, Vec3::ZERO);
        });
    });
    let size = gfx.view_size();
    gfx.with_shader(crt, |g| {
        g.draw_sprite(target.texture(), &Sprite::at(Vec2::ZERO).size(size));
    });
}
```

The render texture holds the finished, tone-mapped picture, so the shader
sees what the player would have seen. Make the render texture the size of
the window in pixels so the result stays sharp. The engine's own effects
(bloom, the grade, lens effects) are settings, covered in
[3D graphics](graphics-3d.md#post-processing).

## Surface shaders for 3D materials

A surface shader changes the surface of a material before the engine
lights it. The engine reads the material's factors, textures and decals
into a `Surface`, calls your `material` function, and lights what it
returns with the scene's sun, lights, shadows, sky or environment, probes
and fog. It does not change how the mesh is placed or shaped.

To use one, compile it with `Graphics::compile_surface_shader` or
`compile_surface_shader_with_uniforms`, and set it as `Material::shader`.
This program from the `shaders` example turns a material into flowing
lava:

```wgsl
struct Lava { flow: vec4<f32> };
@group(3) @binding(0) var<uniform> lava: Lava;

fn lava_hash(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2<f32>(127.1, 311.7))) * 43758.5453);
}

fn lava_noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = mix(lava_hash(i), lava_hash(i + vec2<f32>(1.0, 0.0)), u.x);
    let b = mix(lava_hash(i + vec2<f32>(0.0, 1.0)), lava_hash(i + vec2<f32>(1.0, 1.0)), u.x);
    return mix(a, b, u.y);
}

fn material(input: Surface) -> Surface {
    var s = input;
    let t = scene_time() * lava.flow.x;
    let w = s.world;
    let p = vec2<f32>(w.x + w.y * 0.6, w.z - w.y * 0.8) * 2.2 + vec2<f32>(t, t * 0.4);
    let n = lava_noise(p) * 0.65 + lava_noise(p * 2.3 + vec2<f32>(t, 0.0)) * 0.35;
    let crust = smoothstep(0.42, 0.58, n);
    s.base = mix(vec3<f32>(0.9, 0.25, 0.03), vec3<f32>(0.06, 0.05, 0.05), crust);
    s.roughness = mix(0.35, 0.95, crust);
    s.metallic = 0.0;
    let pulse = 0.75 + 0.25 * sin(scene_time() * 3.0 + n * 6.0);
    s.emissive = vec3<f32>(2.2, 0.55, 0.08) * (1.0 - crust) * pulse;
    return s;
}
```

```rust,no_run
use yowie::gfx::Graphics;
use yowie::gfx::scene::Material;
use yowie::GameResult;

fn lava_material(gfx: &Graphics, source: &str) -> GameResult<Material> {
    let shader = gfx.compile_surface_shader_with_uniforms(source, &[0.35f32, 0.0, 0.0, 0.0])?;
    Ok(Material {
        shader: Some(shader),
        ..Material::default()
    })
}
```

### The Surface

The `Surface` your function receives and returns has these fields. A
program usually sets the fields from `base` to `layers` and reads the
fields from `world` down.

| Field | Type | Meaning |
|---|---|---|
| `base` | `vec3<f32>` | Base colour, linear. |
| `alpha` | `f32` | Alpha. Cut-out materials test it after your function runs. |
| `metallic` | `f32` | 0 to 1. |
| `roughness` | `f32` | 0 to 1. |
| `normal` | `vec3<f32>` | World-space unit normal, after normal mapping and flipped for back faces. |
| `occlusion` | `f32` | Ambient occlusion, 1 for none. |
| `emissive` | `vec3<f32>` | Emitted light, linear; values above 1 glow in the HDR image. |
| `unlit` | `bool` | Shows `base` plus `emissive` without lighting. |
| `layers` | `Layers` | The layered features; see below. |
| `world` | `vec3<f32>` | World position. |
| `view` | `vec3<f32>` | Unit vector from the surface towards the eye. |
| `uv`, `uv2` | `vec2<f32>` | The mesh's texture coordinates, before the material's texture transform. |
| `color` | `vec4<f32>` | The vertex colour, linear, straight alpha. |
| `geometric` | `vec3<f32>` | The mesh normal before normal mapping. |
| `tangent` | `vec3<f32>` | A unit tangent. |

`scene_time()` returns the scene's time in seconds. Every field starts
filled from the material, so a program that returns its input unchanged
draws the material as it would draw without a shader; that identity
function is the program the engine itself uses.

`s.layers` holds the [layered features](graphics-3d.md#layered-features).
A zero strength switches a feature off, and a program may turn one on or
change it. Transmission is the exception: the material's own
`transmission` decides whether it draws in the refracting pass, so a
program can change how much light a transmissive material passes but
cannot make an ordinary material refract. The fields are `clearcoat`,
`clearcoat_roughness`, `clearcoat_normal`, `sheen` (linear colour),
`sheen_roughness`,
`subsurface`, `thickness`, `transmission`, `ior`, `volume` (world units),
`attenuation` (linear colour), `attenuation_distance`, `specular`,
`specular_color`, `iridescence`, `iridescence_ior`,
`iridescence_thickness` (nanometres), `anisotropy` and `anisotropy_dir`
(a world-space unit vector).

### Where surface shaders run

- The program runs in every lit pass the material draws in: opaque,
  cut-out, sorted blending, order-independent blending, transmission and
  fur shells.
- The shadow pass ignores it. Cut-out shadows follow the material's own
  alpha, not the alpha your program returns.
- Decals are applied to the `Surface` before your function sees it.
- In a `StaticGroup`, items with a surface shader draw as ordinary draws,
  not from the GPU-culled render bundles.
- A surface shader has no texture bindings of its own. It sees the
  material's textures through the fields the engine fills, so to feed it an
  image, put the image in one of the material's texture slots.
- A surface shader cannot move vertices. For a mesh that changes shape,
  rewrite it with `Mesh::update` or use morph targets.

## Uniforms

To pass values from Rust, compile with `compile_shader_with_uniforms` or
`compile_surface_shader_with_uniforms` and a starting value, then call
`set_uniforms(&value)` whenever the values change. The value is any type
that implements `bytemuck::Pod`: arrays of `f32`, glam types such as
`yowie::Vec2`, `Vec4` and `Mat4`, or your own `#[repr(C)]` struct with
`#[derive(Pod, Zeroable)]`. Add `bytemuck = { version = "1", features =
["derive"] }` to your `Cargo.toml` for the derive.

| Rule | Behaviour |
|---|---|
| Size | `set_uniforms` must be given a value of the same size as the starting value, or it returns `GfxError::UniformSize` and keeps the old values. `Shader::uniform_size` reports the size. |
| Timing | Draws queued before a `set_uniforms` call keep the values they were queued with. To give each draw its own values, set them before each draw. |
| Batching | Draws with different values do not batch together: 2D draws split into separate calls, and 3D materials with different values are separate materials. |
| Checking | The engine does not compare your Rust type with the WGSL block. When the Rust value is smaller, drawing with the shader can fail GPU validation, and the engine logs the error as an uncaptured GPU error. |

### Matching the WGSL layout

A uniform block follows WGSL's layout rules for the uniform address space,
and your Rust type must match them byte for byte.

| WGSL type | Size | Alignment | Rust type |
|---|---|---|---|
| `f32`, `i32`, `u32` | 4 | 4 | `f32`, `i32`, `u32` |
| `vec2<f32>` | 8 | 8 | `[f32; 2]` or `Vec2` |
| `vec3<f32>` | 12 | 16 | `[f32; 3]` or `Vec3`, at an offset that is a multiple of 16 |
| `vec4<f32>` | 16 | 16 | `[f32; 4]` or `Vec4` |
| `mat4x4<f32>` | 64 | 16 | `[[f32; 4]; 4]` or `Mat4`, column by column |
| `array<vec4<f32>, N>` | 16 N | 16 | `[[f32; 4]; N]` or `[Vec4; N]` |
| a struct | the end of its last member, rounded up to its alignment | its largest member's, and at least 16 inside another struct | a `#[repr(C)]` struct |

Check these points when you lay out a block:

- A `vec3<f32>` aligns to 16 but is 12 bytes long, so a following `f32`
  fills the gap. Put each `vec3` at a multiple of 16 and follow it with a
  scalar, or use `vec4`.
- In a uniform block an array's element stride must be a multiple of 16,
  so `array<f32, N>` is not allowed. Use `array<vec4<f32>, N>` and pack
  four values per element.
- Keep the whole block a multiple of 16 bytes; pad the end with unused
  fields if needed.
- `#[derive(Pod)]` rejects a struct with padding between its fields. It
  finds a mismatch only where Rust itself would insert padding, so check
  each offset against the WGSL rules yourself, and write any padding as
  named fields.

```rust,no_run
use bytemuck::{Pod, Zeroable};
use yowie::gfx::{Graphics, Shader};
use yowie::{GameResult, Vec2, Vec3};

// Matches the WGSL `Ripple` struct: 32 bytes.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Ripple {
    centre: Vec2,  // vec2<f32> at offset 0
    radius: f32,   // f32 at 8
    strength: f32, // f32 at 12
    tint: Vec3,    // vec3<f32> at 16
    time: f32,     // f32 at 28
}

fn ripple_shader(gfx: &Graphics, source: &str) -> GameResult<Shader> {
    let calm = Ripple {
        centre: Vec2::ZERO,
        radius: 0.0,
        strength: 0.0,
        tint: Vec3::ONE,
        time: 0.0,
    };
    Ok(gfx.compile_shader_with_uniforms(source, &calm)?)
}

fn splash(ripple: &Shader, at: Vec2, t: f32) -> GameResult {
    ripple.set_uniforms(&Ripple {
        centre: at,
        radius: 60.0 * t,
        strength: 0.02,
        tint: Vec3::new(0.8, 0.9, 1.0),
        time: t,
    })?;
    Ok(())
}
```

```wgsl
struct Ripple {
    centre: vec2<f32>,
    radius: f32,
    strength: f32,
    tint: vec3<f32>,
    time: f32,
};
@group(2) @binding(0) var<uniform> ripple: Ripple;

fn fragment(input: SpriteInput) -> vec4<f32> {
    let offset = input.world - ripple.centre;
    let ring = exp(-abs(length(offset) - ripple.radius) * 0.2);
    let dir = offset / max(length(offset), 1e-4);
    let c = sample_texture(input.uv + dir * ring * ripple.strength);
    let shade = mix(vec3<f32>(1.0), ripple.tint, ring);
    return vec4<f32>(c.rgb * shade, c.a) * input.color;
}
```

## Compile errors

A compile function and `reload_source` return `GfxError::Shader` when the
source does not compile or does not fit the engine's pipeline. Print the
error with `{}` to get its message. It starts with `shader error:`, and
each error starts with its line and column counted in your own source,
followed by naga's report. The `shaders` example compiles this source,
whose first line is the empty line after the opening quote:

```rust
// A typo on line 3: `colour` is not a field of `SpriteInput`.
const BROKEN: &str = "
fn fragment(input: SpriteInput) -> vec4<f32> {
    return sample_texture(input.uv) * input.colour;
}
";
```

The error reads:

```text
shader error:
line 3, column 45:
Shader 'yowie game shader' parsing error: invalid field accessor `colour`
   ┌─ wgsl:84:45
   │
84 │     return sample_texture(input.uv) * input.colour;
   │                                             ^^^^^^ invalid accessor
```

The line numbers in naga's report count the prelude as well, so they are
larger than the ones on the first line. Other errors and their causes:

| Starts with | Then says | Cause |
|---|---|---|
| `engine prelude line N:` | no definition in scope for identifier `fragment` | The 2D source has no `fragment` function, or its name is misspelt. |
| `engine line N:` | no definition in scope for identifier `material` | The surface source has no `material` function. |
| `line L, column C:` | redefinition of a name, such as `sqr` | Your source declares a name the prelude already declares. Rename yours. |
| `line L, column C:` | Shader validation error: Global variable ... is invalid | A declaration breaks WGSL's rules, such as `array<f32, N>` in a uniform block. |
| `Validation Error` | In Device::create_render_pipeline ... Error matching ShaderStages(FRAGMENT) shader requirements against the pipeline | The module compiled, but its bindings do not fit the engine's pipeline: a uniform block in a shader compiled without uniforms, or a binding at a group or binding number the engine does not provide. |

The `shaders` example draws its broken shader's error text beside its
tiles.

## Hot reload

To edit a 2D shader while the game runs, load it from the assets with
`Context::load_shader(path)` or `Context::load_shader_with_uniforms(path,
&initial)`. With `Config::hot_reload` on (the default in debug builds), the
engine recompiles the shader in place when the file changes, and every
draw that uses it, and every clone of it, runs the new program. The
uniform values stay. A source that fails to compile is logged as a warning
and the previous program keeps running. The engine stops watching a shader
once the game drops every clone of it.

```rust,no_run
use yowie::gfx::Shader;
use yowie::{Context, GameResult};

fn load_water_shader(ctx: &mut Context) -> GameResult<Shader> {
    Ok(ctx.load_shader_with_uniforms("shaders/water.wgsl", &[0.03f32, 18.0, 0.0, 0.0])?)
}
```

To recompile a shader from source you hold yourself, call
`reload_source(&gfx, &source)` on a `Shader` or a `SurfaceShader`. It keeps
the uniform values, and on an error it returns the error and keeps the
previous program. The `shaders` example reloads its wave and lava shaders
this way when you press R.

Surface shaders have no loader on `Context`. To follow a surface shader's
file on disk, load its text as a `yowie::content::ShaderSource` asset and
reload when the handle's generation changes:

```rust,no_run
use yowie::asset::Handle;
use yowie::content::ShaderSource;
use yowie::gfx::scene::SurfaceShader;
use yowie::{Context, GameResult};

struct LiveSurface {
    handle: Handle<ShaderSource>,
    seen: u64,
    shader: SurfaceShader,
}

impl LiveSurface {
    fn load(ctx: &mut Context, path: &str) -> GameResult<Self> {
        let handle = ctx.assets.load_now::<ShaderSource>(path)?;
        let source = handle.get().ok_or("shader source not ready")?;
        let shader = ctx.gfx.compile_surface_shader(&source.0)?;
        Ok(LiveSurface {
            seen: handle.generation(),
            handle,
            shader,
        })
    }

    // Call once per frame, from `update`.
    fn poll(&mut self, ctx: &Context) {
        let generation = self.handle.generation();
        if generation == self.seen {
            return;
        }
        self.seen = generation;
        if let Some(source) = self.handle.get()
            && let Err(e) = self.shader.reload_source(&ctx.gfx, &source.0)
        {
            eprintln!("{}: {e}", self.handle.path());
        }
    }
}
```

See [game services](services.md) for the asset system and hot reload in
general.

## Custom GPU draws

When a hook is not enough (your own vertex layout, compute work, extra
textures, instanced effects), implement `yowie::gfx::GpuDraw` and queue it
as an `Arc<dyn GpuDraw>`. The GPU particles in `yowie::particle` are built
this way, and `crates/yowie-particle/src/gpu/draw.rs` in the Yowie source
is a complete implementation to read.

| Method | Called |
|---|---|
| `prepare(&self, ctx: &mut PrepareContext)` | Before any pass of the frame, with the device, queue and command encoder, for compute work and uploads. |
| `draw_2d(&self, pass, ctx: &Draw2D)` | Inside the 2D pass, when queued with `Graphics::draw_gpu`. Ordered by layer and sort key like a sprite. |
| `draw_3d(&self, pass, ctx: &Draw3D)` | Inside a scene's HDR pass, when queued with `Scene::draw_gpu`. Runs after the blended meshes and before the debug lines. |

Each has a default that does nothing. Set the pipeline and every bind
group your draw needs, apart from group 0 in a 3D draw, which is already
bound. You may change any binding; the renderer rebinds its own state
after your draw returns. Queue the same draw twice in a frame (to a render
texture and to the screen) and `prepare` runs twice, so advance a
simulation only on the first `prepare` of a frame.

What a 2D draw receives in `Draw2D`: the target `format`, the view block's
layout, bind group and dynamic offset (the `YowieView` struct above), and
`pixel_scale`. The viewport and scissor are already set. Blending is
yours; the current blend mode and transform do not apply.

What a 3D draw receives in `Draw3D`, and must match:

- Group 0 is bound to the scene's frame block; build your pipeline layout
  with `frame_layout` at group 0 to read it.
- The colour target has `color_format` (HDR) and the depth attachment has
  `depth_format`. Depth is reversed (the far plane is 0): test with
  `CompareFunction::GreaterEqual` and do not write depth.
- `sample_count` is 1, or 2, 4 or 8 when the scene multisamples. Your
  pipeline's multisample count must match, so build pipelines per count.
- `extra_targets` lists the pass's colour targets after the HDR colour.
  Append them to your pipeline's targets as given; they have empty write
  masks.

To keep pipelines across frames, store them with
`Graphics::extension(|device| ...)`, which builds a value once per renderer
and shares it after. `Graphics::downlevel_capabilities` says whether the GPU
has compute shaders and indirect draws. Probe captures leave custom 3D
draws out.

You create the shader modules of a custom draw yourself with wgpu, so the
engine's preludes do not apply to them.

## Bounds checks

The engine creates its own shader modules (the default sprite program,
distance-field text, gradients, the mesh program, the sky,
post-processing, culling and GPU particles) with wgpu's runtime bounds
checks turned off. Game shaders go through wgpu's checked path: 2D shaders
and surface shaders are compiled with the checks on, so an out-of-range
index in your code cannot read outside a buffer or array. A surface shader
is compiled together with the engine's mesh program, so a material with a
surface shader runs that whole program with the checks on, which can make
it slower on the GPU than the same material without one.
