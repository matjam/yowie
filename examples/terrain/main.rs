//! A valley at the end of the day: chunked terrain whose chunks drop
//! detail with distance, painted sand, grass, rock and snow by height and
//! slope; a lake coloured by its depth; pines drawn as models near the
//! camera and as baked impostors far away; billboard trees; rocks at
//! three levels of detail; two campfires and a sweeping searchlight; an
//! atmospheric sky with aerial perspective and mist in the valley; and
//! place names anchored in the world.
//!
//! Left click digs a crater where the pointer meets the ground; the
//! chunks under it rebuild. Right drag turns the camera, W, A, S and D
//! fly, Q and E sink and climb, Shift flies faster. The window on the
//! right turns features on and off. Escape quits.

mod panel;
mod props;
mod world;

use yowie::gfx::scene::{
    Atmosphere, Billboard, BillboardStyle, Brush, BrushMode, Camera3D, DirectionalLight, Fog,
    Impostor, ImpostorOptions, LabelStyle, Lod, LodState, Material, Mesh, ModelInstance,
    Projection, Scene, Sky, Terrain, TerrainDrawOptions, TerrainOptions, Water, WaterOptions,
};
use yowie::gfx::{Font, TextStyle, Texture, TextureOptions};
use yowie::input::{Key, MouseButton};
use yowie::rng::Rng;
use yowie::{
    Color, Config, Context, FlyCamera, Game, GameResult, Mat4, Quat, Rect, Vec2, Vec3, vec2, vec3,
};

use panel::Settings;
use props::{FireParts, Tower};
use world::{LAKE, LAKE_RADIUS, WATER};

const SEED: u64 = 2024;
/// Where the campfires burn, world X and Z.
const CAMPS: [Vec2; 2] = [Vec2::new(-24.0, 112.0), Vec2::new(-62.0, 64.0)];
/// Where the searchlight tower stands.
const TOWER: Vec2 = Vec2::new(12.0, 126.0);

/// A pine, rock or tree standing on the ground.
struct Spot {
    at: Vec2,
    yaw: f32,
    scale: f32,
}

struct Valley {
    fly: FlyCamera,
    terrain: Terrain,
    water: Water,
    pine: ModelInstance,
    pine_far: Impostor,
    pines: Vec<Spot>,
    trees: Vec<Spot>,
    tree_style: BillboardStyle,
    rocks: Lod<Mesh>,
    rock_material: Material,
    rock_spots: Vec<(Spot, LodState)>,
    fire: FireParts,
    tower: Tower,
    summit: Vec3,
    font: Font,
    settings: Settings,
    panel: Rect,
}

/// Up to `count` spots where `fits(x, z)` holds, from `tries` random
/// candidates over the terrain.
fn scatter(
    rng: &mut Rng,
    count: usize,
    tries: usize,
    fits: impl Fn(f32, f32) -> bool,
) -> Vec<Spot> {
    let half = (world::SAMPLES - 1) as f32 * world::CELL * 0.5 - 4.0;
    let mut out = Vec::new();
    for _ in 0..tries {
        if out.len() == count {
            break;
        }
        let (x, z) = (rng.between(-half, half), rng.between(-half, half));
        if fits(x, z) {
            out.push(Spot {
                at: vec2(x, z),
                yaw: rng.between(0.0, std::f32::consts::TAU),
                scale: rng.between(0.75, 1.3),
            });
        }
    }
    out
}

impl Valley {
    fn on_ground(&self, at: Vec2, sink: f32) -> Vec3 {
        vec3(at.x, self.terrain.height(at.x, at.y) - sink, at.y)
    }

    fn sun(&self) -> DirectionalLight {
        let e = self.settings.sun.to_radians();
        let to_sun = vec3(-0.92 * e.cos(), e.sin(), 0.38 * e.cos()).normalize();
        let warmth = (self.settings.sun / 40.0).clamp(0.0, 1.0);
        let color = Color::rgb(1.0, 0.78 + 0.2 * warmth, 0.58 + 0.38 * warmth);
        DirectionalLight::new(-to_sun, color, 3.2).with_shadows(160.0)
    }

