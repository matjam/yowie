---
title: Game services
group: Engine
order: 4
summary: assets and pack files, saves and settings, translation, random numbers, timers, tweens, grids with pathfinding and field of view, and networking
---

# Game services

Yowie ships small libraries for the jobs most games share. Each is a module
of the `yowie` crate:

| Module | Section | What it does |
|---|---|---|
| `yowie::asset` | [Assets](#assets) | Reads files from directories and pack files, loads them on worker threads, and reloads them when they change. The engine's instance is `ctx.assets`. |
| `yowie::save` | [Saves and settings](#saves-and-settings) | Writes versioned JSON slots to the player's data directory. |
| `yowie::locale` | [Translation](#translation) | Looks up translated text with placeholders, plurals and fallback languages. |
| `yowie::rng` | [Random numbers](#random-numbers) | A seeded generator, dice, weighted picks, shuffles and noise. |
| `yowie::timer` | [Timers and sequences](#timers-and-sequences) | Countdowns and scripted steps on game time. |
| `yowie::tween` | [Tweens](#tweens) | Eased animation of values from one end to another. |
| `yowie::grid` | [Grids, paths and field of view](#grids-paths-and-field-of-view) | Cell grids, A*, Dijkstra maps, lines, field of view and flood fill. |
| `yowie::net` | [Networking](#networking) | Typed messages over TCP and UDP, and helpers for networked games. |

Only assets are wired into the `Context`. The others are plain values that
your game creates, keeps in its `Game` struct, and advances from `update`.
None of them reads a clock: timers, tweens and sequences move by the `dt`
you pass, so they pause with the game and follow `ctx.time.set_scale`.

## Assets

### Where files come from

The engine builds `ctx.assets` at start-up over these locations, highest
priority first. Locations that do not exist are skipped, and a pack that
does not open is logged and skipped.

1. `Config::asset_dir`, when set.
2. The `assets` directory in the working directory.
3. The `assets` directory beside the executable.
4. Each pack file in `Config::packs`, in order.
5. `assets.pak` in the working directory, then beside the executable.

A file in a higher location hides a file of the same path in a lower one,
so a loose file overrides the copy in the shipped pack. Asset paths use
forward slashes and are relative to the location's root, for example
`"sprites/hero.png"`. `.` and `..` components are resolved; a path that
climbs above the root is an error.

To read a file directly, use the file system behind the assets:

| Call | Returns |
|---|---|
| `ctx.assets.vfs().read(path)` | The bytes. |
| `ctx.assets.vfs().read_to_string(path)` | The text. |
| `ctx.assets.vfs().exists(path)` | Whether any location has the file. |
| `ctx.assets.vfs().list(dir)` | The entries of a directory across all locations. |
| `ctx.assets.vfs().real_path(path)` | The file's path on disk, when it comes from a directory. |

### Loading engine assets

To load a file as a GPU object or sound, call one of these on the context.
Each reads and decodes the file before it returns.

| Call | Formats | Returns |
|---|---|---|
| `ctx.load_texture(path, TextureOptions)` | PNG, JPEG, Radiance HDR, KTX2 | `Texture` |
| `ctx.load_font(path)` | TrueType, OpenType | `Font` |
| `ctx.load_shader(path)` | WGSL 2D fragment shader | `Shader` |
| `ctx.load_shader_with_uniforms(path, &initial)` | WGSL with a uniform block | `Shader` |
| `ctx.load_sound(path)` | WAV, Ogg Vorbis, MP3, FLAC | `Sound`, decoded at the mixer's rate |
| `ctx.load_map(path)` | Tiled TMX or TMJ, with external tilesets | `Handle<Map>` |

How to draw and play these is in [2D graphics](graphics-2d.md),
[Shaders](shaders.md) and [Audio](audio.md).

### Loading in the background

To keep the window drawing while files load, call
`ctx.load_texture_async(path, options)`. It returns a `TextureLoad` at once
and decodes the image on a worker thread. Call `poll(ctx)` on it each update;
it uploads the image when the decode has finished and then returns the
texture. `ctx.assets.progress()` counts the background loads that have
finished out of those started, for a loading bar.

```rust,no_run
use yowie::gfx::TextureOptions;
use yowie::{Color, Config, Context, Game, GameResult, Rect, TextureLoad, vec2};

const TILES: [&str; 3] = ["tiles/grass.png", "tiles/water.png", "tiles/rock.png"];

struct Loading {
    tiles: Vec<TextureLoad>,
}

impl Game for Loading {
    fn init(ctx: &mut Context) -> GameResult<Self> {
        ctx.assets.reset_progress();
        let tiles = TILES
            .iter()
            .map(|path| ctx.load_texture_async(path, TextureOptions::default()))
            .collect();
        Ok(Loading { tiles })
    }

    fn update(&mut self, ctx: &mut Context) -> GameResult {
        for tile in &mut self.tiles {
            tile.poll(ctx)?;
        }
        Ok(())
    }

    fn draw(&mut self, ctx: &mut Context) -> GameResult {
        let done = ctx.assets.progress().fraction();
        ctx.gfx.fill_rect(Rect::new(100.0, 280.0, 600.0, 16.0), Color::GRAY);
        ctx.gfx.fill_rect(Rect::new(100.0, 280.0, 600.0 * done, 16.0), Color::GREEN);
        for (i, tile) in self.tiles.iter().enumerate() {
            if let Some(texture) = tile.texture() {
                ctx.gfx.draw_texture(texture, vec2(100.0 + i as f32 * 80.0, 320.0));
            }
        }
        Ok(())
    }
}

fn main() -> Result<(), yowie::Error> {
    yowie::run::<Loading>(Config::new("Loading"))
}
```

A `TextureLoad` that failed reports why through `error()`, and `state()`
returns `LoadState::Loading`, `Ready` or `Failed`. Call
`ctx.assets.reset_progress()` before a new batch so the bar starts at zero;
call it only while no background load is running.

### Hot reload

With `Config::hot_reload` on (the default in debug builds), the engine
watches every asset directory. When a file changes, the assets loaded from
it are decoded again on a worker thread:

- Textures and shaders from the `ctx.load_*` calls change in place. Every
  sprite, material and clone that uses them shows the new version.
- A shader that no longer compiles is logged, and the previous program keeps
  running.
- A map's handle gets a higher `generation()`. Compare it each frame and
  rebuild what the map drives when it changes.
- Fonts and sounds are copies and do not change.
- A failed reload keeps the previous version.

Pack files are not watched. Hot reload needs at least one asset directory.

To react to loads and reloads, read `ctx.asset_events()`. The engine
collects the events once per frame, before the frame's updates.

```rust,no_run
use yowie::Context;
use yowie::asset::AssetEvent;

fn report_assets(ctx: &Context) {
    for event in ctx.asset_events() {
        match event {
            AssetEvent::Reloaded { path, .. } => log::info!("reloaded {path}"),
            AssetEvent::ReloadFailed { path, error, .. } => {
                log::warn!("{path} kept its previous version: {error}");
            }
            AssetEvent::Failed { path, error, .. } => log::warn!("{path} did not load: {error}"),
            AssetEvent::Loaded { .. } | AssetEvent::Changed { .. } => {}
        }
    }
}
```

| Event | Sent when |
|---|---|
| `Loaded { path, type_name }` | A background load finished. |
| `Failed { path, type_name, error }` | A background load failed. |
| `Changed { path }` | A watched file changed, whether or not anything uses it. |
| `Reloaded { path, type_name }` | An asset was decoded again; its handle's generation increased. |
| `ReloadFailed { path, type_name, error }` | Decoding the changed file failed. |

To reload a path by hand, call `ctx.assets.reload(path)`.

### Your own asset types

To load a file format of your own through the same system, implement
`Loader` and register it. A loader turns a file's bytes into a value. It runs
on a worker thread, so it must produce CPU data only; create GPU objects from
the result on the game thread.

```rust
use yowie::asset::{Assets, BoxError, LoadContext, Loader, MemorySource, Vfs};

/// A level as rows of characters.
struct LevelText {
    rows: Vec<String>,
}

struct LevelLoader;

impl Loader for LevelLoader {
    type Asset = LevelText;

    fn load(&self, bytes: &[u8], _path: &str, _ctx: &LoadContext<'_>) -> Result<LevelText, BoxError> {
        let text = std::str::from_utf8(bytes)?;
        Ok(LevelText { rows: text.lines().map(str::to_owned).collect() })
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // In a game, use ctx.assets instead of building a manager.
    let mut vfs = Vfs::new();
    vfs.mount_memory(MemorySource::new().with("levels/one.txt", &b"#####\n#...#\n#####"[..])?);
    let assets = Assets::new(vfs, 1)?;

    assets.register(LevelLoader);
    let level = assets.load::<LevelText>("levels/one.txt");
    assert_eq!(level.wait()?.rows.len(), 3);
    Ok(())
}
```

In a game, register the loader in `init` with `ctx.assets.register(...)`.
Then load with one of two calls:

| Call | Behaviour |
|---|---|
| `ctx.assets.load::<T>(path)` | Starts loading on a worker and returns a `Handle<T>` at once. |
| `ctx.assets.load_now::<T>(path)` | Loads on the calling thread and returns the ready handle or the error. |

Loading the same path as the same type while a handle to it is alive returns
the same asset. A `Handle<T>` reports `state()`, `is_ready()`, `error()` and
`generation()`, and `get()` returns the value as an `Arc<T>` once it is
ready. `wait()` blocks until the load ends.

To read files that an asset refers to, such as a level's tile set, use the
`LoadContext` passed to `load`: `ctx.read(path)` takes an asset path,
`ctx.read_relative(rel)` takes a path relative to the asset's own
directory. Files read through it are recorded, so changing one of them also
reloads the asset.

### Pack files

To ship assets as one file, pack the directory with `yowie-pack`:

```bash
cargo run -p yowie-asset --bin yowie-pack -- -o assets.pak assets
```

| Option | Default | Effect |
|---|---|---|
| `-o`, `--output <file>` | `assets.pak` | The pack to write. |
| `--level <n>` | 19 | The zstd level, 1 to 22. |
| `--no-compress` | off | Stores every file as it is. |

Paths inside the pack are relative to the directory and sorted, so the same
directory gives the same file. Files whose names start with a dot are left
out. Images, audio and other formats that are already compressed are stored
without compression. Each entry carries a CRC-32 that is checked on every
read.

Put `assets.pak` beside the executable and the engine mounts it. To pack
from a build script or tool, call `yowie::asset::pack_dir(dir, out,
Compression::Zstd(19))`, or write entries one at a time with `PackWriter`.

### Example

`cargo run --example assets` loads a directory of images in the background
behind a progress bar, rewrites one image every two seconds so hot reload
replaces it, packs the directory into a pack file when you press P, and
counts its runs in a settings slot. `--dir <path>` picks the directory.

## Saves and settings

To keep data between runs, open a `yowie::save::Store` and write values to
named slots. A store is a directory; each slot is a file `<name>.json`
holding `{"version": N, "data": ...}`. Any type with serde's `Serialize`
and `Deserialize` can be a slot.

```rust,no_run
use serde::{Deserialize, Serialize};
use yowie::save::{SaveError, Store};

#[derive(Serialize, Deserialize, Default)]
struct Settings {
    volume: f32,
    fullscreen: bool,
}

#[derive(Serialize, Deserialize)]
struct Progress {
    level: u32,
    gold: u64,
}

fn main() -> Result<(), SaveError> {
    let store = Store::open("com", "Example", "Cave Crawler")?;

    // Settings: defaults until the slot exists.
    let mut settings: Settings = store.load_or("settings", Settings::default())?;
    settings.volume = 0.8;
    store.write("settings", &settings)?;

    // A save slot.
    store.write("slot-1", &Progress { level: 3, gold: 120 })?;
    let progress: Progress = store.read("slot-1")?;
    println!("level {} with {} gold", progress.level, progress.gold);
    println!("slots: {:?}", store.list()?);
    Ok(())
}
```

`Store::open(qualifier, organisation, application)` uses the platform's data
directory: `~/.local/share` on Linux, `~/Library/Application Support` on
macOS and `%APPDATA%` on Windows. `Store::at(dir)` uses a directory you
choose. Both create the directory.

| Call | What it does |
|---|---|
| `write(name, &value)` | Writes the slot and returns when it is on disk. |
| `write_async(name, &value)` | Encodes now, writes on the store's thread, and returns a `SaveHandle`. |
| `read(name)` | Reads the slot, upgrading older versions. `SaveError::NotFound` when it does not exist. |
| `load_or(name, default)` | Like `read`, returning `default` when the slot does not exist. Other errors are still returned. |
| `exists(name)`, `delete(name)`, `list()` | Checks, removes and lists slots. `list` is sorted. |
| `flush()` | Waits for queued background writes. |
| `path()` | The store's directory. |

Writes are atomic: the data goes to a temporary file that is synced and then
renamed over the slot, so a crash leaves the old save or the new one. Reads,
`write`, `exists`, `delete` and `list` wait for queued background writes
first, so they never see an older slot. Dropping the store also waits.

Slot names may not be empty, start with a dot, or contain `/ \ : * ? " < > |`
or control characters (`SaveError::InvalidName`).

### Saving without a stall

To save a large state without holding up a frame, call `write_async` and
check the handle in later updates:

```rust,no_run
use serde::Serialize;
use yowie::save::{SaveHandle, Store};

#[derive(Serialize)]
struct World {
    turn: u32,
}

struct Autosave {
    store: Store,
    pending: Option<SaveHandle>,
}

impl Autosave {
    fn start(&mut self, world: &World) {
        self.pending = Some(self.store.write_async("autosave", world));
    }

    /// Call once per update.
    fn check(&mut self) {
        let Some(handle) = &mut self.pending else { return };
        let failure = match handle.poll() {
            None => return,
            Some(Ok(())) => None,
            Some(Err(e)) => Some(e.to_string()),
        };
        self.pending = None;
        if let Some(reason) = failure {
            log::warn!("autosave failed: {reason}");
        }
    }
}
```

Writes run in the order they were requested.

### Versions and migration

To change a save format without breaking old saves, raise the store's
version and give it a migration hook. The hook receives the version the data
is at and the data as a `serde_json::Value`, and returns the data at the next
version. The store calls it once per step until the data reaches the current
version.

```rust,no_run
use serde_json::{Value, json};
use yowie::save::{SaveError, Store};

/// Upgrades save data by one version.
fn upgrade(version: u32, mut data: Value) -> Result<Value, String> {
    match version {
        // Version 1 had no gold.
        1 => {
            if let Some(fields) = data.as_object_mut() {
                fields.insert("gold".into(), json!(0));
            }
            Ok(data)
        }
        // Version 2 kept the level as text.
        2 => {
            let level = data["level"].as_str().and_then(|s| s.parse::<u64>().ok());
            let level = level.ok_or("level is not a number")?;
            data["level"] = json!(level);
            Ok(data)
        }
        _ => Ok(data),
    }
}

fn open_saves() -> Result<Store, SaveError> {
    Ok(Store::open("com", "Example", "Cave Crawler")?
        .with_version(3)
        .migrate_with(upgrade))
}
```

| Case | Result |
|---|---|
| Slot at the current version | Read as it is. |
| Slot at an older version, with a hook | Upgraded step by step, then read. |
| Slot at an older version, no hook | Read as it is. |
| A file without the `version` and `data` envelope | Treated as version 0. |
| Slot at a newer version | `SaveError::TooNew`. |
| The hook returns `Err(reason)` | `SaveError::Migration`. |

To save an entity world, store the string from `World::save_string` in a
slot, and pass the string you `read` back to `World::load_str`; see
[Entities and systems](ecs.md#saving-and-loading-a-world).

## Translation

To show text in the player's language, load a table per language into a
`yowie::locale::Bundle` and look up keys through a `Translator`.

```rust
use yowie::locale::{Args, Bundle, LocaleError};

const EN: &str = r#"{
  "menu": { "play": "Play", "quit": "Quit" },
  "found": "You found {item}.",
  "arrows": { "one": "{n} arrow", "other": "{n} arrows" }
}"#;

const FR: &str = r#"{
  "menu": { "play": "Jouer" },
  "arrows": { "one": "{n} flèche", "other": "{n} flèches" }
}"#;

fn main() -> Result<(), LocaleError> {
    let mut bundle = Bundle::new(["en"]);
    bundle.load_json("en", EN)?;
    bundle.load_json("fr", FR)?;

    let fr = bundle.translator("fr-CA");
    assert_eq!(fr.get("menu.play"), "Jouer");
    assert_eq!(fr.get("menu.quit"), "Quit");
    assert_eq!(fr.count("arrows", 0), "0 flèche");
    assert_eq!(fr.count("arrows", 2), "2 flèches");
    assert_eq!(fr.get_with("found", &Args::new().set("item", "a key")), "You found a key.");
    assert_eq!(fr.get("menu.options"), "[menu.options]");

    // Keys the French table still lacks.
    assert_eq!(bundle.missing("fr", "en"), ["found", "menu.quit"]);
    Ok(())
}
```

A table is a JSON object of strings. Nested objects make dotted keys:
`{"menu": {"play": "Play"}}` is `menu.play`. An object whose keys are all
plural categories (`zero`, `one`, `two`, `few`, `many`, `other`) is one
plural message.

`bundle.translator(lang)` tries `lang`, then its base language (`fr` for
`fr-CA`), then the fallbacks given to `Bundle::new` in order. A key that no
table has comes back as `[key]`, so missing text shows on screen.

| Call | Returns |
|---|---|
| `get(key)` | The message. |
| `get_with(key, &args)` | The message with `{name}` placeholders filled from `args`. |
| `count(key, n)` | The plural form for `n`, with `{n}` filled in. |

`Args::new().set(name, value)` fills a placeholder with any `Display` value.
`Args::num(name, value)` fills one with a number and also picks the plural
form; the number named `n` wins, otherwise the first number set. `{{` and
`}}` write literal braces, and a placeholder with no argument stays as
written.

Plural forms follow the CLDR rules for English, French, German, Spanish,
Italian, Portuguese, Russian, Polish, Ukrainian, Czech, Arabic, Hebrew,
Japanese, Chinese, Korean and Turkish, and a few related languages. Other
languages use the English rule. The rule comes from the language of the
table that holds the message.

The engine does not load tables for you. To load them from the assets and
follow the system language:

```rust,no_run
use yowie::Context;
use yowie::locale::Bundle;

fn load_text(ctx: &Context) -> Result<Bundle, Box<dyn std::error::Error>> {
    let mut bundle = Bundle::new(["en"]);
    for lang in ["en", "fr", "de"] {
        let json = ctx.assets.vfs().read_to_string(&format!("text/{lang}.json"))?;
        bundle.load_json(lang, &json)?;
    }
    Ok(bundle)
}
```

`bundle.system_translator()` returns a translator for the language the
operating system reports, and `yowie::locale::system_locale()` returns that
tag, such as `"en-US"`. A `Translator` borrows its `Bundle`; keep the
bundle and the chosen language tag in your game, and call
`bundle.translator(&lang)` where you draw text.

## Random numbers

To get random numbers that repeat for the same seed on every platform,
create a `yowie::rng::Rng`. It is a PCG32 generator, and it implements serde's
`Serialize` and `Deserialize`, so a saved game resumes the exact sequence.

```rust
use yowie::rng::{Dice, Rng};

#[derive(Debug)]
enum Loot {
    Coins,
    Potion,
    Sword,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut rng = Rng::new(2024);

    let face = rng.range(1, 6);
    assert!((1..=6).contains(&face));
    let heading = rng.between(0.0, std::f32::consts::TAU);
    assert!((0.0..std::f32::consts::TAU).contains(&heading));
    let damage = "2d6+1".parse::<Dice>()?.roll(&mut rng);
    assert!((3..=13).contains(&damage));

    let table = [(Loot::Coins, 70.0), (Loot::Potion, 25.0), (Loot::Sword, 5.0)];
    if let Some((item, _)) = rng.choose_weighted(&table, |(_, weight)| *weight) {
        println!("dropped {item:?}");
    }

    let mut deck: Vec<u32> = (1..=52).collect();
    rng.shuffle(&mut deck);

    // A separate generator for the level, so level generation does not
    // change the sequence the rest of the game sees.
    let mut level_rng = rng.fork();
    let rooms = level_rng.range(4, 9);
    assert!((4..=9).contains(&rooms));

    // Save and resume.
    let saved = serde_json::to_string(&rng)?;
    let mut resumed: Rng = serde_json::from_str(&saved)?;
    assert_eq!(resumed.next_u32(), rng.next_u32());
    Ok(())
}
```

| Call | Returns |
|---|---|
| `Rng::new(seed)`, `Rng::with_stream(seed, stream)` | A generator. Different streams with the same seed give independent sequences. |
| `fork()` | A new generator on its own stream, taken from this one. |
| `range(lo, hi)`, `range_i64(lo, hi)` | An integer in `lo..=hi`. |
| `below(n)` | An integer in `0..n`. |
| `float()`, `float64()` | A float in `[0, 1)`. |
| `between(lo, hi)`, `between64(lo, hi)` | A float in `[lo, hi)`. |
| `chance(p)` | `true` with probability `p`. |
| `roll(dice, sides)` | The sum of `dice` rolls of a `sides`-sided die. |
| `normal(mean, sd)`, `normal64(mean, sd)` | A normally distributed value. |
| `choose(&items)`, `choose_mut(&mut items)` | A uniformly chosen element, or `None` for an empty slice. |
| `weighted_index(&weights)`, `choose_weighted(&items, weight)` | A pick in proportion to weight. Weights that are zero, negative or NaN are never picked. |
| `shuffle(&mut items)` | Shuffles in place. |
| `state()`, `Rng::from_state(state, inc)` | The raw state, and a generator rebuilt from it. |

`Dice` parses expressions such as `3d6+2`, `d20` and `2D8-1`: one to 1000
dice of one to a million sides and a modifier. Its `min()` and `max()` give
the range of totals.

`Noise::new(seed)` is two-dimensional gradient noise for terrain and
textures. `get(x, y)` returns a smooth value in about -1 to 1,
`fbm(x, y, octaves, lacunarity, gain)` layers several frequencies (5, 2.0
and 0.5 are typical), and `ridged(...)` returns 0 to 1 with sharp crests.

## Timers and sequences

### Timers

To do something after a delay or at an interval of game time, use
`yowie::timer::Timers`. Each timer carries a value, usually an enum of game
events, and `update(dt)` returns the timers that fired.

```rust
use yowie::timer::{Timer, Timers};

#[derive(Clone, Debug, PartialEq)]
enum Cue {
    SpawnWave,
    Blink,
}

let mut timers = Timers::new();
timers.after(3.0, Cue::SpawnWave);
let blink = timers.every(0.5, Cue::Blink);

let fired = timers.update(1.2);
assert_eq!(fired.iter().filter(|f| f.data == Cue::Blink).count(), 2);

timers.cancel(blink);
let fired = timers.update(2.0);
assert_eq!(fired.len(), 1);
assert_eq!(fired[0].data, Cue::SpawnWave);
assert!(timers.is_empty());

// One timer on its own, such as a weapon cooldown.
let mut cooldown = Timer::once(0.25);
assert_eq!(cooldown.update(0.1), 0);
assert_eq!(cooldown.update(0.2), 1);
assert!(cooldown.finished());
```

`update` returns the firings in the order they happened, each with its
`id`, a clone of its `data`, and `at`, the seconds into this update at which
it fired. A repeating timer keeps the time left over, so it does not drift,
and fires as many times as the step covers. One-shot timers are removed
after they fire. `pause(id)`, `resume(id)`, `cancel(id)` and `clear()`
control timers by id or all at once.

Times are `f64` seconds. Pass the update step as
`f64::from(ctx.time.delta())`.

### Sequences

To script a cutscene or an intro, build a `Sequence` of steps and call
`update(dt)` until it returns `true`:

| Step | Runs |
|---|---|
| `wait(seconds)` | Waits. |
| `call(f)` | Calls `f` once. |
| `wait_until(cond)` | Waits until `cond()` returns `true`, checked each update. |
| `run(f)` | Calls `f(dt)` each update until it returns `true`. |
| `parallel(vec![...])` | Runs several sequences at once, until all are done. |
| `looped()` | Starts again from the first step after the last; never done. |

Steps must be `Send` and `'static`, so they cannot borrow your game or hold
an `Rc`. To get results out, send them through a channel and read it in
`update`:

```rust
use std::sync::mpsc;
use yowie::timer::Sequence;

#[derive(Debug, PartialEq)]
enum Beat {
    OpenDoor,
    ShowTitle,
    FadeIn,
}

let (tx, rx) = mpsc::channel();
let (door, title) = (tx.clone(), tx.clone());
let mut intro = Sequence::new()
    .wait(1.0)
    .call(move || {
        let _ = door.send(Beat::OpenDoor);
    })
    .wait(0.5)
    .parallel(vec![
        Sequence::new().call(move || {
            let _ = title.send(Beat::ShowTitle);
        }),
        Sequence::new().wait(2.0).call(move || {
            let _ = tx.send(Beat::FadeIn);
        }),
    ]);

assert!(!intro.update(1.2));
assert_eq!(rx.try_recv(), Ok(Beat::OpenDoor));
assert!(!intro.update(1.0));
assert_eq!(rx.try_recv(), Ok(Beat::ShowTitle));
assert!(intro.update(2.0));
assert_eq!(rx.try_recv(), Ok(Beat::FadeIn));
```

Time left over when a step ends carries into the next step, so one long
update can run several steps. `skip()` marks the sequence done without
running the rest, and `reset()` goes back to the first step.

## Tweens

To animate a value from one end to another, create a `yowie::tween::Tween`
and call `update(dt)` each update. It returns the current value.

```rust
use yowie::tween::{Curve, Ease, Repeat, Tween};
use yowie::{Color, vec2};

// Slide a panel in from the left over half a second.
let mut slide = Tween::new(vec2(-300.0, 40.0), vec2(20.0, 40.0), 0.5).ease(Ease::Out(Curve::Cubic));
slide.update(0.25);
assert!(slide.value().x > -140.0);
assert_eq!(slide.update(0.25), vec2(20.0, 40.0));
assert!(slide.done());

// Pulse a scale up and down forever, after a short delay.
let mut pulse = Tween::new(1.0_f32, 1.2, 0.3)
    .ease(Ease::InOut(Curve::Sine))
    .delay(0.1)
    .repeat(Repeat::Forever)
    .yoyo();
assert_eq!(pulse.update(0.05), 1.0);
assert!(!pulse.update(10.0).is_nan());
assert!(!pulse.done());

// Flash red, then fade back to white.
let mut flash = Tween::new(Color::WHITE, Color::RED, 0.1).then(Tween::new(Color::RED, Color::WHITE, 0.4));
flash.update(1.0);
assert!(flash.done());
assert_eq!(flash.value(), Color::WHITE);
```

| Builder | Effect |
|---|---|
| `ease(Ease)` | The easing curve. The default is `Ease::Linear`. |
| `delay(seconds)` | Waits before the first play. |
| `repeat(Repeat::Times(n))`, `repeat(Repeat::Forever)` | Plays `n` more times, or until reset. |
| `yoyo()` | Plays every second repeat backwards. |
| `then(next)` | Returns a `Chain` that plays `next` when this one ends. |

`value()` reads the value without advancing, `progress()` returns the
eased progress of the current play, `done()` reports the end of the last
play, and `reset()` starts again with the delay. Time left after a tween
ends passes to the next tween of a `Chain`.

| `Ease` | Shape |
|---|---|
| `Linear` | No easing. |
| `Smoothstep` | Slow at both ends. |
| `Steps(n)` | Jumps through `n` equal levels. |
| `Bezier(x1, y1, x2, y2)` | A CSS-style cubic Bézier. |
| `Custom(fn(f32) -> f32)` | Your function of progress from 0 to 1. |
| `In(curve)`, `Out(curve)`, `InOut(curve)` | Slow start, slow end, or both, with `Curve::Quad`, `Cubic`, `Quart`, `Quint`, `Sine`, `Expo`, `Circ`, `Back`, `Elastic` or `Bounce`. |

`Back` and `Elastic` go past the ends on the way. Tweens work on any type
with the `Lerp` trait: `f32`, `f64`, `Vec2`, `Vec3`, `Vec4`, `Quat`
(spherical), `Color` (per channel in sRGB), `Transform` and `Transform2D`.
Implement `Lerp` for your own types. Tween times are `f32` seconds; pass
`ctx.time.delta()`.

## Grids, paths and field of view

`yowie::grid` works on cell grids with `IVec2` positions: `x` to the right,
`y` down, `(0, 0)` at the top left. Pathfinding and distance maps take a
cost function `cost(from, to)` that returns the price of stepping onto
`to`. Return `BLOCKED` (or any negative or NaN value) to forbid the step.

```rust
use yowie::grid::{
    BLOCKED, Diagonal, Grid, Neighbourhood, Pathfinder, dijkstra, downhill, flood_fill, fov_cells,
    line,
};
use yowie::{IVec2, ivec2};

const MAP: [&str; 5] = [
    "#########",
    "#...#...#",
    "#.#.#.#.#",
    "#.#...#.#",
    "#########",
];

fn main() {
    let walls = Grid::from_fn(9, 5, |p: IVec2| MAP[p.y as usize].as_bytes()[p.x as usize] == b'#');
    let cost = |_from: IVec2, to: IVec2| if walls.get(to) == Some(&false) { 1.0 } else { BLOCKED };

    // A* from the top left room to the top right one.
    let mut finder = Pathfinder::new(9, 5).with_min_cost(1.0);
    let path = finder.find(ivec2(1, 1), ivec2(7, 1), Diagonal::Never, cost);
    assert_eq!(path.map(|p| p.cost), Some(10.0));

    // A Dijkstra map toward the exit, followed downhill.
    let to_exit = dijkstra(9, 5, &[ivec2(7, 1)], Diagonal::Never, cost);
    assert_eq!(to_exit.get(ivec2(1, 1)), Some(&10.0));
    assert_eq!(downhill(&to_exit, ivec2(1, 1), Neighbourhood::Four), Some(ivec2(2, 1)));

    // What a player at (3, 3) sees. Cells outside the grid count as walls.
    let seen = fov_cells(ivec2(3, 3), 8, |p| walls.get(p) != Some(&false));
    assert!(seen.contains(&ivec2(6, 3)));
    assert!(!seen.contains(&ivec2(7, 3)));

    // Every floor cell connected to (1, 1).
    let floor = flood_fill(9, 5, ivec2(1, 1), Neighbourhood::Four, |p| walls.get(p) == Some(&false));
    assert_eq!(floor.len(), 15);

    assert_eq!(line(ivec2(0, 0), ivec2(4, 2)).len(), 5);
}
```

### Grid

`Grid<T>` stores one value per cell. Create it with `Grid::new(w, h)`
(default values), `Grid::filled(w, h, value)` or `Grid::from_fn(w, h, f)`.
`get` and `get_mut` return `None` outside the grid, `set` returns `false`
there, `iter` visits every cell with its position, and `neighbours(p,
Neighbourhood::Four)` or `Eight` lists the neighbours inside the grid.
`paint(start, value, hood)` flood-fills the connected region of equal cells.

### Paths

| Call | Returns |
|---|---|
| `astar(w, h, start, goal, rule, cost)` | The least-cost `Path` (`points` from start to goal, and `cost`), or `None`. |
| `Pathfinder::new(w, h).find(start, goal, rule, cost)` | The same, reusing its memory between searches. Keep one per grid size. |
| `dijkstra(w, h, &sources, rule, cost)` | A `Grid<f32>` of the cost from the nearest source to each cell. Unreachable cells hold `BLOCKED`. |
| `downhill(&map, p, hood)` | The neighbour with the lowest value below `p`'s own. Repeat it to walk to the nearest source. |
| `flee_map(&map, rule, cost, 1.2)` | A map whose downhill direction leads away from the sources, avoiding dead ends. |

`Diagonal` decides whether paths step diagonally:

| Rule | Diagonal steps |
|---|---|
| `Never` | None. |
| `Always` | Always, even between two blocked cells. |
| `IfBothClear` | Only when both cells beside the step are open, so paths do not cut corners. |
| `IfEitherClear` | When at least one of them is open. |

A diagonal step costs what the cost function returns for its target, the
same as a straight step. To charge more for diagonals, compare `from` and
`to` in the cost function.

`with_min_cost(c)` tells the search that no step costs less than `c`, which
directs it toward the goal and makes it faster. Leave it at the default of
0 when steps can be free; a value above the cheapest step can return longer
paths.

One Dijkstra map works for many walkers. Build one from the player's cell,
and every monster follows `downhill` toward the player. Build one from every
exit, and each walker finds the nearest exit.

### Lines, field of view and regions

| Call | Returns |
|---|---|
| `line(a, b)` | The cells of the Bresenham line, both ends included. |
| `supercover_line(a, b)` | Every cell the segment between the cell centres touches. Use it to test whether a shot or a sight line passes. |
| `fov(origin, radius, opaque, visit)` | Calls `visit` for each cell visible within `radius`, using symmetric shadowcasting. |
| `fov_cells(origin, radius, opaque)` | The visible cells, origin first. |
| `flood_fill(w, h, start, hood, passable)` | The cells connected to `start`, in breadth-first order. Empty when `start` is not passable. |

In `fov`, `opaque(p)` is called for cells outside the grid too, so return
`true` there. Walls are visible, the origin is always visible, and if one
floor cell sees another, the second sees the first.

### Examples

- `cargo run --example pathfinding`: paint walls with the left mouse button
  and erase with the right, move the start with the middle button, and watch
  A*, a Dijkstra map, field of view and flood fill update. D toggles
  diagonal steps, R makes a random map, F5 saves the map and F9 loads it.
- `cargo run --example roguelike`: a turn-based dungeon with field of view
  and monsters that chase with `Pathfinder`. Move with the arrow keys, HJKL,
  YUBN or the numpad; `.` waits. `--autoplay` plays itself.

## Networking

`yowie::net` sends typed messages between a server and its clients. A
message is any type with serde's `Serialize` and `Deserialize`. Each
server and client runs its socket on background threads; the game calls
`poll()` once per update to collect events without blocking. The module is
part of the default `net` feature.

| Transport | Types | Delivery |
|---|---|---|
| TCP | `TcpServer`, `TcpClient` | Ordered and reliable. Optional TLS. |
| UDP | `UdpServer`, `UdpClient` | Per message: unreliable, sequenced or reliable and ordered. Round-trip and loss figures. |

### TCP

```rust,no_run
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use yowie::net::{ClientEvent, ServerEvent, TcpClient, TcpConfig, TcpServer};
use yowie::{Context, Waker};

/// What clients send.
#[derive(Serialize, Deserialize)]
enum Up {
    Say(String),
}

/// What the server sends.
#[derive(Serialize, Deserialize)]
enum Down {
    Said { from: String, text: String },
}

/// Wakes the engine's loop when a message arrives.
fn waker_fn(waker: &Waker) -> Arc<dyn Fn() + Send + Sync> {
    let waker = waker.clone();
    Arc::new(move || waker.wake())
}

fn host(ctx: &Context) -> yowie::net::Result<TcpServer<Up, Down>> {
    let server = TcpServer::listen("0.0.0.0:7777", TcpConfig::default())?;
    server.set_waker(waker_fn(&ctx.waker()));
    Ok(server)
}

fn join(ctx: &Context, address: &str) -> yowie::net::Result<TcpClient<Down, Up>> {
    let client = TcpClient::connect(address, TcpConfig::default())?;
    client.set_waker(waker_fn(&ctx.waker()));
    Ok(client)
}

/// Server side, once per update.
fn serve(server: &TcpServer<Up, Down>) {
    while let Some(event) = server.poll() {
        match event {
            ServerEvent::Connected { id, addr } => log::info!("{id} joined from {addr}"),
            ServerEvent::Disconnected { id, reason } => log::info!("{id} left: {reason:?}"),
            ServerEvent::Message { id, message: Up::Say(text) } => {
                let _ = server.broadcast(&Down::Said { from: id.to_string(), text });
            }
        }
    }
}

/// Client side, once per update.
fn listen(client: &TcpClient<Down, Up>, chat: &mut Vec<String>) {
    while let Some(event) = client.poll() {
        match event {
            ClientEvent::Connected => chat.push("Connected".into()),
            ClientEvent::Disconnected(reason) => chat.push(format!("Disconnected: {reason:?}")),
            ClientEvent::Message(Down::Said { from, text }) => chat.push(format!("{from}: {text}")),
        }
    }
}
```

`TcpServer<In, Out>` receives `In` and sends `Out`; a client is the other
way round, `TcpClient<In, Out>` with the server's `Out` as its `In`.
`TcpClient::connect` returns at once. The connection is made on a background
thread and reported as `ClientEvent::Connected` or `Disconnected`, and
messages sent before then wait in the queue.

| Server call | What it does |
|---|---|
| `listen(addr, config)` | Listens. Port 0 picks a free port; `local_addr()` returns it. |
| `send(id, &msg)` | Queues a message for one client. |
| `broadcast(&msg)` | Queues it for every client. Returns the ids whose queues were full. |
| `broadcast_except(&msg, Some(id))` | Every client but one. |
| `clients()`, `client_addr(id)` | The connected clients and their addresses. |
| `disconnect(id)` | Closes a connection after its queued messages are written. |
| `shutdown()` | Closes everything. Dropping the server does the same. |

`TcpConfig` defaults: messages up to 1 MiB, 256 queued sends per
connection, 64 clients, 5 s to connect, 10 s for a TLS handshake and for a
blocked write.

For TLS, build a `TlsServer` from a PEM certificate chain and key
(`TlsServer::from_pem_files(cert, key)`) and call `TcpServer::listen_tls`.
On the client, `TlsClient::system_roots(host)` checks the certificate
against the system's trusted roots, `with_roots_pem` against your own
authority, and `pinned_pem` accepts exactly one certificate. Pass it to
`TcpClient::connect_tls`.

### UDP

The UDP types add a handshake, acknowledgements, resends and keepalives on
top of UDP, and choose the delivery per message with a `Channel`:

| Channel | Delivery | Size |
|---|---|---|
| `Unreliable` | Sent once. May be lost or arrive out of order. | One datagram, under 1200 bytes. |
| `Sequenced` | Sent once. A message older than one already delivered is dropped. | One datagram. |
| `Reliable` | Resent until acknowledged, delivered once and in order. | Up to `UdpConfig::max_message`, split and rejoined. |

`UdpServer::bind(addr, config)` and `UdpClient::connect(addr, config)`
create the ends, and `send(id, channel, &msg)` (server) or
`send(channel, &msg)` (client) send. Events are the same `ServerEvent` and
`ClientEvent` types as for TCP. `stats()` returns the smoothed round trip,
the share of packets lost and the reliable fragments waiting for
acknowledgement. A connection that receives no packets for 5 seconds is
dropped; idle connections send a keepalive every 500 ms.

Use `Sequenced` for state that the next message replaces, such as positions,
and `Reliable` for events that must arrive, such as chat or a door opening.

To test behaviour under a bad connection, wrap a socket in
`yowie::net::udp::LossySocket` with a `LossConfig` of drop rate, delay and
jitter, and pass it to `UdpServer::with_socket` or `UdpClient::with_socket`.

### Waking a turn-based loop

In `LoopMode::TurnBased` the engine sleeps until input arrives. To wake it
when network traffic arrives, give each server and client a waker built
from `ctx.waker()`, as `waker_fn` does above. The callback runs on a
network thread; it must return quickly. See
[The window](window.md#the-loop).

### Helpers for networked games

`yowie::net::game` holds the bookkeeping that real-time network games need.
None of it does I/O; times are `f64` seconds on a timeline you choose.

| Type | Use |
|---|---|
| `Interpolator<T>` | Buffers timestamped snapshots of a remote object and samples between them, so it moves smoothly between updates. Sample one or two snapshot intervals in the past. |
| `Predictor<S, I, F>` | Applies the local player's inputs at once, then adopts the server's state and replays the inputs the server has not seen yet. |
| `Clock` | Estimates the server's clock from ping replies. |
| `History<T>` | Keeps past states for a time window, for checking a shot against where a target was. |
| `SnapshotBuffer`, `SnapshotReceiver` | Encode each client's snapshots as a difference from the last one it acknowledged, and rebuild them. |
| `InterestGrid`, `Interest` | Find the objects near each viewer, and report which entered and left its range. |

```rust
use yowie::net::game::{Interpolator, Predictor};
use yowie::{Vec2, vec2};

fn step(position: &Vec2, input: &Vec2) -> Vec2 {
    *position + *input
}

// Client prediction: move at once, then correct.
let mut me = Predictor::new(Vec2::ZERO, step);
let first = me.apply(vec2(1.0, 0.0));
me.apply(vec2(1.0, 0.0));
assert_eq!(*me.state(), vec2(2.0, 0.0));

// The server processed the first input and placed the player at (0.5, 0).
me.reconcile(first, vec2(0.5, 0.0));
assert_eq!(*me.state(), vec2(1.5, 0.0));
assert_eq!(me.pending(), 1);

// Another player, from snapshots taken at 1 s and 2 s.
let mut other = Interpolator::new(8);
other.push(1.0, vec2(0.0, 0.0));
other.push(2.0, vec2(10.0, 0.0));
assert_eq!(other.sample(1.5), Some(vec2(5.0, 0.0)));
```

The `step` function of a `Predictor` must give the same result on the
client and the server.

### Example

`cargo run --example network -- --listen :7777` starts a chat host, and
`cargo run --example network -- --join localhost:7777` joins it. Chat lines
go over TCP and pointer positions over sequenced UDP. `--reliable` sends the
chat over reliable UDP and shows the link's round trip and loss,
`--name <n>` sets your name, and `--bot` sends a greeting once connected.
Both ends use the turn-based loop and wake on traffic.

For the API reference, run `cargo doc --open` and look under `yowie_asset`,
`yowie_save`, `yowie_locale`, `yowie_rng`, `yowie_timer`, `yowie_tween`,
`yowie_grid` and `yowie_net`.
