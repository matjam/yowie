//! Every widget of the interface toolkit, the built-in themes, a texture
//! skin, a live particle editor that saves its emitter as JSON, the
//! frame-timing overlay and the debug console.
//!
//! Arguments: `--theme <name>` picks a built-in theme (try `nord`),
//! `--skin` draws the widgets from nine-slice textures, `--debug` (or
//! `--overlay`) shows the frame-timing overlay, `--console` and `--panels` start with the
//! console drop-down or the debug panels open, and `--beep` plays a tone through the audio
//! mixer (never in headless runs). In a window, the backtick key opens
//! the console and F4 the debug panels; F3 toggles the overlay. Try
//! `theme nord`, `set gallery.volume 0.2` and `help` in the console.

use yowie::audio::{Pcm, PlayOptions, Sound};
use yowie::console::IntVar;
use yowie::gfx::{NineSlice, Texture, TextureOptions};
use yowie::input::Key;
use yowie::particle::{Backend, Blend, Emitter, Particles as Effect, presets};
use yowie::ui::{Column, Frame, Skin, Theme, theme_names};
use yowie::{Color, Config, Context, Game, GameResult, Rect, Vec2, vec2};

const MAPS: [&str; 5] = ["Archipelago", "Pangaea", "Fractal", "Highlands", "Atoll"];
const QUALITY: [&str; 3] = ["Low", "Medium", "High"];
const ITEMS: [&str; 3] = ["Sword", "Shield", "Potion"];

struct Gallery {
    args: Args,
    theme: IntVar,
    applied: (i64, bool),
    skin: Skin,
    use_skin: bool,
    audio: Option<Sound>,
    play_beep: bool,
    quit: bool,
    volume: yowie::console::FloatVar,