    fn draw_scene(&mut self, s: &mut Scene, t: f32) {
        let set = self.settings;
        s.set_sun(self.sun());
        let air = Atmosphere::new(3000.0).with_aerial(set.aerial);
        s.set_sky(Sky::atmospheric(air, Color::rgb8(70, 76, 60)));
        if set.fog {
            s.set_fog(Fog::valley(
                Color::rgb8(196, 204, 214),
                0.03,
                WATER + 0.5,
                0.5,
            ));
        }
        let options = TerrainDrawOptions {
            show_levels: set.show_levels,
            ..TerrainDrawOptions::default()
        };
        s.draw_terrain_with(&self.terrain, options);
        if set.water {
            s.draw_water(&self.water);
        }
        for p in &self.pines {
            let place = Mat4::from_scale_rotation_translation(
                Vec3::splat(p.scale),
                Quat::from_rotation_y(p.yaw),
                self.on_ground(p.at, 0.2),
            );
            if set.impostors {
                s.draw_model_or_impostor(&self.pine, &self.pine_far, place);
            } else {
                s.draw_model(&self.pine, place);
            }
        }
        if set.billboards {
            let trees: Vec<Billboard> = self
                .trees
                .iter()
                .map(|p| {
                    let size = vec2(4.0, 6.0) * p.scale;
                    Billboard::upright(self.on_ground(p.at, 0.3), size)
                })
                .collect();
            s.draw_billboards(&self.tree_style, &trees);
        }
        let ground: Vec<Vec3> = self
            .rock_spots
            .iter()
            .map(|(r, _)| self.on_ground(r.at, 0.3))
            .collect();
        for ((r, state), at) in self.rock_spots.iter_mut().zip(ground) {
            let place = Mat4::from_scale_rotation_translation(
                Vec3::splat(r.scale * 1.8),
                Quat::from_rotation_y(r.yaw),
                at,
            );
            s.draw_lod(&self.rocks, &self.rock_material, place, state);
        }
        for (i, camp) in CAMPS.iter().enumerate() {
            props::draw_fire(s, &self.fire, self.on_ground(*camp, 0.1), t, i as f32);
        }
        if set.searchlight {
            self.tower.draw(s, self.on_ground(TOWER, 0.0), t);
        }
        if set.labels {
            self.labels(s);
        }
    }

    fn labels(&self, s: &mut Scene) {
        let style = LabelStyle {
            text: TextStyle::new(15.0).color(Color::rgb8(240, 238, 230)),
            fade: Some((260.0, 420.0)),
            ..LabelStyle::default()
        };
        let lake = vec3(LAKE.x, WATER + 3.0, LAKE.y);
        s.label(&self.font, lake, "Mirror Lake", &style);
        s.label(&self.font, self.on_ground(CAMPS[0], -2.5), "Camp", &style);
        s.label(
            &self.font,
            self.on_ground(CAMPS[1], -2.5),
            "Old camp",
            &style,
        );
        s.label(
            &self.font,
            self.on_ground(TOWER, -12.0),
            "Watchtower",
            &style,
        );
        s.label(&self.font, self.summit + Vec3::Y * 3.0, "Summit", &style);
    }

    fn dig(&self, ctx: &Context, cam: &Camera3D) {
        let ray = cam.screen_ray(ctx.input.mouse(), ctx.gfx.view_size());
        if let Some(hit) = self.terrain.raycast(&ray, 2000.0) {
            let brush = Brush::dig(
                hit.point,
                self.settings.dig_radius,
                self.settings.dig_radius * 0.6,
            );
            self.terrain.apply_brush(&ctx.gfx, &brush);
        }
    }
}

/// Flattens a round clearing at `at` and digs a shallow fire pit in it.
fn clear_camp(ctx: &Context, terrain: &Terrain, at: Vec2) {
    let center = vec3(at.x, 0.0, at.y);
    let level = terrain.height(at.x, at.y);
    let flatten = Brush {
        center,
        radius: 7.0,
        strength: 1.0,
        mode: BrushMode::Flatten(level),
    };
    terrain.apply_brush(&ctx.gfx, &flatten);
    terrain.apply_brush(&ctx.gfx, &Brush::dig(center, 1.4, 0.35));
}

