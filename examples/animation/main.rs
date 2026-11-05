//! Keyframe clips on 2D sprites and 3D transforms, a flipbook walker,
//! crossfades between a hero's clips, `Finished` events chaining back to
//! idle, a robot arm firing an animation event, another reaching a moving
//! target by two-bone IK and a third mixing two clips by a pace slider.
//!
//! The ECS runs the animation: players advance in a system, the engine's
//! `animate` system writes their poses into transforms, and a system turns
//! `Finished` events into crossfades. Use the Idle, Jump and Spin buttons
//! to crossfade the hero; drag the 3D view to orbit. `--autoplay` presses
//! Jump and Spin on its own, which is how the headless run gets a picture
//! mid-crossfade. Escape quits.

mod arms;
mod clips;
#[path = "../common/mod.rs"]
mod common;

use std::sync::Arc;

use arms::Arms;
use yowie::anim::{Clip, FlipbookPlayer, Player, PlayerEvent, Value};
use yowie::ecs::{Entity, Schedule, Stage, World};
use yowie::ecs_glue::{AnimationEvent, DeltaTime, animate};
use yowie::gfx::scene::{Camera3D, DirectionalLight, Material, Mesh, MeshData, Sky};
use yowie::gfx::{Font, Sprite, SpriteSheet, Texture, TextureOptions};
use yowie::input::Key;
use yowie::{
    Color, Config, Context, Game, GameResult, Mat4, OrbitCamera, Quat, Rect, Transform,
    Transform2D, Vec2, Vec3, uvec2, vec2, vec3,
};

/// Where a 2D entity sits; its clip moves it relative to this.
struct Anchor(Vec2);
/// The colour and size a sprite's clip drives.
struct Look {
    tint: Color,
    size: Vec2,
}
/// A cube of the orbiting ring.
struct Cube(Color);
/// The sheet-frame walker.
struct Walker(FlipbookPlayer);

/// The hero's clips.
struct Clips {
    idle: Arc<Clip>,
    jump: Arc<Clip>,
    spin: Arc<Clip>,
}

/// The most recent lines of what happened.
struct Log(Vec<String>);

impl Log {
    fn say(world: &mut World, line: String) {
        if let Some(log) = world.resource_mut::<Log>() {
            log.0.push(line);
            if log.0.len() > 6 {
                log.0.remove(0);
            }
        }
    }
}

/// Copies the values a sprite clip drives besides its transform.
fn look(world: &mut World) {
    for (player, look) in world.query_mut::<(&Player, &mut Look)>() {
        player.evaluate(|name, value| match (name, value) {
            ("tint", Value::Color(c)) => look.tint = c,
            ("size", Value::Vec2(s)) => look.size = s,
            _ => {}
        });
    }
}

fn flipbooks(world: &mut World) {
    let dt = world.resource::<DeltaTime>().map_or(0.0, |d| d.0);
    for walker in world.query_mut::<&mut Walker>() {
        walker.0.advance(dt);
    }
}

/// Fades an entity whose one-shot clip ended back to idle.
fn back_to_idle(world: &mut World) {
    let ended: Vec<(Entity, String)> = world
        .read_events::<AnimationEvent>()
        .filter_map(|e| match &e.event {
            PlayerEvent::Finished { clip } => Some((e.entity, clip.clone())),
            PlayerEvent::Marker { .. } => None,
        })
        .collect();
    for (entity, clip) in ended {
        let Some(idle) = world.resource::<Clips>().map(|c| Arc::clone(&c.idle)) else {
            continue;
        };
        let switched = match world.get::<&mut Player>(entity) {
            Ok(mut p) if p.clip().is_some_and(|c| c.name == clip) => {
                p.crossfade(idle, 0.3);
                true
            }
            _ => false,
        };
        if switched {
            Log::say(world, format!("finished {clip}, back to idle"));
        }
    }
}

struct Anim {
    world: World,
    schedule: Schedule,
    hero: Entity,
    arms: Arms,
    cube: Mesh,
    ball: Mesh,
    dot: Texture,
    sheet: SpriteSheet,
    font: Font,
    orbit: OrbitCamera,
    panel: Rect,
    speed: f32,
    clock: f32,
    autoplay: bool,
    script: usize,
}