    clicks: u32,
    shadows: bool,
    difficulty: i32,
    lives: i32,
    name: String,
    quality: usize,
    tint_hue: f32,
    win_a: Rect,
    win_b: Rect,
    editor: Editor,
    tab: usize,
    map: usize,
    fog: bool,
    notes: String,
    table_sel: Option<usize>,
    slots: [Option<&'static str>; 3],
    confirm: bool,
    about: bool,
}

#[derive(Clone, Debug, Default)]
struct Args {
    theme: Option<String>,
    skin: bool,
    beep: bool,
    overlay: bool,
    console: bool,
    panels: bool,
}

impl Args {
    fn parse() -> Args {
        let mut a = Args::default();
        let mut it = std::env::args().skip(1);
        while let Some(arg) = it.next() {
            match arg.as_str() {
                "--theme" => a.theme = it.next(),
                "--skin" => a.skin = true,
                "--beep" => a.beep = true,
                "--overlay" | "--debug" => a.overlay = true,
                "--console" => a.console = true,
                "--panels" => a.panels = true,
                other => {
                    if let Some(t) = other.strip_prefix("--theme=") {
                        a.theme = Some(t.to_owned());
                    }
                }
            }
        }
        a
    }
}

/// A 16 by 16 beveled tile, raised or sunk, as pixel art for a nine-slice
/// with a 4 unit border.
fn bevel(ctx: &Context, base: [u8; 3], raised: bool) -> GameResult<NineSlice> {
    let mut px = Vec::with_capacity(16 * 16 * 4);
    for y in 0..16i32 {
        for x in 0..16i32 {
            let d = x.min(y).min(15 - x).min(15 - y);
            // The top and left edges catch the light; the bottom and right
            // edges are in shadow.
            let top_left = y == d || x == d;
            let shade = |c: u8, k: f32| (f32::from(c) * k).clamp(0.0, 255.0) as u8;
            let rgb = match d {
                0 => [14, 16, 24],
                1 | 2 => {
                    let light = top_left == raised;
                    let k = if light { 1.35 } else { 0.62 };
                    base.map(|c| shade(c, k))
                }
                _ => base.map(|c| shade(c, 1.0 - y as f32 * 0.012)),
            };
            px.extend_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
        }
    }
    let tex = Texture::from_pixels(&ctx.gfx, 16, 16, &px, TextureOptions::default())?;
    Ok(NineSlice::uniform(tex, 4.0))
}

fn make_skin(ctx: &Context) -> GameResult<Skin> {
    Ok(Skin {
        panel: Some(bevel(ctx, [96, 84, 104], true)?),
        title: Some(bevel(ctx, [150, 80, 70], true)?),
        button: Some(bevel(ctx, [120, 110, 140], true)?),
        button_hover: Some(bevel(ctx, [150, 140, 175], true)?),
        button_active: Some(bevel(ctx, [90, 80, 110], false)?),
        field: Some(bevel(ctx, [50, 46, 62], false)?),
        track: Some(bevel(ctx, [60, 54, 74], false)?),
        fill: Some(bevel(ctx, [90, 170, 190], true)?),
    })
}

impl Gallery {
    fn build_theme(&self) -> Theme {
        let names: Vec<&str> = theme_names().collect();
        let i = (self.theme.get().max(0) as usize).min(names.len() - 1);
        let mut t = Theme::named(names[i]).unwrap_or_default();
        if self.use_skin {
            t.skin = Some(self.skin.clone());
            t.border_width = 0.0;
        }
        t
    }
}

impl Game for Gallery {
    fn init(ctx: &mut Context) -> GameResult<Self> {
        let args = Args::parse();
        let names: Vec<&str> = theme_names().collect();
        let first = args
            .theme
            .as_deref()
            .and_then(|t| names.iter().position(|n| *n == t))
            .unwrap_or(0);
        let theme = ctx.console.int(
            "gallery.theme",
            first as i64,
            "index of the interface theme",
        );
        let volume = ctx
            .console
            .float("gallery.volume", 0.65, "the volume slider's value");
        let t = theme.clone();
        ctx.console.register_completing(
            "theme",
            "theme [name]: switch the interface theme, or list them",
            move |args| match args.first() {
                None => Ok(theme_names().collect::<Vec<_>>().join(" ")),
                Some(n) => match theme_names().position(|t| t == n) {
                    Some(i) => {
                        t.set(i as i64);
                        Ok(format!("theme {n}"))
                    }
                    None => Err(format!("no theme {n:?}")),
                },
            },
            |args| {
                let prefix = args.last().map_or("", String::as_str);
                theme_names()
                    .filter(|n| n.starts_with(prefix))
                    .map(str::to_owned)
                    .collect()
            },
        );
        // The engine opens the output device in windowed runs and never in
        // headless ones, where the mixer renders to memory.
        let beep = ctx.audio.sound(&Pcm::sine(440.0, 0.3, ctx.audio.rate()))?;
        ctx.console.attach_mixer(&ctx.audio);
        let audio = Some(beep);
        let skin = make_skin(ctx)?;
        let mut g = Gallery {
            theme,
            applied: (-1, false),
            use_skin: args.skin,
            play_beep: args.beep,
            args,
            skin,
            audio,
            quit: false,
            volume,
            clicks: 0,
            shadows: true,
            difficulty: 1,
            lives: 3,
            name: "Ada".into(),
            quality: 2,
            tint_hue: 0.07,
            win_a: Rect::new(16.0, 40.0, 452.0, 548.0),
            win_b: Rect::new(484.0, 40.0, 460.0, 548.0),
            editor: Editor::new(ctx)?,
            tab: 0,
            map: 1,
            fog: true,
            notes: "Multi-line notes wrap here.\nSelect with Shift and the arrows, then cut, copy, paste or undo.".into(),
            table_sel: Some(1),
            slots: [Some("Sword"), None, None],
            confirm: false,
            about: false,
        };
        ctx.set_debug_overlay(g.args.overlay);
        if g.args.console {
            // For screenshots: open the drop-down with a few commands run.
            ctx.console.set_open(true);
            ctx.console.execute("help");
            ctx.console.execute("get gallery.volume");
            ctx.console.execute("theme");
        }
        ctx.console.set_panels_open(g.args.panels);
        let theme = g.build_theme();
        ctx.ui.set_theme(theme);
        g.applied = (g.theme.get(), g.use_skin);
        Ok(g)
    }