impl Game for Valley {
    fn init(ctx: &mut Context) -> GameResult<Self> {
        let options = TerrainOptions {
            cell: world::CELL,
            lod_distance: 48.0,
            layers: world::layers(&ctx.gfx, SEED)?,
            rules: Some(world::splat_rules()),
            ..TerrainOptions::default()
        };
        let terrain = Terrain::new(&ctx.gfx, world::height_map(SEED), options)?;
        for camp in CAMPS {
            clear_camp(ctx, &terrain, camp);
        }
        clear_camp(ctx, &terrain, TOWER);
        let water = Water::over_terrain(
            &ctx.gfx,
            &terrain,
            WaterOptions {
                center: vec3(LAKE.x, WATER, LAKE.y),
                size: Vec2::splat(LAKE_RADIUS * 2.6),
                ..WaterOptions::default()
            },
        );
        let pine = props::pine(&ctx.gfx)?.instance();
        let mut pine_far = Impostor::bake(&mut ctx.gfx, &pine, ImpostorOptions::default())?;
        pine_far.distance = 70.0;
        pine_far.fade = 12.0;
        let mut rng = Rng::new(SEED);
        let near_camp = |x: f32, z: f32| {
            CAMPS
                .iter()
                .chain([&TOWER])
                .any(|c| c.distance(vec2(x, z)) < 9.0)
        };
        let t = &terrain;
        let pines = scatter(&mut rng, 320, 6000, |x, z| {
            let h = t.height(x, z);
            h > 2.0 && h < world::SNOW_LINE - 4.0 && t.normal(x, z).y > 0.86 && !near_camp(x, z)
        });
        let trees = scatter(&mut rng, 420, 6000, |x, z| {
            let h = t.height(x, z);
            h > 1.8 && h < 18.0 && t.normal(x, z).y > 0.9 && !near_camp(x, z)
        });
        let rock_spots = scatter(&mut rng, 90, 3000, |x, z| {
            t.height(x, z) > WATER + 0.3 && !near_camp(x, z)
        })
        .into_iter()
        .map(|s| (s, LodState::default()))
        .collect();
        let map = terrain.height_map();
        let (mut best, mut summit) = (f32::MIN, Vec3::ZERO);
        let half = (world::SAMPLES - 1) as f32 * world::CELL * 0.5;
        for j in 0..map.rows() {
            for i in 0..map.cols() {
                let h = map.heights()[(j * map.cols() + i) as usize];
                if h > best {
                    best = h;
                    summit = vec3(
                        i as f32 * world::CELL - half,
                        h,
                        j as f32 * world::CELL - half,
                    );
                }
            }
        }
        let tree_texture =
            Texture::from_image(&ctx.gfx, &props::tree_image(), TextureOptions::smooth())?;
        let mut fly = FlyCamera::new(vec3(-40.0, 34.0, 172.0));
        fly.look_at(vec3(24.0, 2.0, 30.0));
        fly.speed = 25.0;
        fly.lens.projection = Projection::Perspective {
            fov_y: 55f32.to_radians(),
        };
        fly.lens.far = 1200.0;
        let view = ctx.gfx.view_size();
        Ok(Valley {
            fly,
            terrain,
            water,
            pine,
            pine_far,
            pines,
            trees,
            tree_style: BillboardStyle::textured(&tree_texture),
            rocks: props::rocks(&ctx.gfx)?,
            rock_material: Material::color(Color::rgb8(128, 122, 114), 0.0, 0.85),
            rock_spots,
            fire: FireParts::new(&ctx.gfx)?,
            tower: Tower::new(&ctx.gfx)?,
            summit,
            font: ctx.gfx.default_font(),
            settings: Settings::default(),
            panel: Rect::new(view.x - 282.0, 12.0, 270.0, 372.0),
        })
    }

    fn update(&mut self, ctx: &mut Context) -> GameResult {
        if ctx.input.key_pressed(Key::Escape) {
            ctx.quit();
        }
        if ctx.input_captured() {
            return Ok(());
        }
        self.fly.update(&ctx.input, ctx.time.delta());
        let ground = self
            .terrain
            .height(self.fly.position.x, self.fly.position.z);
        self.fly.position.y = self.fly.position.y.max(ground.max(WATER) + 1.5);
        if ctx.input.mouse_pressed(MouseButton::Left) && !ctx.ui.wants_mouse() {
            let cam = self.fly.camera();
            self.dig(ctx, &cam);
        }
        Ok(())
    }

    fn draw(&mut self, ctx: &mut Context) -> GameResult {
        let t = ctx.time.elapsed() as f32;
        let cam = self.fly.camera();
        ctx.gfx.scene(&cam, |s| self.draw_scene(s, t));
        let stats = ctx.gfx.stats();
        panel::draw_stats(&mut ctx.gfx, &self.font, &self.terrain, stats);
        let settings = &mut self.settings;
        let rect = &mut self.panel;
        ctx.ui.draw(&mut ctx.gfx, &ctx.input, |f| {
            f.window("Terrain", rect, |f| settings.widgets(f));
        });
        Ok(())
    }
}

fn main() -> Result<(), yowie::Error> {
    let config = Config::new("Yowie terrain").with_size(1280, 720);
    yowie::run::<Valley>(config)
}