/// A white disc with a soft edge.
fn disc(size: u32) -> Vec<u8> {
    let r = size as f32 / 2.0;
    let mut px = Vec::new();
    for y in 0..size {
        for x in 0..size {
            let d = ((x as f32 + 0.5 - r).powi(2) + (y as f32 + 0.5 - r).powi(2)).sqrt();
            px.extend_from_slice(&[255, 255, 255, ((r - d).clamp(0.0, 1.0) * 255.0) as u8]);
        }
    }
    px
}

/// Four 16 by 16 frames of a figure whose legs alternate.
fn walker_sheet() -> Vec<u8> {
    let mut px = vec![0u8; 64 * 16 * 4];
    for frame in 0..4usize {
        let mut put = |x: usize, y: usize, c: [u8; 4]| {
            let i = (y * 64 + frame * 16 + x) * 4;
            px[i..i + 4].copy_from_slice(&c);
        };
        for y in 2..7 {
            for x in 5..11 {
                put(x, y, [250, 220, 180, 255]);
            }
        }
        for y in 7..12 {
            for x in 4..12 {
                put(x, y, [80, 160, 220, 255]);
            }
        }
        let stride = [0i32, 1, 0, -1][frame];
        for y in 12..16 {
            for x in [5 + stride, 6 + stride, 9 - stride, 10 - stride] {
                put(x as usize, y, [40, 40, 90, 255]);
            }
        }
    }
    px
}

impl Anim {
    fn crossfade_hero(&mut self, clip: Arc<Clip>, seconds: f32) {
        Log::say(
            &mut self.world,
            format!("crossfade to {} ({seconds}s)", clip.name),
        );
        if let Ok(mut p) = self.world.get::<&mut Player>(self.hero) {
            p.crossfade(clip, seconds);
        }
    }

    fn hero_clip(&self, pick: impl Fn(&Clips) -> &Arc<Clip>) -> Option<Arc<Clip>> {
        self.world.resource::<Clips>().map(|c| Arc::clone(pick(c)))
    }
}

impl Game for Anim {
    fn init(ctx: &mut Context) -> GameResult<Self> {
        let smooth = TextureOptions::smooth();
        let dot = Texture::from_pixels(&ctx.gfx, 32, 32, &disc(32), smooth)?;
        let walker =
            Texture::from_pixels(&ctx.gfx, 64, 16, &walker_sheet(), TextureOptions::default())?;
        let sheet = SpriteSheet::new(walker, uvec2(16, 16));

        let mut world = World::new();
        let hero_clips = Clips {
            idle: clips::idle(),
            jump: clips::jump(),
            spin: clips::spin(),
        };
        let mut idle = Player::new();
        idle.play(Arc::clone(&hero_clips.idle));
        let hero = world.spawn((idle, Transform::from_xyz(0.0, 0.5, 0.0)));
        world.insert_resource(hero_clips);
        world.insert_resource(Log(Vec::new()));
        world.insert_resource(DeltaTime(0.0));

        let bounce = clips::bounce();
        for i in 0..6 {
            let mut p = Player::new();
            p.play(Arc::clone(&bounce));
            p.seek(i as f32 * 0.2);
            world.spawn((
                p,
                Transform2D::IDENTITY,
                Anchor(vec2(380.0 + i as f32 * 56.0, 130.0)),
                Look {
                    tint: Color::WHITE,
                    size: vec2(28.0, 28.0),
                },
            ));
        }
        let mut p = Player::new();
        p.play(clips::pulse());
        world.spawn((
            p,
            Transform2D::IDENTITY,
            Anchor(vec2(790.0, 95.0)),
            Look {
                tint: Color::rgb8(120, 200, 255),
                size: vec2(30.0, 30.0),
            },
        ));
        let book = Arc::new(yowie::gfx::sheet_flipbook([0, 1, 2, 3], 8.0, &[]));
        world.spawn((Walker(FlipbookPlayer::new(book)), Anchor(vec2(930.0, 36.0))));
        for i in 0..8 {
            let mut p = Player::new();
            p.play(clips::orbit(i as f32 / 8.0 * std::f32::consts::TAU));
            world.spawn((
                p,
                Transform::IDENTITY.with_scale(Vec3::splat(0.4)),
                Cube(Color::rgb8(120 + 15 * i as u8, 160, 220 - 15 * i as u8)),
            ));
        }

        let mut schedule = Schedule::new();
        schedule.add_system(Stage::Update, "animation", animate);
        schedule.add_system(Stage::Update, "sprite-look", look);
        schedule.add_system(Stage::Update, "flipbooks", flipbooks);
        schedule.add_system(Stage::Update, "back-to-idle", back_to_idle);

        let view = ctx.gfx.view_size();
        let yaw = std::env::args()
            .skip_while(|a| a != "--yaw")
            .nth(1)
            .and_then(|v| v.parse().ok())
            .unwrap_or(0.5);
        let mut orbit = OrbitCamera::new(vec3(0.0, 1.0, 0.0), yaw, 0.42, 9.5);
        orbit.lens = Camera3D::perspective(Vec3::ZERO, Vec3::ZERO, 0.8).with_viewport(Rect::new(
            330.0,
            170.0,
            view.x - 340.0,
            view.y - 180.0,
        ));
        Ok(Anim {
            world,
            schedule,
            hero,
            arms: Arms::new(),
            cube: Mesh::new(&ctx.gfx, &MeshData::cube(1.0))?,
            ball: Mesh::new(&ctx.gfx, &MeshData::sphere(1.0, 16, 32))?,
            dot,
            sheet,
            font: ctx.gfx.default_font(),
            orbit,
            panel: Rect::new(10.0, 180.0, 310.0, 430.0),
            speed: 1.0,
            clock: 0.0,
            autoplay: std::env::args().any(|a| a == "--autoplay"),
            script: 0,
        })
    }