    fn update(&mut self, ctx: &mut Context) -> GameResult {
        if ctx.input.key_pressed(Key::Escape) && !ctx.ui.wants_keyboard() && !self.confirm {
            self.confirm = true;
        }
        if self.play_beep {
            self.play_beep = false;
            if let Some(beep) = &self.audio {
                ctx.audio.play(
                    beep,
                    PlayOptions {
                        volume: 0.4,
                        ..PlayOptions::default()
                    },
                );
            }
        }
        if self.quit {
            ctx.quit();
        }
        Ok(())
    }

    fn draw(&mut self, ctx: &mut Context) -> GameResult {
        if (self.theme.get(), self.use_skin) != self.applied {
            self.applied = (self.theme.get(), self.use_skin);
            let theme = self.build_theme();
            ctx.ui.set_theme(theme);
        }
        let t = ctx.time.elapsed() as f32;
        let view = ctx.gfx.view_size();
        for i in 0..10 {
            let a = t * 0.4 + i as f32;
            let c = vec2(
                view.x * 0.5 + 360.0 * a.cos(),
                view.y * 0.55 + 200.0 * (a * 1.3).sin(),
            );
            let col = Color::rgba8(70 + i as u8 * 12, 80, 190 - i as u8 * 9, 90);
            ctx.gfx.fill_circle(c, 70.0, col);
        }
        let mut quit = false;
        let (names, sel_theme) = (theme_names().collect::<Vec<_>>(), self.theme.get());
        let mut theme_idx = (sel_theme.max(0) as usize).min(names.len() - 1);
        let progress = (t * 0.25).fract();
        let mut vol = self.volume.get();
        let mut skin_on = self.use_skin;
        let mut beep = false;
        ctx.ui.draw(&mut ctx.gfx, &ctx.input, |f| {
            self.menu(f, &names, &mut theme_idx, &mut skin_on);
            self.widgets_window(
                f,
                &names,
                &mut theme_idx,
                &mut skin_on,
                &mut vol,
                progress,
                &mut beep,
            );
            self.more_window(f);
            self.editor.window(f);
            self.dialogs(f, &mut quit);
        });
        self.editor.draw_preview(&mut ctx.gfx, ctx.time.delta());
        self.volume.set(vol);
        self.theme.set(theme_idx as i64);
        self.use_skin = skin_on;
        self.play_beep |= beep;
        self.quit |= quit;
        Ok(())
    }
}

impl Gallery {
    fn menu(
        &mut self,
        f: &mut Frame<'_>,
        names: &[&str],
        theme_idx: &mut usize,
        skin_on: &mut bool,
    ) {
        f.menu_bar(|f| {
            f.menu("File", |f| {
                if f.menu_item_with("Reset clicks", "", None).clicked {
                    self.clicks = 0;
                }
                f.menu_separator();
                if f.menu_item_with("Quit", "Esc", None).clicked {
                    self.confirm = true;
                }
            });
            f.menu("View", |f| {
                f.menu("Theme", |f| {
                    for (i, n) in names.iter().enumerate() {
                        if f.menu_item_with(n, "", Some(i == *theme_idx)).clicked {
                            *theme_idx = i;
                        }
                    }
                });
                if f.menu_item_with("Texture skin", "", Some(*skin_on)).clicked {
                    *skin_on = !*skin_on;
                }
            });
            f.menu("Help", |f| {
                if f.menu_item("About").clicked {
                    self.about = true;
                }
            });
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn widgets_window(
        &mut self,
        f: &mut Frame<'_>,
        names: &[&str],
        theme_idx: &mut usize,
        skin_on: &mut bool,
        vol: &mut f32,
        progress: f32,
        beep: &mut bool,
    ) {
        f.window("Widgets", &mut self.win_a, |f| {
            f.heading("Buttons and toggles");
            f.horizontal(|f| {
                let b = f.button("Click me");
                f.tooltip(&b, "Counts clicks; Tab and Enter work too");
                if b.clicked {
                    self.clicks += 1;
                }
                f.label(&format!("clicked {} times", self.clicks));
            });
            f.checkbox("Enable shadows", &mut self.shadows);
            f.columns(3, |f| {
                for (i, d) in ["Easy", "Normal", "Hard"].iter().enumerate() {
                    f.radio(d, &mut self.difficulty, i as i32);
                }
            });
            f.separator();
            f.heading("Values");
            f.columns(2, |f| {
                f.slider("Volume", vol, 0.0..=1.0);
                f.slider_int("Lives", &mut self.lives, 1..=9);
            });
            f.progress("Loading", progress);
            f.separator();
            f.heading("Text and choices");
            f.text_field("Name", &mut self.name);
            f.combo_box("Quality", &mut self.quality, &QUALITY);
            f.horizontal(|f| {
                let tint = Color::hsv(self.tint_hue * 360.0, 0.85, 1.0);
                if f.color_swatch("Tint (click)", tint).clicked {
                    self.tint_hue = (self.tint_hue + 0.13).fract();
                }
                f.color_swatch("Glass", tint.with_alpha(0.45));
            });
            f.separator();
            f.heading("Look and sound");
            f.combo_box("Theme", theme_idx, names);
            f.checkbox("Texture skin", skin_on);
            f.horizontal(|f| {
                let can = self.audio.is_some();
                f.disabled(!can, |f| {
                    if f.button("Play beep").clicked {
                        *beep = true;
                    }
                });
                f.label_dim("through the audio mixer");
            });
        });
    }

    fn more_window(&mut self, f: &mut Frame<'_>) {
        let mut win = self.win_b;
        f.window("More widgets", &mut win, |f| {
            f.tabs("sections", &["Lists", "Text", "Table", "Drag"], &mut self.tab);
            f.space(4.0);
            match self.tab {
                0 => {
                    f.list_box("Maps", 124.0, &MAPS, &mut self.map);
                    f.collapsing_header("Options", true, |f| {
                        f.checkbox("Fog of war", &mut self.fog);
                        f.label_dim("collapsing headers keep their state");
                    });
                    f.tree_node_with("World", true, false, |f| {
                        f.tree_node_with("Forest", true, false, |f| {
                            f.tree_leaf("Oak", false);
                            f.tree_leaf("Pine", self.fog);
                        });
                        f.tree_node("Desert", |f| {
                            f.tree_leaf("Dune", false);
                        });
                    });
                }
                1 => {
                    f.text_area("Notes", &mut self.notes, 150.0);
                    f.paragraph("Text fields support selection with Shift and the arrows or the mouse, double-click for a word, clipboard shortcuts, undo and input-method composition.");
                }
                2 => {
                    let cols = [Column::fill("Item"), Column::fixed("Qty", 60.0), Column::fixed("Weight", 80.0)];
                    let rows = [("Iron sword", 1, 3.5), ("Wooden shield", 2, 4.0), ("Healing potion", 12, 0.2), ("Rope", 3, 1.1), ("Lantern", 1, 2.0)];
                    f.table("Inventory", &cols, rows.len(), 190.0, &mut self.table_sel, |f, r, c| {
                        let (n, q, w) = rows[r];
                        match c {
                            0 => f.label(n),
                            1 => f.label(&q.to_string()),
                            _ => f.label(&format!("{w:.1} kg")),
                        };
                    });
                    if let Some(i) = self.table_sel {
                        f.label_dim(&format!("selected: {}", rows[i].0));
                    }
                }
                _ => self.drag_tab(f),
            }
        });
        self.win_b = win;
    }

    fn drag_tab(&mut self, f: &mut Frame<'_>) {
        f.label_dim("Drag an item onto a slot.");
        f.columns(3, |f| {
            for item in ITEMS {
                let r = f.button(item);
                f.drag_source(&r, item, item);
            }
        });
        f.space(8.0);
        for (i, slot) in self.slots.iter_mut().enumerate() {
            let text = match slot {
                Some(item) => format!("Slot {}: {item}", i + 1),
                None => format!("Slot {}: empty", i + 1),
            };
            let r = f.button(&format!("{text}##slot{i}"));
            if let Some(item) = f.drop_target::<&'static str>(&r) {
                *slot = Some(item);
            }
        }
        if f.button("Clear slots").clicked {
            self.slots = [None; 3];
        }
    }

    fn dialogs(&mut self, f: &mut Frame<'_>, quit: &mut bool) {
        let mut confirm = self.confirm;
        f.modal("Quit?", &mut confirm, vec2(300.0, 130.0), |f| {
            f.label("Leave the gallery?");
            f.columns(2, |f| {
                if f.button("Quit").clicked {
                    *quit = true;
                }
                if f.button("Stay").clicked {
                    self.confirm = false;
                }
            });
        });
        if self.confirm {
            self.confirm = confirm;
        }
        let mut about = self.about;
        f.modal("About", &mut about, Vec2::new(340.0, 150.0), |f| {
            f.paragraph("Yowie's immediate-mode interface toolkit. Press ` for the console and F4 for the panels.");
            if f.button("Close").clicked {
                self.about = false;
            }
        });
        if self.about {
            self.about = about;
        }
    }
}

// ---- The particle editor --------------------------------------------------

const PRESETS: [&str; 5] = ["Fire", "Smoke", "Sparks", "Confetti", "Rain"];
/// Where `Save JSON` writes, in the working directory.
const SAVE_PATH: &str = "emitter.json";
const PREVIEW_H: f32 = 120.0;

/// A window with a live particle preview, sliders for the emitter and its
/// JSON, which can be edited as text and saved to a file.
struct Editor {
    fx: Effect,
    emitter: Emitter,
    preset: usize,
    tab: usize,
    win: Rect,
    preview: Rect,
    json: String,
    status: String,
    gpu: bool,
}

fn preset_emitter(i: usize) -> Emitter {
    let e = match i {
        0 => presets::fire(),
        1 => presets::smoke(),
        2 => presets::sparks(),
        3 => presets::confetti(),
        _ => presets::rain(),
    };
    Emitter {
        layer: yowie::gfx::layers::UI + 1,
        prewarm: 0.0,
        ..e
    }
}

impl Editor {
    fn new(ctx: &Context) -> GameResult<Self> {
        let soft = Texture::from_image(
            &ctx.gfx,
            &presets::soft_circle(64),
            TextureOptions::smooth(),
        )?;
        let emitter = preset_emitter(0);
        let mut fx = Effect::new(&ctx.gfx, emitter.clone(), Backend::Cpu);
        fx.set_texture(soft.clone());
        Ok(Self {
            fx,
            json: emitter.to_json().unwrap_or_default(),
            emitter,
            preset: 0,
            tab: 0,
            win: Rect::new(960.0, 40.0, 304.0, 548.0),
            preview: Rect::ZERO,
            status: String::new(),
            gpu: false,
        })
    }

    fn load_preset(&mut self, i: usize) {
        self.preset = i;
        self.emitter = preset_emitter(i);
        self.fx.set_emitter(self.emitter.clone());
        self.fx.start();
    }

    /// The emitter as written to a file: on the default layer.
    fn saved(&self) -> Emitter {
        Emitter {
            layer: 0,
            ..self.emitter.clone()
        }
    }

    fn window(&mut self, f: &mut Frame<'_>) {
        let mut win = self.win;
        f.window("Particle editor", &mut win, |f| {
            self.preview = f.allocate(0.0, PREVIEW_H);
            f.fill_rect(self.preview, Color::rgb8(10, 12, 22));
            f.tabs("editor", &["Tune", "JSON"], &mut self.tab);
            f.space(4.0);
            if self.tab == 0 {
                self.tune(f);
            } else {
                self.json_tab(f);
            }
            if !self.status.is_empty() {
                f.label_dim(&self.status.clone());
            }
        });
        self.win = win;
    }

    fn tune(&mut self, f: &mut Frame<'_>) {
        let mut preset = self.preset;
        f.combo_box("Preset", &mut preset, &PRESETS);
        if preset != self.preset {
            self.load_preset(preset);
        }
        let e = &mut self.emitter;
        f.slider("Rate", &mut e.rate, 0.0..=400.0);
        f.slider("Lifetime", &mut e.lifetime.max, 0.2..=4.0);
        f.slider("Speed", &mut e.speed.max, 0.0..=300.0);
        f.slider("Spread", &mut e.spread, 0.0..=std::f32::consts::TAU);
        f.slider("Gravity", &mut e.gravity.y, -300.0..=300.0);
        f.slider("Size", &mut e.size.max, 2.0..=60.0);
        f.slider("Drag", &mut e.drag, 0.0..=2.0);
        e.lifetime.min = e.lifetime.min.min(e.lifetime.max);
        e.speed.min = e.speed.min.min(e.speed.max);
        e.size.min = e.size.min.min(e.size.max);
        let mut additive = e.blend == Blend::Additive;
        let mut gpu = self.gpu;
        let mut save = false;
        f.columns(3, |f| {
            f.checkbox("Add", &mut additive);
            f.checkbox("GPU", &mut gpu);
            save = f.button("Save JSON").clicked;
        });
        e.blend = if additive { Blend::Additive } else { Blend::Alpha };
        self.gpu = gpu;
        if save {
            self.save();
        }
        self.json = self.saved().to_json().unwrap_or_default();
    }

    fn json_tab(&mut self, f: &mut Frame<'_>) {
        f.text_area("JSON", &mut self.json, 300.0);
        f.columns(2, |f| {
            if f.button("Apply").clicked {
                match Emitter::from_json(&self.json) {
                    Ok(e) => {
                        self.emitter = Emitter {
                            layer: yowie::gfx::layers::UI + 1,
                            ..e
                        };
                        self.status = "applied the JSON".into();
                    }
                    Err(e) => self.status = e.to_string(),
                }
            }
            if f.button("Save JSON").clicked {
                self.save();
            }
        });
    }

    fn save(&mut self) {
        let json = self.saved().to_json().unwrap_or_default();
        self.status = match std::fs::write(SAVE_PATH, json) {
            Ok(()) => format!("saved {SAVE_PATH}"),
            Err(e) => format!("cannot save {SAVE_PATH}: {e}"),
        };
    }

    /// Runs the edited emitter and draws it inside the preview area.
    fn draw_preview(&mut self, gfx: &mut yowie::gfx::Graphics, dt: f32) {
        let want = if self.gpu { Backend::Gpu } else { Backend::Cpu };
        self.fx.switch_to(gfx, want);
        self.fx.set_emitter(self.emitter.clone());
        let r = self.preview;
        let at = match self.preset {
            0 | 1 => vec2(r.x + r.w * 0.5, r.y + r.h * 0.85),
            4 => vec2(r.x, r.y),
            _ => vec2(r.x + r.w * 0.5, r.y + r.h * 0.5),
        };
        self.fx.set_position(at);
        self.fx.update(dt);
        if self.fx.finished() {
            self.fx.start();
        }
        gfx.with_clip(r, |g| self.fx.draw(g));
    }
}

fn main() -> Result<(), yowie::Error> {
    let args = Args::parse();
    let mut config = Config::new("Yowie gallery").with_size(1280, 600);
    config.console = true;
    config.debug_overlay = args.overlay;
    config.clear_color = Color::rgb8(24, 26, 36);
    yowie::run::<Gallery>(config)
}
