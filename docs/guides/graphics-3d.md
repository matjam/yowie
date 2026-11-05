---
title: 3D graphics
group: Graphics
order: 2
summary: cameras, meshes and glTF models, materials, lights and shadows, sky and fog, probes, terrain and large scenes, culling and LOD, post-processing and the costs and limits of each
---

# 3D graphics

The renderer in `yowie::gfx` draws 3D scenes and 2D sprites into the same
frame. This guide covers the 3D half: cameras, geometry, glTF models,
materials, lights and shadows, the sky, indirect light, large outdoor
scenes, picking, post-processing, and what each feature costs. The 2D half
is in [2D graphics](graphics-2d.md), and game-written WGSL is in
[shaders](shaders.md).

Every type in this guide lives in `yowie::gfx::scene` unless the text says
otherwise. For the full API, run `cargo doc --open`.

These examples show the features in use:

| Example | What it shows |
|---|---|
| `cargo run --example viewer` | An orbit camera, primitives, a glTF creature, translucent panes, a 2D panel over the scene. `--model <file.gltf\|file.glb>` shows your own model; `--sorted` sorts the panes instead of compositing them order-independently. |
| `cargo run --example materials` | Every material feature on a row of spheres, cut-out leaves, a decal, an outline, an x-ray tint and a stencil mask. `--env <panorama>` lights it from an image. |
| `cargo run --example lighting` | Cascaded sun shadows, shadowed point lights, 160 clustered lights, an atmospheric sky, and every post-processing setting on a slider. See its flags under [Post-processing](#post-processing). |
| `cargo run --example probes` | A reflection probe and a light probe grid baked in a room built as a static group. `--show-probes` draws the grid cells; `--ssr` turns on screen-space reflections. |
| `cargo run --example terrain` | Chunked terrain, a lake, impostors, billboards, levels of detail, valley fog, an atmospheric sky and world labels. |
| `cargo run --example physics3d` | Five hundred cubes in many materials, picked with a physics ray. |
| `cargo run --example softbody` | Meshes rewritten every frame from a cloth and a soft body. |
| `cargo run --example solar` | Instanced asteroids, picking, and a render texture minimap with an orthographic camera. |

## Conventions

| Topic | Convention |
|---|---|
| Handedness | Right-handed, +Y up. A camera looks down -Z in its own space. |
| Units | World units. The renderer fixes no scale, but its defaults (a near plane of 0.1, a sun shadow distance of 60, an occlusion radius of 1) suit one unit per metre, which is also the glTF unit. |
| Angles | Radians everywhere: fields of view, cone angles, rotations. |
| Colours | `yowie::Color` values are sRGB with straight alpha. The renderer converts them to linear light and lights in linear light. |
| Light | The scene renders into a high dynamic range (HDR) image. Light intensities are multipliers on the colour, and values above 1 are normal. |
| Depth | Reversed on the GPU: the near plane maps to 1 and the far plane to 0. You only meet this in custom GPU draws. |
| Triangles | Counter-clockwise seen from the front. Back faces are culled unless the material is double-sided. |
| Texture coordinates | The origin is the top-left of the image and V runs down. |
| Placement | Every draw takes anything that implements `Placement`: a `Mat4`, a `yowie::Transform`, an `Affine3A`, or a `Vec3` position. |

## How a frame is put together

To draw 3D, call `Graphics::scene` during `draw` with a camera and a closure.
The closure receives a `Scene` and records into it. Nothing renders until
the frame ends, so the order of the calls inside the closure does not matter
for settings: the last `set_sun` wins, wherever it appears.

When the frame ends, each output renders in this order:

1. Render textures you drew into with `Graphics::draw_to`, in the order
   their scopes finished, then the screen.
2. For each output, every scene recorded for it, in call order. A scene
   renders its shadow maps, then its HDR colour, depth and stencil images:
   opaque and cut-out meshes, the sky, outlines, glass, x-ray tints,
   blended meshes, GPU particles and debug lines.
3. Each scene's post-processing chain, which ends in the scene's viewport
   of the output. With the default settings the chain is exposure and a
   tone curve.
4. The 2D pass: sprites, text, the interface and the console, over
   whatever the scenes left.

So 2D always draws over 3D in the same output, whatever order the calls
come in. To draw a HUD, draw it with the 2D calls after the scene. To put a
3D object under 2D sprites, render the scene into a `RenderTexture` and
draw that texture as a sprite on a low layer.

`Graphics::set_frame_post` changes step 3 for a whole output: the scenes
and the 2D layers below `yowie::gfx::layers::UI` draw into an HDR image,
one post chain runs over all of it, and the interface layers draw on top
untouched. See [Post-processing](#post-processing).

## A first scene

This program draws a turning cube on a floor under a shadowed sun and a
sky, with an orbit camera:

```rust,no_run
use yowie::gfx::scene::{DirectionalLight, Material, Mesh, MeshData, Sky};
use yowie::input::Key;
use yowie::{Color, Config, Context, Game, GameResult, Mat4, OrbitCamera, Quat, Vec3};

struct Showcase {
    orbit: OrbitCamera,
    cube: Mesh,
    floor: Mesh,
    spin: f32,
}

impl Game for Showcase {
    fn init(ctx: &mut Context) -> GameResult<Self> {
        Ok(Showcase {
            orbit: OrbitCamera::new(Vec3::ZERO, 0.6, 0.4, 6.0),
            cube: Mesh::new(&ctx.gfx, &MeshData::cube(1.0))?,
            floor: Mesh::new(&ctx.gfx, &MeshData::plane(20.0, 1))?,
            spin: 0.0,
        })
    }

    fn update(&mut self, ctx: &mut Context) -> GameResult {
        if ctx.input.key_pressed(Key::Escape) {
            ctx.quit();
        }
        let dt = ctx.time.delta();
        self.orbit.update(&ctx.input, dt);
        self.spin += dt;
        Ok(())
    }

    fn draw(&mut self, ctx: &mut Context) -> GameResult {
        let camera = self.orbit.camera();
        let red = Material::color(Color::rgb8(200, 50, 40), 0.0, 0.4);
        let grass = Material::color(Color::rgb8(90, 140, 70), 0.0, 0.9);
        let cube_at = Mat4::from_rotation_translation(
            Quat::from_rotation_y(self.spin),
            Vec3::new(0.0, 0.5, 0.0),
        );
        ctx.gfx.scene(&camera, |s| {
            s.set_sun(DirectionalLight::default().with_shadows(30.0));
            s.set_sky(Sky::default());
            s.draw(&self.floor, &grass, Vec3::ZERO);
            s.draw(&self.cube, &red, cube_at);
        });
        Ok(())
    }
}

fn main() -> Result<(), yowie::Error> {
    yowie::run::<Showcase>(Config::new("Showcase").with_size(1280, 720))
}
```

The rest of this guide takes each part in turn.

## Cameras

To describe a view, build a `Camera3D`. It looks from `position` at
`target`, with `up` as the world direction that appears up.

| Field | Default | Meaning |
|---|---|---|
| `position` | `(0, 0, 5)` | The eye. |
| `target` | origin | The point the camera looks at. |
| `up` | `Vec3::Y` | Must not be parallel to the view direction. |
| `projection` | perspective, 60 degrees | `Projection::Perspective { fov_y }` (vertical field of view, radians) or `Projection::Orthographic { half_height }` (half the visible height in world units). |
| `near` | 0.1 | The nearest visible distance, above zero. |
| `far` | 1000 | The farthest visible distance. |
| `viewport` | `None` | The rectangle of the view the scene draws into, in view units. `None` fills the view. |

`Camera3D::perspective(position, target, fov_y)` and
`Camera3D::orthographic(position, target, half_height)` build the two
kinds. `with_viewport` and `with_depth_range` return a changed copy. An
orthographic camera keeps sizes constant with distance, which suits
isometric and top-down views.

To split the screen, give each player a camera with its own viewport and
call `scene` once per camera. Scenes that share an output render in call
order.

```rust,no_run
use yowie::gfx::Graphics;
use yowie::gfx::scene::{Camera3D, Material, Mesh};
use yowie::{Rect, Vec3};

fn draw_split(gfx: &mut Graphics, players: [Vec3; 2], arena: &Mesh, stone: &Material) {
    let view = gfx.view_size();
    let half = view.x * 0.5;
    for (i, at) in players.into_iter().enumerate() {
        let eye = at + Vec3::new(0.0, 4.0, 8.0);
        let camera = Camera3D::perspective(eye, at, 60f32.to_radians())
            .with_viewport(Rect::new(i as f32 * half, 0.0, half, view.y));
        gfx.scene(&camera, |s| {
            s.draw(arena, stone, Vec3::ZERO);
        });
    }
}
```

To place 2D over a 3D point, call `Camera3D::world_to_screen(world,
view_size)`. It returns the point in view units, or `None` when the point
is behind the camera. To turn the mouse into a ray for picking, call
`Camera3D::screen_ray(mouse, view_size)` or, inside a scene,
`Scene::screen_ray(mouse)`. Both respect the viewport.

### Input-driven cameras

The root crate has two cameras that read input. Call `update` in your
game's `update` and draw with `camera()`. Each keeps a `lens: Camera3D`
whose projection, depth range and viewport it copies into the camera it
returns.

- `yowie::FlyCamera` flies freely. W, A, S and D move, Q and E go down
  and up, Shift multiplies the speed by `fast`, and the view turns while
  the right mouse button is held, or with every pointer movement when
  `always_look` is set (for a captured cursor). `speed` is in world units
  per second. `look_at` turns it towards a point.
- `yowie::OrbitCamera` circles `target` at `distance`. Dragging with the
  left button turns it, the wheel zooms between `min_distance` and
  `max_distance`, dragging with the right or middle button pans the
  target, and the arrow keys turn it at `key_speed` radians per second.
  Yaw zero puts the camera on the +Z side of the target.

```rust,no_run
use yowie::{FlyCamera, Vec3};

fn spawn_camera() -> FlyCamera {
    let mut fly = FlyCamera::new(Vec3::new(0.0, 5.0, 20.0));
    fly.look_at(Vec3::ZERO);
    fly.speed = 15.0;
    fly.lens.far = 2000.0;
    fly
}
```

## Meshes

A `Mesh` is indexed triangle geometry on the GPU. To make one, fill a
`MeshData` and call `Mesh::new(&gfx, &data)`. Cloning a mesh shares it;
the GPU memory is freed when the last clone and every queued draw that
uses it are gone.

`MeshData` has generators for common shapes, each centred on the origin
with normals, tangents and texture coordinates:

| Generator | Shape |
|---|---|
| `MeshData::cube(size)` | A cube `size` on a side, each face with its own texture square. |
| `MeshData::sphere(radius, rings, segments)` | A UV sphere. |
| `MeshData::plane(size, subdivisions)` | A square in the XZ plane facing +Y. |
| `MeshData::cylinder(radius, height, segments)` | A capped cylinder along Y. |
| `MeshData::capsule(radius, half_height, rings, segments)` | A capsule along Y; `half_height` is the straight part above and below the centre. |
| `MeshData::torus(major, minor, rings, segments)` | A torus in the XZ plane. |

To build geometry yourself, push `Vertex3D` values and three indices per
triangle. `Vertex3D::new(position, normal, uv)` fills the rest with
defaults. A vertex also has `tangent` (with the bitangent sign in `w`, as
glTF stores it), `uv2` (a second coordinate set), `color` (sRGB, multiplied
into the base colour when the material uses vertex colours) and `joints`
and `weights` for skinning. `compute_normals` and `compute_tangents`
fill normals and tangents from the triangles, which normal maps need.

```rust,no_run
use yowie::gfx::Graphics;
use yowie::gfx::scene::{Mesh, MeshData, Vertex3D};
use yowie::{GameResult, Vec2, Vec3};

fn ramp(gfx: &Graphics) -> GameResult<Mesh> {
    let corners = [
        Vec3::new(-1.0, 0.0, 1.0),
        Vec3::new(1.0, 0.0, 1.0),
        Vec3::new(1.0, 1.0, -1.0),
        Vec3::new(-1.0, 1.0, -1.0),
    ];
    let uvs = [
        Vec2::new(0.0, 1.0),
        Vec2::new(1.0, 1.0),
        Vec2::new(1.0, 0.0),
        Vec2::new(0.0, 0.0),
    ];
    let vertices = corners
        .iter()
        .zip(uvs)
        .map(|(p, uv)| Vertex3D::new(*p, Vec3::Y, uv))
        .collect();
    let mut data = MeshData::new(vertices, vec![0, 1, 2, 0, 2, 3]);
    data.compute_normals();
    data.compute_tangents();
    Ok(Mesh::new(gfx, &data)?)
}
```

`MeshData::append` merges shapes into one mesh, `transformed` moves a copy,
and `with_color` sets every vertex colour. `Mesh::new` fails on geometry
with no triangles, an index count that is not a multiple of three, or an
index past the last vertex.

### Changing a mesh

To replace the geometry of a mesh that changes (dug terrain, voxel
chunks, cloth), call `Mesh::update(&gfx, &data)`. It rewrites the buffers
in place, or replaces them when the new geometry is larger, and every draw
of the current frame sees the new geometry. The `softbody` example rewrites
two meshes this way every frame.

Culling uses `Mesh::bounds`, the box around the vertices with any morph
targets at full weight. When a mesh draws outside that box (a skinned mesh
posed far from its rest shape, for example), call `Mesh::set_bounds` with a
box that holds the drawn shape. Updates keep the box you set.

### Drawing meshes

| Call | Use |
|---|---|
| `Scene::draw(mesh, material, placement)` | One mesh. Draws of the same mesh with an equal material merge into one instanced draw call. |
| `Scene::draw_instanced(mesh, material, &[Mat4])` | Many copies in one call. |
| `Scene::draw_moved(mesh, material, placement, previous)` | One mesh that also says where it was last frame, for temporal anti-aliasing and motion blur. |
| `Scene::draw_skinned(mesh, material, placement, &joints)` | A skinned mesh posed by joint matrices: each joint's model-space matrix times its inverse bind matrix, as `yowie::anim::Skeleton::joint_matrices` returns. |
| `Scene::draw_morphed(mesh, material, placement, &weights)` | A mesh made with `Mesh::with_morph_targets`, blended by one weight per target. |

Each call returns a `DrawId` that names the draw within the scene, for
picking and for `hide_in_probes`.

```rust,no_run
use yowie::gfx::scene::{Material, Mesh, Scene};
use yowie::{Mat4, Quat, Transform, Vec3};

fn draw_crates(s: &mut Scene, crate_mesh: &Mesh, wood: &Material, stacks: &[Vec3]) {
    // One placement per crate, one draw call for all of them.
    let models: Vec<Mat4> = stacks.iter().map(|p| Mat4::from_translation(*p)).collect();
    s.draw_instanced(crate_mesh, wood, &models);
    // A Transform places a mesh too.
    let tipped = Transform::from_xyz(0.0, 2.0, 0.0).with_rotation(Quat::from_rotation_z(0.3));
    s.draw(crate_mesh, wood, tipped);
}
```

Before rendering, the renderer culls each draw's bounding sphere against
the camera, sorts opaque draws by pipeline, material and mesh, merges equal
neighbours into instanced runs, and orders the runs front to back. Two
materials are equal when every field is equal, so build a material once
and reuse it, or build it the same way each frame, to keep crowds in one
call. A skinned draw is culled by its rest box moved by each joint.

## glTF models

To load a model, call `Model::load(&gfx, path)` for a `.gltf` or `.glb` file
on disk, or `Model::from_slice(&gfx, &bytes)` for data with embedded
buffers and images. `from_slice` suits files read through the asset system
(see [game services](services.md)), including pack files:

```rust,no_run
use yowie::gfx::scene::{Model, ModelInstance};
use yowie::{Context, GameResult};

fn load_knight(ctx: &mut Context) -> GameResult<ModelInstance> {
    let bytes = ctx.assets.vfs().read("models/knight.glb")?;
    let model = Model::from_slice(&ctx.gfx, &bytes)?;
    let mut knight = model.instance();
    knight.play("Idle");
    Ok(knight)
}
```

A `Model` holds the meshes, materials, node hierarchy, skins, morph targets
and animation clips, and cloning it shares them. To draw it, make a
`ModelInstance` with `Model::instance`: an instance holds one pose and an
animation player, so a crowd of the same model is one `Model` and many
instances.

| Call | What it does |
|---|---|
| `ModelInstance::play(name)` | Starts a clip from its beginning; false when there is no such clip. |
| `ModelInstance::crossfade(name, seconds)` | Fades from the current clip to another. |
| `ModelInstance::advance(dt)` | Runs the player and recomputes the pose. Call it in `update`. |
| `ModelInstance::player` | The `yowie::anim::Player`, for blending and events; see [animation](animation.md). Call `update_pose` after changing it directly. |
| `ModelInstance::node_matrix(node)` | A node's model-space matrix, to attach a weapon to a hand. |
| `ModelInstance::set_node_local(node, transform)` | Overrides a node's local transform until the next pose update, for aiming or inverse kinematics. |
| `ModelInstance::set_morph_weights(node, weights)` | Sets a node's morph weights until the next pose update. |
| `ModelInstance::set_material_override(part, material)` | Draws one part with another material; `None` restores it. |
| `Scene::draw_model(instance, placement)` | Draws every part. Returns the first part's `DrawId`; the others follow in part order. |
| `Scene::draw_model_with(instance, placement, f)` | Draws every part, asking `f` for a replacement material per part. |

`Model::node_index(name)` finds a node, `Model::clips()` lists the clips,
`Model::parts()` lists the drawable parts with their material indices, and
`Model::bounds()` is the rest-pose box, useful to frame a camera on a model
of unknown size.

```rust,no_run
use yowie::gfx::scene::{Material, Mesh, ModelInstance, Scene};
use yowie::Mat4;

fn draw_knight(s: &mut Scene, knight: &ModelInstance, sword: &Mesh, steel: &Material, at: Mat4) {
    s.draw_model(knight, at);
    let hand = knight
        .model()
        .node_index("hand_r")
        .and_then(|n| knight.node_matrix(n));
    if let Some(hand) = hand {
        s.draw(sword, steel, at * hand);
    }
}
```

To build a model in code from meshes, call `Model::from_parts(&[(mesh,
material, placement)])`. The result draws, bakes into an impostor and takes
levels of detail like a loaded model.

The loader reads every triangle primitive, the metallic-roughness
material with its textures, alpha mode and double-sidedness, and the
extensions `KHR_materials_unlit`, `KHR_texture_transform`,
`KHR_materials_emissive_strength`, `KHR_materials_clearcoat`, `_sheen`,
`_transmission`, `_volume`, `_ior`, `_specular`, `_iridescence` and
`_anisotropy`. The base colour texture's transform applies to all of a
material's textures. A model's images are expanded to 8-bit RGBA textures
when it loads, so they do not benefit from KTX2 compression.

## Materials

A `Material` is a metallic-roughness physically based material, as glTF
defines it, plus optional layered features. The factors alone are a
complete material; every texture is optional.

`Material::color(base, metallic, roughness)` builds a plain material and
`Material::glowing(color, strength)` an unlit emissive one. For anything
else, set fields and fill the rest from `Material::default()` or one of
those constructors.

| Field | Default | Meaning |
|---|---|---|
| `base_color` | white | Multiplies the base colour texture and vertex colours; its alpha is the surface's alpha. |
| `base_color_texture` | `None` | The albedo, sRGB. |
| `metallic` | 0 | 0 for a dielectric, 1 for a metal. |
| `roughness` | 1 | 0 for a mirror, 1 for fully matte. |
| `metallic_roughness_texture` | `None` | Roughness in green, metallic in blue, multiplying the factors. |
| `normal_texture`, `normal_scale` | `None`, 1 | A tangent-space normal map and the scale of its X and Y. |
| `occlusion_texture`, `occlusion_strength` | `None`, 1 | Ambient occlusion in red, and how much it darkens ambient light. |
| `emissive`, `emissive_strength`, `emissive_texture` | black, 1, `None` | Emitted light. A strength above 1 glows past white in the HDR image, which bloom picks up. |
| `alpha_mode` | `AlphaMode::Opaque` | `Opaque`, `Mask(cutoff)` or `Blend`. |
| `double_sided` | false | Draws back faces too, lit with a flipped normal. |
| `unlit` | false | Shows the base colour and emissive without lighting. |
| `vertex_colors` | true | Whether vertex colours multiply the base colour. |
| `uv_transform` | identity | A `TextureTransform` (scale, then rotation, then offset) for every texture. `TextureTransform::tiled(x, y)` repeats the textures. |
| `uv_sets` | all 0 | Which coordinate set (0 or 1) each core texture reads. |
| `ior` | 1.5 | Index of refraction: 1.5 for glass and most plastics, 1.33 for water. |
| `receive_decals` | true | Whether decals project onto the surface. |
| `stencil` | draws everywhere | See [Stencil masks](#stencil-masks). |
| `shader` | `None` | A game-written surface program; see [shaders](shaders.md). |

### Textures for materials

Textures that hold colour (base colour, emissive) are sRGB. Textures that
hold data (metallic-roughness, normal, occlusion and every feature map)
must be created with `TextureOptions::data` set, so they are sampled
without colour conversion. The five core textures sample with each
texture's own filter and wrap settings, so set `wrap: Wrap::Repeat` on
textures you tile and turn mipmaps on. The feature maps share one linear,
repeating, mipmapped sampler.

```rust,no_run
use yowie::gfx::scene::{Material, TextureTransform};
use yowie::gfx::{TextureOptions, Wrap};
use yowie::{Color, Context, GameResult};

fn brick_wall(ctx: &mut Context) -> GameResult<Material> {
    let colour = TextureOptions {
        wrap: Wrap::Repeat,
        ..TextureOptions::smooth()
    };
    let data = TextureOptions { data: true, ..colour };
    // Occlusion in red, roughness in green, metallic in blue.
    let orm = ctx.load_texture("bricks/orm.png", data)?;
    Ok(Material {
        base_color_texture: Some(ctx.load_texture("bricks/albedo.png", colour)?),
        normal_texture: Some(ctx.load_texture("bricks/normal.png", data)?),
        metallic_roughness_texture: Some(orm.clone()),
        occlusion_texture: Some(orm),
        uv_transform: TextureTransform::tiled(4.0, 2.0),
        ..Material::color(Color::WHITE, 0.0, 1.0)
    })
}
```

With hot reload on (the default in debug builds), a texture loaded with
`Context::load_texture` is replaced in place when its file changes, and
every material that holds it shows the new image.

### Compressed textures

To cut texture memory and load time, compress images to KTX2 with the
`yowie-tex` tool from `crates/yowie-tools`, and load the `.ktx2` file like
any other image. The file carries its mip chain.

```bash
cargo run -p yowie-tools --bin yowie-tex -- -v --format bc7 art/bricks/albedo.png
cargo run -p yowie-tools --bin yowie-tex -- --format bc7 --linear --outdir build art/bricks/normal.png
```

| Format | Use |
|---|---|
| `bc7` (the default) | Colour that must hold up close, and normal maps with `--linear`. |
| `bc1` | Opaque colour at half the size of BC7. |
| `bc3` | Colour with alpha. |
| `bc4` | One-channel data: masks, fur strands, height. Always linear. |
| `bc5` | Two-channel data (red and green). Always linear. |

Pass `--linear` for any texture that holds data. `--no-mips` writes level 0
alone, `--filter kaiser` changes the mip filter, `--fast` trades a little
BC7 quality for speed, and `--straight-alpha` keeps alpha straight instead
of premultiplying. Material normal maps read red, green and blue, so a
two-channel BC5 file does not work as a material normal map; use BC7 with
`--linear`.

`Graphics::supports_bc` says whether the GPU takes block-compressed
textures. Desktop GPUs do; many mobile GPUs do not. Check it before
loading `.ktx2` files and fall back to PNG when it is false.

The file format code is the `yowie-ktx2` crate in the workspace: the
renderer reads files with it and `yowie-tex` writes them. The `yowie`
crate does not re-export it, so a tool of your own adds it as a
dependency. `Ktx2File::open(path)` or `Ktx2File::parse(&bytes)` returns
the `width`, `height`, Vulkan `format`, `premultiplied` flag and one byte
vector per mip level in `levels`; `to_bytes` writes a file back. A file
holds one 2D block-compressed texture.

### Alpha and transparency

`AlphaMode::Mask(cutoff)` discards fragments whose alpha is below the
cutoff, in the lit pass and in the shadow pass, so leaves and fences cast
cut-out shadows. Cut-out materials write depth and sort with opaque ones.

`AlphaMode::Blend` draws the surface after the opaque scene, with alpha
fading the whole shaded result. How blended surfaces combine is a setting
of the scene, `Scene::set_transparency`:

| `Transparency` | Behaviour |
|---|---|
| `OrderIndependent` (the default) | Weighted blended accumulation into two extra images, then one composite. No sorting, so crossing surfaces composite without a seam and equal blended draws batch into one call. Nearer surfaces weigh more, but the result approximates exact ordering. |
| `Sorted` | Draws back to front by the centre of each draw's bounds. Exact for separate surfaces, wrong where surfaces cross. |

To compare the two on crossing panes, run `cargo run --example viewer` and
then `cargo run --example viewer -- --sorted`. Blended and transmissive
surfaces cast no shadows in either mode.

### Layered features

Each feature is an `Option` on `Material` (or a number, for subsurface).
`None` switches it off and costs nothing. Each follows the glTF extension
named in the table.

| Field | Type and constructor | Look |
|---|---|---|
| `clearcoat` | `Clearcoat::new(strength, roughness)` | A varnish with its own roughness and optional normal map (`KHR_materials_clearcoat`): car paint, lacquer, wet surfaces. |
| `sheen` | `Sheen::new(color, roughness)` | Soft light at grazing angles (`KHR_materials_sheen`): velvet, felt, cloth. |
| `specular` | `Specular::tint(color)` | Scales and tints a dielectric's reflection (`KHR_materials_specular`). |
| `subsurface`, `thickness_texture` | `f32` from 0 to 1 | Light let through thin parts and tinted by the base colour: leaves, wax, skin. The thickness texture keeps it out of thick parts. |
| `transmission` | `Transmission::glass(thickness)` | Refracting glass with absorption (`KHR_materials_transmission` and `_volume`). |
| `iridescence` | `Iridescence::new(thickness_nm)` | A thin film whose hue turns with the angle (`KHR_materials_iridescence`). |
| `anisotropy` | `Anisotropy::new(strength)` | A highlight stretched along the tangents (`KHR_materials_anisotropy`): brushed metal, hair. |
| `fur` | `Fur::new(shells, length)` | The mesh drawn again as shells along its normals: fur, grass, moss. |
| `outline` | `Outline::new(width_px, color)` | A line around the silhouette, hidden where geometry stands in front. |
| `xray` | `Option<Color>` | A tint drawn over the parts of the mesh hidden behind other geometry. |

```rust,no_run
use yowie::gfx::scene::{Clearcoat, Material, Sheen, Transmission};
use yowie::Color;

fn showroom() -> [Material; 3] {
    let car_paint = Material {
        clearcoat: Some(Clearcoat::new(1.0, 0.05)),
        ..Material::color(Color::rgb8(150, 20, 30), 0.0, 0.6)
    };
    let velvet = Material {
        sheen: Some(Sheen::new(Color::rgb8(200, 180, 255), 0.4)),
        ..Material::color(Color::rgb8(40, 30, 90), 0.0, 0.9)
    };
    let tinted_glass = Material {
        transmission: Some(Transmission {
            attenuation_color: Color::rgb8(120, 200, 255),
            attenuation_distance: 1.0,
            ..Transmission::glass(0.8)
        }),
        ..Material::color(Color::WHITE, 0.0, 0.05)
    };
    [car_paint, velvet, tinted_glass]
}
```

Rules that matter when you combine them:

- Transmissive meshes draw after the opaque scene and refract a blurred
  copy of it. Blended meshes and other glass behind them do not show
  through.
- Fur, outlines and x-ray tints apply to opaque and cut-out materials.
  Fur shells cast no shadow. `Fur::shells` runs from 1 to 64, and each
  shell is another instance of the mesh in the same draw call.
- `Outline::width` is in pixels, so the line keeps its width at any
  distance.
- Feature textures (clearcoat, sheen, specular, iridescence, anisotropy,
  fur) read the first texture coordinate set and are data. Sheen and
  specular textures carry colour in RGB and a factor in alpha; create them
  with `premultiplied` set so the upload leaves the colour alone.

### Stencil masks

To draw one material only where another has drawn (portals, cutaways,
windows into another room), set `Material::stencil`. The stencil buffer
starts each frame at zero, and games use references 0 to 127.

| Constructor | Effect |
|---|---|
| `Stencil::mark(r)` | Writes `r` wherever the material draws. |
| `Stencil::inside(r)` | Draws only where the buffer holds `r`. |
| `Stencil::outside(r)` | Draws only where the buffer does not hold `r`. |

For other tests and writes, set `Stencil { test, reference, write }` with a
`StencilTest` and a `StencilOp`. Materials that write the stencil draw
before those that do not, so the order of your draw calls does not matter.
A masked draw still passes the depth test, so it must stand in front of its
mask.

```rust,no_run
use yowie::gfx::scene::{Material, Mesh, Scene, Stencil};
use yowie::{Color, Mat4};

fn draw_portal(s: &mut Scene, frame: &Mesh, frame_at: Mat4, beyond: &Mesh, beyond_at: Mat4) {
    let window = Material {
        stencil: Stencil::mark(1),
        ..Material::color(Color::rgb8(25, 35, 55), 0.0, 0.3)
    };
    let other_world = Material {
        stencil: Stencil::inside(1),
        ..Material::color(Color::rgb8(255, 120, 40), 0.0, 0.3)
    };
    s.draw(frame, &window, frame_at);
    s.draw(beyond, &other_world, beyond_at);
}
```

### Decals

To project an image onto whatever lies inside a box (bullet holes, scorch
marks, road paint), make a `Decal` and call `Scene::draw_decal(&decal,
placement)`. The placement maps the cube from -0.5 to 0.5 onto the box. The
image projects down the box's Y axis, with X running across it and Z from
its bottom edge to its top, and fades on surfaces that do not face up the
box. A decal is lit like the surface under it.

| Field | Default | Meaning |
|---|---|---|
| `texture` | (required) | The colour image, sRGB; alpha is coverage. |
| `tint` | white | Multiplies the image; alpha scales the coverage. |
| `roughness` | `None` | Replaces the surface's roughness under the decal. |
| `normal_texture`, `normal_strength` | `None`, 1 | A data normal map that bumps the surface under the decal. |

To stick a decal on a surface the player hit, point the box's Y axis along
the hit normal:

```rust,no_run
use yowie::gfx::scene::{Decal, RayHit, Scene};
use yowie::{Mat4, Quat, Vec3};

fn scorch(s: &mut Scene, mark: &Decal, hit: &RayHit) {
    let place = Mat4::from_scale_rotation_translation(
        Vec3::new(1.0, 0.5, 1.0),
        Quat::from_rotation_arc(Vec3::Y, hit.normal),
        hit.point,
    );
    s.draw_decal(mark, place);
}
```

Decals cost no draw calls: every decal of a scene is one record, and every
fragment of a material with `receive_decals` on tests itself against each
decal's box. A scene draws at most `MAX_DECALS` (256) decals. Decal images
are copied into atlas cells of 256 by 256 texels, 64 cells in all; a frame
that uses more than 64 different images leaves the extra decals out.

## Lights and shadows

A scene has one directional light, the sun, and any number of point and
spot lights.

| Light | Call | Key fields |
|---|---|---|
| `DirectionalLight` | `Scene::set_sun` | `direction` (the way the light travels, so a noon sun points down), `color`, `intensity`, `shadows`, `shadow_distance`, `shadow_strength` |
| `PointLight` | `Scene::add_point_light` | `position`, `color`, `intensity`, `range`, `shadows` |
| `SpotLight` | `Scene::add_spot_light` | `position`, `direction`, `color`, `intensity`, `range`, `inner_angle`, `outer_angle`, `shadows` |

Light colours are sRGB and `intensity` multiplies them in linear light. A
sun is usually brighter than 1 (`DirectionalLight::default()` uses 3).
Point and spot light intensity is the light at the light's position; it
falls off as `(1 - (d / range)²)²` with distance `d`, reaching zero at
`range`. There is no inverse-square term, so pick `range` for how far the
light should reach. A spot light is full inside `inner_angle` and fades to
zero at `outer_angle`, both half-angles in radians from its axis.
`SpotLight::new` uses 20 and 30 degrees; `with_cone` changes them.

When the scene has no sky and no environment, `Scene::set_ambient` sets
the light that reaches everything evenly. The default is a dim blue-grey.

```rust,no_run
use yowie::gfx::scene::{PointLight, Scene, SpotLight};
use yowie::{Color, Vec3};

fn light_the_crypt(s: &mut Scene, torches: &[Vec3], t: f32) {
    s.set_ambient(Color::rgb8(20, 22, 30));
    for (i, at) in torches.iter().enumerate() {
        let flicker = 3.0 + 0.4 * (t * 9.0 + i as f32).sin();
        s.add_point_light(PointLight::new(*at, Color::rgb8(255, 150, 70), flicker, 6.0));
    }
    let lantern = SpotLight::new(Vec3::new(0.0, 3.0, 0.0), Vec3::NEG_Y, Color::WHITE, 8.0, 10.0)
        .with_cone(15f32.to_radians(), 25f32.to_radians())
        .with_shadows();
    s.add_spot_light(lantern);
}
```

Point and spot lights are clustered: the view is cut into a grid of 16 by
9 tiles and 24 depth slices, each light is listed in the cells its sphere
reaches, and each fragment loops over its own cell's lights. Hundreds of
small lights are affordable; a large `range` puts a light in more cells
and costs more. A scene keeps `MAX_LIGHTS` (1024) point and spot lights,
the first added first, and one cell lists at most 255.
`FrameStats::lights_dropped` counts lights refused for either limit.

### Shadows

To turn shadows on, call `DirectionalLight::with_shadows(distance)`, or set
`shadows: true` on a point or spot light (`with_shadows()`).

- The sun uses three cascaded maps of 2048 by 2048 texels covering
  `shadow_distance` world units from the camera. Keep the distance as
  small as the scene allows, because the cascades spread their texels over
  it. `shadow_strength` from 0 to 1 sets how dark shadows are.
- Each shadowed spot light renders one 1024 by 1024 map. The first
  `MAX_SPOT_SHADOWS` (4) spot lights that ask get one.
- Each shadowed point light renders six 512 by 512 cube faces. The first
  `MAX_POINT_SHADOWS` (4) point lights that ask get them. A shadowed point
  light is the most expensive light.

Every map lives in one 4096 by 5632 depth atlas (32-bit float, about
88 MiB), created the first time a scene asks for shadows and shared by every
scene after. Lookups filter a 3 by 3 block of comparisons. Each map renders
only the casters its frustum reaches, including casters outside the
camera's view, and `FrameStats::shadow_instances` counts them. Cut-out
materials cast cut-out shadows. Blended and transmissive materials and fur
shells cast none, and a surface shader does not change a shadow.

## Sky, environment and fog

### The procedural sky

To give a scene a sky, call `Scene::set_sky(Sky { .. })`. The sky draws
behind the scene and lights it: metals reflect it and matte surfaces take
its colour from above and from the ground.

| `Sky` field | Default | Meaning |
|---|---|---|
| `zenith`, `horizon`, `ground` | blue, pale blue, brown | The gradient above and below the horizon, sRGB. |
| `up` | +Y | Away from the ground. |
| `sun_size` | 0.009 | The angular radius of the sun's disc in radians; 0 hides it. |
| `sun_brightness` | 30 | The disc's brightness as a multiple of the sun light. |
| `stars` | 0 | The brightness of stars showing through thin air. |
| `vacuum` | 0 | 0 is full air, 1 a black sky with stars. |
| `atmosphere` | `None` | Computed scattering in place of the gradient above the horizon. |

`Atmosphere::new(height)` computes the sky from single scattering of
sunlight by air and haze: blue overhead at noon, red at the horizon at
dusk. `height` is the depth of the air in world units and sets the scale
of everything else. A scene a hundred units across under a height of 60
looks through a lot of air and washes out; under 3000 it keeps crisp
distance. `at_altitude(h)` thins the air as the camera climbs, to black at
`height`, and `with_aerial(scale)` multiplies the distance through the air
to surfaces, so a small scene can fade into the sky like distant
mountains. `Sky::atmospheric(atmosphere, ground)` builds a sky around one.

The renderer bakes the sky into small lighting cubes (64 texels a face)
when the sky or the sun direction changes, and keeps up to four baked
skies. A sun that moves every frame rebakes every frame; the sun direction
is compared in steps of 1/64, so a slow sun rebakes less often.

### Image-based lighting

To light a scene from a photograph of surroundings, decode an
equirectangular panorama into an `Environment` and set it with
`Scene::set_environment`. An environment takes precedence over the sky for
lighting and background.

```rust,no_run
use yowie::gfx::scene::{Environment, EnvironmentOptions};
use yowie::{Context, GameResult};

fn studio_light(ctx: &mut Context) -> GameResult<Environment> {
    let bytes = ctx.assets.vfs().read("env/studio.hdr")?;
    let options = EnvironmentOptions {
        size: 256,
        intensity: 1.0,
    };
    Ok(Environment::from_bytes(&ctx.gfx, &bytes, options)?)
}
```

`Environment::from_bytes` reads OpenEXR, Radiance HDR, PNG and JPEG.
`Panorama::decode` and `Panorama::from_pixels` give you the image first if
you want to change it, and `Environment::new` bakes a `Panorama`. The top
row of a panorama is straight up and its centre column faces -Z. The bake
blocks while the GPU builds a radiance cube (`size` texels a face, also
what the background shows), a prefiltered specular cube and an irradiance
cube, so make environments while loading. An 8-bit PNG or JPEG cannot
carry bright light; raise `intensity` to make up some of it.

`Scene::set_background(false)` keeps the sky or environment lighting the
scene but shows the clear colour behind it.

### Fog

To add fog, call `Scene::set_fog`. Fog colours surfaces by their distance
from the camera; the sky is not fogged.

| Constructor | Fog |
|---|---|
| `Fog::linear(color, start, end)` | Ramps from `start` to full at `end` world units. |
| `Fog::exponential(color, density)` | Thickens with `density` per world unit. |
| `Fog::valley(color, density, height, falloff)` | Exponential fog full at and below world height `height`, thinning by a factor of e every `1 / falloff` units above it. A camera on a hill looks down into the mist and through clear air over it. |

Linear and exponential fog can be combined in one `Fog`; the denser wins.

```rust,no_run
use yowie::gfx::scene::{Atmosphere, DirectionalLight, Fog, Scene, Sky};
use yowie::{Color, Vec3};

fn evening(s: &mut Scene) {
    let sun = DirectionalLight::new(Vec3::new(-0.9, -0.25, 0.3), Color::rgb8(255, 210, 160), 3.2);
    s.set_sun(sun.with_shadows(120.0));
    let air = Atmosphere::new(3000.0).with_aerial(4.0);
    s.set_sky(Sky::atmospheric(air, Color::rgb8(70, 76, 60)));
    s.set_fog(Fog::valley(Color::rgb8(196, 204, 214), 0.03, 0.5, 0.5));
}
```

## Reflection probes and light probes

The sky and environment light every point the same way, so an object
inside a red room still reflects a blue sky. Probes capture the scene
itself.

- A `ReflectionProbe` captures a cube map from a point. Shiny surfaces
  inside its `bounds` reflect the capture instead of the environment.
  With `box_projection` on (the default) the reflection is projected onto
  the box, so a room's walls reflect where they stand. Smaller boxes take
  precedence where probes overlap, and `blend_distance` fades a probe out
  past its box. A probe lights specular reflections only.
- A `LightProbeGrid` captures a small cube at each cell of a 3D grid and
  keeps it as spherical harmonics. Inside the grid, and half a cell past
  its outer cells, it replaces the diffuse light of the environment or
  ambient colour, so a matte ball takes the colour of the wall beside it.
  Place cells in open space: a cell inside a wall captures darkness and
  dims its neighbours.

To bake them, call `Graphics::bake_reflection_probe` and
`Graphics::bake_light_probes` with a closure that records the scene to
capture, usually the same function you draw with. Bake the grid first, so
the reflection probe's capture shows walls lit by it:

```rust,no_run
use yowie::gfx::scene::{
    Aabb, DirectionalLight, LightProbeGrid, ReflectionProbe, Scene, StaticGroup,
};
use yowie::{Color, Context, GameResult, UVec3, Vec3};

struct Hall {
    room: StaticGroup,
    probe: ReflectionProbe,
    grid: LightProbeGrid,
}

fn draw_hall(s: &mut Scene, room: &StaticGroup) {
    s.set_sun(DirectionalLight::new(Vec3::new(-0.3, -1.0, -0.2), Color::WHITE, 1.0));
    s.set_ambient(Color::rgb(0.12, 0.13, 0.15));
    s.draw_static(room);
}

fn bake_hall(ctx: &mut Context, room: StaticGroup) -> GameResult<Hall> {
    let bounds = Aabb::new(Vec3::new(-10.0, 0.0, -10.0), Vec3::new(10.0, 6.0, 10.0));
    let probe = ReflectionProbe::new(Vec3::new(0.0, 1.8, 0.0), bounds);
    let mut grid = LightProbeGrid::new(
        Vec3::new(-8.0, 1.0, -8.0),
        Vec3::new(4.0, 2.0, 4.0),
        UVec3::new(5, 3, 5),
    );
    ctx.gfx.bake_light_probes(&mut grid, |s| draw_hall(s, &room))?;
    ctx.gfx.bake_reflection_probe(&probe, |s| {
        s.set_light_probes(Some(&grid));
        draw_hall(s, &room);
    });
    Ok(Hall { room, probe, grid })
}

fn draw_lit_hall(s: &mut Scene, hall: &Hall) {
    s.add_reflection_probe(&hall.probe);
    s.set_light_probes(Some(&hall.grid));
    draw_hall(s, &hall.room);
}
```

Both bakes submit their own GPU work at once and leave the frame being
drawn alone. A light probe grid bake blocks until the GPU finishes; a grid
of a few hundred cells takes a fraction of a second. To skip the bake on
the next run, save `LightProbeGrid::to_bytes` and load it with
`LightProbeGrid::from_bytes`. `LightProbeGrid::irradiance(position,
normal)` returns the baked light at a point, to light sprites or particles
to match.

A reflection probe can also capture from the scene that shows it. A probe
with no capture is captured from the first scene that adds it.
`ReflectionProbe::request_bake` asks for a new capture from the next scene,
and `with_update(ProbeUpdate::EveryFrames(n))` recaptures every `n` frames
for rooms whose contents move. Each capture renders the scene six times;
`FrameStats::probe_faces` counts the faces.

Captures leave out terrain, water, billboards, impostors, GPU particles
and any draw you pass to `Scene::hide_in_probes`, such as the shiny ball
that would otherwise reflect itself. Captures composite blended surfaces
sorted, and glass in a capture reflects without refracting.
`Scene::draw_light_probes(&grid, &sphere, radius)` draws a white sphere at
each cell, to see where the cells are and what light each holds.

A scene shows up to `MAX_REFLECTION_PROBES` (8) probes, or one on GPUs
without cube map arrays. A grid has at most `MAX_LIGHT_PROBES` (65,536)
cells.

## Large scenes

### Static groups

Level geometry that never moves costs less as a `StaticGroup`. Add the
meshes once with `StaticGroup::add(mesh, material, placement)` and draw the
group every frame with `Scene::draw_static`. The first time a scene draws a
group, the renderer orders its items in a bounding volume hierarchy and
merges those sharing a mesh and material into instanced batches. After
that, a compute pass culls the items on the GPU against the view and
against a depth pyramid of what nearer geometry hides, and the batches
replay from render bundles. The processor cost per item is close to zero.

```rust,no_run
use yowie::gfx::scene::{Material, Mesh, StaticGroup};
use yowie::{Color, Vec3};

fn build_city(block: &Mesh) -> StaticGroup {
    let mut city = StaticGroup::new();
    let stone = Material::color(Color::rgb8(150, 140, 130), 0.0, 0.9);
    for i in 0..1000 {
        let at = Vec3::new((i % 40) as f32 * 3.0, 0.0, (i / 40) as f32 * 3.0);
        city.add(block, &stone, at);
    }
    city
}
```

Editing a group (`add`, `set_placement`, `clear`) rebuilds all of that on
the next draw, so keep moving things out of it. Items whose material
blends, refracts, uses the stencil, or has fur, an outline, an x-ray tint
or a surface shader draw as ordinary draws. On GPUs without compute
shaders or indirect draws every item draws as an ordinary draw, culled on
the processor. Occlusion culling applies to static groups only; ordinary
draws are culled against the view alone.

### Levels of detail

To swap a detailed mesh for coarser ones as it shrinks on screen, build a
`Lod` from finest to coarsest. Each level takes over below a screen
coverage: the object's bounding sphere's projected diameter as a fraction
of the view's height, so 1 fills the view top to bottom. Keep one
`LodState` per object; it remembers the last level so that `hysteresis`
(0.15 by default, a fraction of the threshold) stops an object near a
boundary from flickering.

```rust,no_run
use yowie::gfx::scene::{Lod, LodState, Material, Mesh, Scene};
use yowie::Vec3;

struct Rock {
    at: Vec3,
    lod: LodState,
}

fn rock_levels(fine: Mesh, coarse: Mesh, tiny: Mesh) -> Lod<Mesh> {
    Lod::new(fine).then(coarse, 0.15).then(tiny, 0.04).cull_below(0.01)
}

fn draw_rocks(s: &mut Scene, levels: &Lod<Mesh>, granite: &Material, rocks: &mut [Rock]) {
    for rock in rocks {
        s.draw_lod(levels, granite, rock.at, &mut rock.lod);
    }
}
```

`Scene::draw_model_lod` does the same for `Lod<ModelInstance>`, and
`Scene::screen_coverage(center, radius)` gives the coverage for your own
choices. `FrameStats::lod_levels` counts draws per level.

### Terrain

A `Terrain` is a chunked heightfield. It keeps its heights on the
processor for queries and on the GPU for drawing; each chunk drops detail
with distance, and the whole terrain draws in one call per level. Build it
from a `HeightMap` (`from_fn`, `flat`, `new` from raw heights, or
`from_image` from an image's brightness) and `TerrainOptions`:

| Field | Default | Meaning |
|---|---|---|
| `cell` | 1 | World units between samples. |
| `center` | origin | The middle of the grid; its `y` is added to every height. |
| `chunk_size` | 32 | Samples across a chunk, a power of two. `cols - 1` and `rows - 1` must be whole multiples of it. |
| `levels` | 4 | Resolutions per chunk, each halving the one before (at most 8). |
| `lod_distance` | 0 (two chunk widths) | How far the finest level reaches; each level after covers twice the distance. |
| `hysteresis` | 0.1 | How far past a level's distance a chunk moves before it changes level. |
| `layers` | sand, grass, rock, snow | Four `TerrainLayer`s the splat map blends: a tiling texture, a tint, `scale` (world units per repeat) and roughness. |
| `rules` | `Some(SplatRules::default())` | Paints the splat map from height and slope, and repaints edited ground. `None` starts with the first layer everywhere for a map you paint. |
| `triplanar` | true | Projects the layers from the sides on steep ground, so cliffs show no stretched texels. |

```rust,no_run
use yowie::gfx::Graphics;
use yowie::gfx::scene::{Brush, HeightMap, Ray, Terrain, TerrainOptions, Water, WaterOptions};
use yowie::{GameResult, Vec2};

fn build_island(gfx: &Graphics) -> GameResult<(Terrain, Water)> {
    // 257 samples is eight 32-sample chunks plus one.
    let map = HeightMap::from_fn(257, 257, |x, z| {
        let (dx, dz) = (x as f32 - 128.0, z as f32 - 128.0);
        12.0 - (dx * dx + dz * dz).sqrt() * 0.12
    });
    let options = TerrainOptions {
        cell: 2.0,
        ..TerrainOptions::default()
    };
    let terrain = Terrain::new(gfx, map, options)?;
    let water = Water::over_terrain(
        gfx,
        &terrain,
        WaterOptions {
            size: Vec2::splat(600.0),
            ..WaterOptions::default()
        },
    );
    Ok((terrain, water))
}

fn dig(gfx: &Graphics, terrain: &Terrain, ray: &Ray) {
    if let Some(hit) = terrain.raycast(ray, 2000.0) {
        terrain.apply_brush(gfx, &Brush::dig(hit.point, 4.0, 2.0));
    }
}
```

To draw a terrain, call `Scene::draw_terrain`, or `draw_terrain_with` and
`TerrainDrawOptions` to stop it casting shadows or to tint each chunk by
its level. The frustum culls chunks; `FrameStats::terrain_chunks` and
`terrain_chunks_culled` count them.

To query the ground, call `Terrain::height(x, z)`, `Terrain::normal(x, z)`
and `Terrain::raycast(ray, max_distance)`. `height` follows the surface the
finest level draws, which splits each cell along the same diagonal as the
physics heightfields, so `yowie::phys::Shape3D::grid_heightfield` built
from `Terrain::height_map` collides with the ground the player sees (see
[physics](physics.md)).

To edit the ground, call `Terrain::apply_brush` with a `Brush`
(`BrushMode::Raise`, `Lower`, `Flatten(height)` or `Smooth`;
`Brush::dig` and `Brush::raise` are shortcuts), or `Terrain::edit` to
change a rectangle of samples yourself. Either rebuilds only the chunks,
normals and splat texels it touched. `set_splat` replaces the splat map
with an image, `set_splat_rules` repaints it from rules, and `set_layer`
replaces one ground layer.

### Water

A `Water` is a level surface over a rectangle, coloured by its depth,
reflecting the sky and sun off moving waves, with foam along the shore.
`Water::over_terrain` reads its depth from a terrain's heights, so edits
show at once; `Water::new` assumes flat ground `floor_depth` below. Draw
it with `Scene::draw_water`; it blends over the scene after the opaque
and blended meshes, and joins order-independent transparency when the
scene uses it. `set_options` changes it from the next frame.

| `WaterOptions` field | Default | Meaning |
|---|---|---|
| `center`, `size` | origin, 100 by 100 | The middle of the surface (its `y` is the water level) and its extent. |
| `shallow_color`, `deep_color`, `depth_range` | teal, navy, 4 | Colours, and the depth at which the water is fully deep. |
| `opacity`, `roughness` | 0.96, 0.06 | How opaque deep water is, and how sharp reflections are. |
| `waves`, `wave_length`, `wave_speed` | 0.35, 6, 1.2 | Wave tilt, the largest wavelength in world units, and speed in units per second. |
| `foam` | 0.35 | Shore foam width in world units; 0 for none. |

### Billboards and impostors

A `Billboard` is a textured quad that turns to face the camera.
`Facing::Camera` turns it fully, for smoke and markers; `Facing::Upright`
turns it about the up axis only, for trees and characters.
`Billboard::new(position, size)` centres it and `Billboard::upright`
stands it on its bottom edge. Billboards share a `BillboardStyle` (texture,
optional normal map, alpha mode, lit or not, shadows), and every billboard
of one style in a scene is one instanced draw call.

An `Impostor` is a model baked into an atlas of views around it and drawn
far away as one quad that shows the nearest view. It stores base colour
and normals, so the scene's lights shade it each frame. Bake it once with
`Impostor::bake(&mut gfx, &instance, ImpostorOptions::default())` (12
views of 128 pixels by default, at most `MAX_IMPOSTOR_VIEWS` (64)), then
set `distance` and `fade` and call `Scene::draw_model_or_impostor`. Near
the camera it draws the model; far away, the impostor; within the `fade`
band both draw with complementary dither patterns.

```rust,no_run
use yowie::gfx::scene::{Billboard, BillboardStyle, Impostor, ModelInstance, Scene};
use yowie::{Mat4, Vec2, Vec3};

fn draw_forest(
    s: &mut Scene,
    bush: &BillboardStyle,
    bushes: &[Vec3],
    pine: &ModelInstance,
    pine_far: &Impostor,
    pines: &[Mat4],
) {
    let quads: Vec<Billboard> = bushes
        .iter()
        .map(|at| Billboard::upright(*at, Vec2::new(2.0, 1.5)))
        .collect();
    s.draw_billboards(bush, &quads);
    for at in pines {
        s.draw_model_or_impostor(pine, pine_far, *at);
    }
}
```

`FrameStats::billboards` and `impostors` count them. Skinned parts of a
model do not dither; they draw whole until the far edge of the fade band.

### Labels in the world

To anchor text to a 3D point, call `Scene::label(&font, position, text,
&LabelStyle)`. The label is drawn in the 2D pass, centred above where the
point appears, at a constant size, with an optional backing plate;
`LabelStyle::fade` fades it out between two distances. Labels behind the
camera are skipped. Labels are not hidden by geometry in front of the
point.

## Picking and debug lines

To find what the mouse is over, cast a ray from the camera and test the
scene's draws. `Scene::pick(&ray)` returns the nearest draw recorded so far
that the ray hits, testing each draw's bounds and then its triangles, as a
`ScenePick` with the `DrawId` and a `RayHit` (distance, point, normal and
triangle index). Pick after drawing the things you want to hit:

```rust,no_run
use yowie::gfx::scene::{DrawId, Material, Mesh, Scene};
use yowie::{Vec2, Vec3};

fn draw_units(
    s: &mut Scene,
    body: &Mesh,
    paint: &Material,
    units: &[Vec3],
    click: Option<Vec2>,
) -> Option<usize> {
    let ids: Vec<DrawId> = units.iter().map(|at| s.draw(body, paint, *at)).collect();
    let ray = s.screen_ray(click?);
    let hit = s.pick(&ray)?;
    ids.iter().position(|id| *id == hit.draw)
}
```

Outside a frame, `yowie::gfx::scene::pick(&ray, items)` tests your own list
of `(key, &Mesh, Mat4)`, and `Mesh::intersect` tests one mesh. Skinned and
morphed meshes are tested in their rest shape. `Scene::pick` tests every
draw in turn. The `physics3d` example picks with a physics ray cast
instead.

To draw lines for gizmos and debugging, call `Scene::line(a, b, color)`,
which geometry in front hides, or `Scene::line_on_top`, which draws over
everything. `yowie::SceneDebugDraw::new(s)` draws a physics world's debug
view through these lines; see [physics](physics.md).

## Post-processing

Each scene's HDR image is finished by a post chain on its way to the
output. To configure it, call `Scene::set_post(PostSettings { .. })` every
frame, or change one field through `Scene::post_mut()`. The default is
exposure 1 and the ACES tone curve, and every effect is off until its
strength or switch is set. An effect that is off costs no pass.

```rust,no_run
use yowie::gfx::scene::{PostSettings, Scene, Tonemap};

fn cinematic(s: &mut Scene) {
    s.set_post(PostSettings {
        tonemap: Tonemap::Agx,
        bloom: 0.3,
        ambient_occlusion: 0.7,
        temporal_aa: true,
        vignette: 0.3,
        ..PostSettings::default()
    });
}
```

`cargo run --example lighting` puts every field on a slider. Its
`--effects` flag takes a comma-separated list for screenshots: `taa`,
`msaa`, `dof`, `motion`, `rays`, `ssr`, `lens`, `grade`, `auto` and
`occlusion` (show the occlusion buffer). Bloom, ambient occlusion and FXAA
start on.

### Exposure and tone

| Field | Default | Meaning |
|---|---|---|
| `exposure` | 1 | Multiplies the light before tone mapping. `Scene::set_exposure` sets it. |
| `auto_exposure` | false | Sets the exposure from the scene's average brightness each frame. Needs compute shaders; without them the manual exposure stays. |
| `exposure_compensation` | 0 | Stops added to the automatic exposure: +1 doubles it. |
| `adaptation_speed` | 1.5 | How fast the automatic exposure follows the scene, per second. |
| `tonemap` | `Tonemap::Aces` | The curve from HDR to the output. `Scene::set_tonemap` sets it. |

| `Tonemap` | Look | Linear mid-grey (0.18) lands at |
|---|---|---|
| `None` | Clamps; bright areas clip. | 0.18 |
| `Aces` | Contrasty, saturated highlights. | 0.106 |
| `Agx` | Gentler; highlights desaturate towards white. | 0.215 |
| `Reinhard` | Soft; never reaches white. | 0.153 |
| `Neutral` | Keeps base colours as authored up to 0.76 and compresses only the highlights. | 0.14 |

### Colour grade

| Field | Default | Meaning |
|---|---|---|
| `temperature`, `tint` | 0, 0 | White balance from -1 to 1: cool to warm, green to magenta. |
| `saturation`, `contrast` | 1, 1 | 0 saturation is grey; contrast spreads colours from mid-grey. |
| `lift`, `gamma`, `gain` | 0, 1, 1 per channel | Shadows, mid-tones and highlights. |
| `lut`, `lut_strength` | `None`, 1 | A colour lookup table applied after the rest of the grade. |

A `Lut` is a strip image n squared pixels wide and n high. To make one,
save `Lut::neutral_image(16)`, paste it into a screenshot, grade the
screenshot in an image editor, crop the strip out and load it with
`Lut::from_bytes` or `Lut::from_image`. The lookup runs on display colours
after tone mapping.

```rust,no_run
use yowie::gfx::scene::Lut;
use yowie::{Context, GameResult};

fn load_grade(ctx: &mut Context) -> GameResult<Lut> {
    let bytes = ctx.assets.vfs().read("grades/dusk.png")?;
    Ok(Lut::from_bytes(&ctx.gfx, &bytes)?)
}
```

### Bloom, occlusion and reflections

| Field | Default | Meaning |
|---|---|---|
| `bloom` | 0 | How much light above the threshold spreads into a glow. |
| `bloom_threshold` | 1 | Linear brightness after exposure where bloom starts, with a soft knee. |
| `ambient_occlusion` | 0 | Screen-space ambient occlusion from 0 to 1. Darkens ambient and image-based light in creases; direct light is left alone. |
| `occlusion_radius` | 1 | How far occlusion reaches, in world units. |
| `show_occlusion` | false | Shows the occlusion buffer instead of the picture. |
| `reflections` | 0 | Screen-space reflections from 0 to 1. Smooth opaque surfaces reflect what the screen shows; elsewhere they keep their environment or probe reflection. |
| `reflection_roughness` | 0.35 | The roughness above which a surface stops reflecting the screen. |
| `reflection_distance` | 30 | How far a reflection ray travels, in world units. |
| `reflection_steps` | 128 | The most steps a ray takes through the depth pyramid. |

### Anti-aliasing

| Field | Default | Meaning |
|---|---|---|
| `samples` | 1 | Multisampling of the scene pass: 1, 2, 4 or 8, lowered to the largest count the GPU supports. |
| `fxaa` | false | Fast approximate anti-aliasing of the finished picture. |
| `temporal_aa` | false | Temporal anti-aliasing: the projection moves by a fraction of a pixel each frame and frames are averaged, reprojected by their motion. |
| `temporal_blend` | 0.1 | How much of the new frame enters the average, 0.02 to 1. Lower is steadier and softer. |

Temporal anti-aliasing and motion blur reproject by the camera's motion. A
mesh that moves on its own needs `Scene::draw_moved` with last frame's
placement, or it smears. When the camera jumps (a cut, a teleport), call
`Scene::cut` so earlier frames are not blended into the new view.

### Camera and lens

| Field | Default | Meaning |
|---|---|---|
| `focus_distance` | 0 (off) | Distance in focus, in world units. |
| `focus_range` | 0 (a quarter of the focus distance) | How far either side stays sharp, and how far the blur takes to reach full size. |
| `bokeh_radius`, `bokeh_samples` | 12, 16 | The widest blur in pixels of a 1080-pixel-high picture, and taps in its disc. |
| `motion_blur`, `motion_samples` | 0, 8 | Smear along each pixel's motion, 0 to 1 of a frame's motion, and taps along it. |
| `god_rays` | 0 | Strength of light shafts from the sun past what stands in front of the sky. Skipped while the sun is behind the camera or the scene has no sun. |
| `god_ray_decay`, `god_ray_density`, `god_ray_samples` | 0.96, 0.6, 32 | How fast a shaft fades, how far towards the sun each pixel looks, and steps. |
| `vignette` | 0 | Darkens the corners. |
| `aberration` | 0 | Splits red and blue towards the edges; 1 is about three pixels at the edge of a 1080-pixel-wide picture. |
| `distortion` | 0 | Barrel (above zero) or pincushion (below). |
| `ghosts` | 0 | Mirrored copies of bright areas through the centre. Uses the bloom images. |
| `grain` | 0 | Film grain that changes every frame; 0.05 is subtle. |

### What each effect costs

The chain runs only the steps the settings turn on. With the defaults it
is one full-screen composite pass (exposure, tone curve, grade and lens
effects together). The grade, the lookup table, vignette, aberration,
distortion and grain add no pass; they are part of the composite.

| Effect | Extra GPU work |
|---|---|
| Any effect that reads depth (occlusion, reflections, god rays, temporal anti-aliasing, motion blur, depth of field) | One depth copy pass, shared. |
| Ambient occlusion | The scene pass writes an extra 16-bit surface image (normal, roughness); one full-size occlusion pass, two blur passes and one apply pass. |
| Reflections | The surface image plus a specular image; a depth pyramid of up to eight levels; one full-size trace pass and the apply pass. |
| Bloom (and ghosts) | A prefilter pass and a down and up chain over up to six levels starting at half size. |
| Multisampling | The scene's colour, depth and extra images at `samples` per pixel, resolved at the end of the pass. Memory and bandwidth grow with the count. |
| FXAA | One full-size pass after the composite. |
| Temporal anti-aliasing | One full-size pass and two history images; a velocity pass when the scene has `draw_moved` draws. |
| Motion blur | One full-size pass, plus the velocity pass. |
| Depth of field | A half-size gather and a full-size combine. |
| God rays | One half-size pass. |
| Auto exposure | Two small compute passes building a histogram. |

The scene pass itself has costs that do not show in the chain: blended
surfaces under order-independent transparency add two images and a pass;
glass copies the opaque scene with a blurred mip chain each frame it draws;
outlines, x-ray tints and fur add draws; each shadowed point light renders
six maps.

### Post-processing a whole frame

`Graphics::set_frame_post(PostSettings { .. })` post-processes everything
on an output below the interface layers, 2D included. A 2D frame is not
exposed or tone mapped, so with no effect on it keeps its colours. Bloom,
the grade, the lens effects and FXAA work on it; the effects that need a
scene's depth stay in the scene's own settings. Call it every frame while
it should apply.

## Render textures

To render a scene into an image (a minimap, a mirror, a portrait, a 3D
model on a 2D menu), make a `yowie::gfx::RenderTexture` and record the
scene inside `Graphics::draw_to`. Render textures render before the screen,
so you can draw the result in the same frame.

```rust,no_run
use yowie::gfx::scene::{Camera3D, Material, Mesh};
use yowie::gfx::{Graphics, RenderTexture};
use yowie::{Color, Rect, Vec3};

fn draw_minimap(gfx: &mut Graphics, map: &RenderTexture, ground: &Mesh, flat: &Material, player: Vec3) {
    let above = Camera3D {
        up: Vec3::NEG_Z,
        ..Camera3D::orthographic(player + Vec3::Y * 50.0, player, 30.0)
    };
    gfx.draw_to(map, Some(Color::BLACK), |g| {
        g.scene(&above, |s| {
            s.set_sky(None);
            s.draw(ground, flat, Vec3::ZERO);
        });
    });
    let view = gfx.view_size();
    let corner = Rect::new(view.x - 212.0, 12.0, 200.0, 200.0);
    gfx.draw_texture_rect(map.texture(), corner, Color::WHITE);
}
```

A scene in a render texture runs the same passes and post chain as one on
the screen, and costs as much per pixel. The `solar` example draws its
minimap this way.

## GPU particles and custom draws

`yowie::particle` effects on the GPU backend draw into a 3D scene with
`Particles::draw_3d(&mut scene)`, as camera-facing quads on a plane you
set with `GpuSystem::set_plane`. They draw after the blended meshes and
test against the scene's depth. The CPU backend draws only in 2D.

A renderer extension that records its own GPU commands implements
`yowie::gfx::GpuDraw` and queues itself with `Scene::draw_gpu`. It draws
in the scene's HDR pass with the scene's frame data bound; see
[shaders](shaders.md#custom-gpu-draws).

## Performance

`Graphics::stats()` reports the last frame as a `FrameStats`, and F3
shows a frame-timing overlay.

| Field | Meaning |
|---|---|
| `draws_3d` | Mesh draws recorded; each instance counts once. |
| `draw_calls_3d` | 3D draw calls after instancing, shadow maps included. Far fewer calls than draws means batching works. |
| `culled_3d` | Draws outside the camera's view. |
| `occluded_3d` | Static group instances hidden behind nearer geometry. The GPU counts them, so the number is a frame or two late. |
| `bundle_draws` | Draw calls replayed from static group render bundles. |
| `lights`, `lights_dropped` | Point and spot lights kept after culling, and lights or cluster entries refused. |
| `shadow_instances` | Instances drawn into shadow maps. |
| `terrain_chunks`, `terrain_chunks_culled` | Terrain chunks drawn and culled. |
| `billboards`, `impostors` | Billboards and impostors drawn. |
| `lod_levels` | Draws picked per level of detail; the last entry counts levels 3 and up. |
| `probe_faces` | Cube faces rendered for reflection probe captures. |
| `gpu` | GPU time per group of passes (shadows, opaque, blended, post, 2D) in microseconds, while `Graphics::set_gpu_timing(true)` is on and the GPU supports timestamps. The figures are two or three frames old. |

```rust,no_run
use yowie::gfx::{Font, Graphics, TextStyle};
use yowie::vec2;

fn draw_stats(gfx: &mut Graphics, font: &Font) {
    let st = gfx.stats();
    let text = format!(
        "{} draws in {} calls, {} culled, {} hidden",
        st.draws_3d, st.draw_calls_3d, st.culled_3d, st.occluded_3d
    );
    gfx.draw_text(font, &text, vec2(10.0, 10.0), &TextStyle::new(14.0));
}
```

On Apple GPUs, which cannot write timestamps inside a pass, GPU timing
splits the scene pass in two, which costs a little bandwidth; leave it off
when you are not measuring.

Where the time goes, in the order to look:

- **Draw calls.** Share meshes and materials so draws merge, use
  `draw_instanced` for crowds, and put level geometry in a `StaticGroup`.
  Materials with different surface shader uniform values do not merge.
- **Shadows.** A smaller `shadow_distance` gives sharper sun shadows at
  the same cost. Every shadowed spot light is one more map and every
  shadowed point light six; give shadows to the few lights that matter.
- **Lights.** Each fragment pays for the lights in its cluster cell. Keep
  `range` as small as the look allows.
- **Decals.** Each fragment of a decal-receiving material tests every
  decal in the scene. Turn `receive_decals` off on materials that never
  show one.
- **Post-processing.** Ambient occlusion, reflections and multisampling
  cost the most; see [What each effect costs](#what-each-effect-costs).
- **Render textures and probe captures.** Each is another full scene
  render.

## Limits

| Limit | Value |
|---|---|
| Point and spot lights per scene | 1024 (`MAX_LIGHTS`); 255 per cluster cell |
| Directional lights per scene | 1 |
| Sun shadow cascades | 3, at 2048 texels |
| Shadowed spot lights | 4 (`MAX_SPOT_SHADOWS`), at 1024 texels |
| Shadowed point lights | 4 (`MAX_POINT_SHADOWS`), six faces at 512 texels |
| Decals per scene | 256 (`MAX_DECALS`); 64 different images per frame |
| Reflection probes per scene | 8 (`MAX_REFLECTION_PROBES`); 1 without cube map arrays |
| Light probe grid cells | 65,536 (`MAX_LIGHT_PROBES`) |
| Morph targets blended per draw | 8 (`MAX_MORPH_WEIGHTS`), the largest weights kept |
| Joints per vertex | 4 |
| Impostor views | 64 (`MAX_IMPOSTOR_VIEWS`) |
| Terrain levels | 8 |
| Fur shells | 64 |
| Stencil references | 0 to 127 |
| Multisampling | up to 8 samples, as the GPU allows |