    fn update(&mut self, ctx: &mut Context) -> GameResult {
        if ctx.input.key_pressed(Key::Escape) {
            ctx.quit();
        }
        let real = ctx.time.delta();
        let dt = real * self.speed;
        self.clock += dt;
        if self.autoplay {
            let t = ctx.time.elapsed() as f32;
            if self.script == 0 && t >= 0.4 {
                self.script = 1;
                self.crossfade_hero(clips::jump(), 0.15);
            } else if self.script == 1 && t >= 1.7 {
                self.script = 2;
                self.crossfade_hero(clips::spin(), 0.15);
            }
        }
        self.world.insert_resource(DeltaTime(dt));
        self.schedule.run(&mut self.world);
        ctx.record_schedule(&self.schedule);
        for line in self.arms.update(dt, self.clock) {
            Log::say(&mut self.world, line);
        }
        if !ctx.ui.wants_mouse() && !ctx.input_captured() {
            self.orbit.update(&ctx.input, real);
        }
        self.orbit.yaw += real * 0.06;
        Ok(())
    }

    fn draw(&mut self, ctx: &mut Context) -> GameResult {
        let view = ctx.gfx.view_size();
        ctx.gfx
            .fill_rect(Rect::new(0.0, 0.0, view.x, 164.0), Color::rgb8(30, 33, 44));
        common::label(
            ctx,
            &self.font,
            "sprite clips: bounce and tint, pulse",
            vec2(380.0, 8.0),
            14.0,
            common::DIM,
        );
        common::label(
            ctx,
            &self.font,
            "flipbook",
            vec2(900.0, 8.0),
            14.0,
            common::DIM,
        );
        for (t, look, anchor) in self.world.query::<(&Transform2D, &Look, &Anchor)>().iter() {
            let sprite = Sprite::at(anchor.0 + t.position)
                .size(look.size)
                .centered()
                .rotation(t.rotation)
                .tint(look.tint);
            ctx.gfx.draw_sprite(&self.dot, &sprite);
        }
        for (walker, anchor) in self.world.query::<(&Walker, &Anchor)>().iter() {
            if let Some(frame) = walker.0.frame() {
                let sprite = Sprite::at(anchor.0).size(vec2(96.0, 96.0));
                self.sheet.draw_frame(&mut ctx.gfx, frame as u32, &sprite);
            }
        }

        let cam = self.orbit.camera();
        let (world, arms, cube, ball) = (&self.world, &mut self.arms, &self.cube, &self.ball);
        ctx.gfx.scene(&cam, |s| {
            s.set_sun(
                DirectionalLight::new(vec3(-0.4, -1.0, -0.5), Color::rgb(1.0, 0.96, 0.9), 3.0)
                    .with_shadows(30.0),
            );
            s.set_sky(Sky::default());
            let floor = Material::color(Color::rgb8(150, 150, 160), 0.0, 0.9);
            s.draw(
                cube,
                &floor,
                Mat4::from_scale_rotation_translation(
                    vec3(10.0, 0.2, 10.0),
                    Quat::IDENTITY,
                    vec3(0.0, -0.6, 0.0),
                ),
            );
            for (t, c) in world.query::<(&Transform, &Cube)>().iter() {
                s.draw(cube, &Material::color(c.0, 0.3, 0.4), *t);
            }
            if let Ok(t) = world.get::<&Transform>(self.hero) {
                let gold = Material::color(Color::rgb8(255, 200, 90), 0.6, 0.3);
                s.draw(cube, &gold, *t);
            }
            arms.draw(s, cube, ball);
        });

        let names = ["event arm", "IK arm", "blend arm"];
        for (name, base) in names.iter().zip(arms::BASES) {
            if let Some(p) = cam.world_to_screen(base + vec3(0.0, 2.7, 0.0), view) {
                let w = ctx
                    .gfx
                    .measure_text(&self.font, name, &yowie::gfx::TextStyle::new(14.0))
                    .x;
                common::label(
                    ctx,
                    &self.font,
                    name,
                    p - vec2(w / 2.0, 0.0),
                    14.0,
                    common::TEXT,
                );
            }
        }
        self.panel_ui(ctx);
        Ok(())
    }
}

impl Anim {
    fn panel_ui(&mut self, ctx: &mut Context) {
        let weights: Vec<String> = self
            .world
            .get::<&Player>(self.hero)
            .map(|p| {
                p.weights()
                    .iter()
                    .map(|(n, w)| format!("{n} {:.0}%", w * 100.0))
                    .collect()
            })
            .unwrap_or_default();
        let log: Vec<String> = self
            .world
            .resource::<Log>()
            .map(|l| l.0.clone())
            .unwrap_or_default();
        let ik = match (self.arms.ik_on, self.arms.reach_error) {
            (true, Some(e)) => format!("IK hand is {e:.3} from its target"),
            _ => "IK off: the clip alone moves the arm".to_owned(),
        };
        let mut want: Option<usize> = None;
        let (speed, pace, ik_on, panel) = (
            &mut self.speed,
            &mut self.arms.pace,
            &mut self.arms.ik_on,
            &mut self.panel,
        );
        ctx.ui.draw(&mut ctx.gfx, &ctx.input, |f| {
            f.window("Animation", panel, |f| {
                f.label(&format!("hero: {}", weights.join(", ")));
                f.horizontal(|f| {
                    for (i, name) in ["Idle", "Jump", "Spin"].iter().enumerate() {
                        if f.button(name).clicked {
                            want = Some(i);
                        }
                    }
                });
                f.slider("Speed", speed, 0.0..=3.0);
                f.slider("Blend arm pace", pace, 0.0..=1.0);
                f.checkbox("IK arm reaches by IK", ik_on);
                f.label_dim(&ik);
                f.separator();
                for line in &log {
                    f.label_dim(line);
                }
            });
        });
        let pick = want.and_then(|i| match i {
            0 => self.hero_clip(|c| &c.idle).map(|c| (c, 0.3)),
            1 => self.hero_clip(|c| &c.jump).map(|c| (c, 0.15)),
            _ => self.hero_clip(|c| &c.spin).map(|c| (c, 0.15)),
        });
        if let Some((clip, fade)) = pick {
            self.crossfade_hero(clip, fade);
        }
    }
}

fn main() -> Result<(), yowie::Error> {
    let config = Config {
        clear_color: Color::rgb8(24, 26, 34),
        console: true,
        ..Config::new("Yowie animation").with_size(1100, 700)
    };
    yowie::run::<Anim>(config)
}
