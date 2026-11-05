# Yowie for language models

Yowie is a Rust game library for real-time and turn-based games that put
2D sprites and 3D models on one screen. Its feature set follows Bunyip
(`~/src/bunyip`, a Go engine by the same author); its API does not. When a
feature needs a reference implementation, read the matching Bunyip package
(its Go source and `docs/guides/*.md`) for behaviour and algorithms, then
write idiomatic Rust.

Programs must build with plain `cargo build` on Linux (native Wayland, X11
fallback), macOS and Windows.

## Layout

| Path | What |
|---|---|
| `src/` | The root crate `yowie`: run loop, `Config`, `Game`, `Context`, headless mode; re-exports every crate below as a module |
| `crates/yowie-core` | glam re-exports, `Color`, `Rect`, `Transform`, `Transform2D` |
| `crates/yowie-input` | key, mouse and gamepad types, per-update `InputState`, action maps |
| `crates/yowie-gfx` | wgpu renderer: 2D, text, vector paths, 3D, post-processing |
| `crates/yowie-ui`, `crates/yowie-console` | immediate-mode widgets; in-game debug console |
| `crates/yowie-ecs` | systems, resources, events, hierarchy, saves, prefabs, scenes over hecs |
| `crates/yowie-phys` | rapier-backed 2D and 3D rigid bodies; `soft` module for XPBD cloth, soft bodies and fluids |
| `crates/yowie-anim` | curves, clips, players, skeletons, blend spaces, IK, morph weights |
| `crates/yowie-audio` | cpal output and capture, mixer, decoding, spatial audio, `tracker` module |
| `crates/yowie-particle`, `crates/yowie-tiled` | particles; Tiled maps |
| `crates/yowie-asset`, `crates/yowie-save`, `crates/yowie-locale` | files, packs, hot reload; saves; translation |
| `crates/yowie-rng`, `crates/yowie-timer`, `crates/yowie-tween`, `crates/yowie-grid`, `crates/yowie-orbit`, `crates/yowie-net` | small services |
| `examples/` | runnable programs: `cargo run --example <name>`; assets under `examples/assets/` |

Dependency versions live once in the root `Cargo.toml` under
`[workspace.dependencies]`; crates use `dep.workspace = true`.

## Rules

- Edition 2024. `cargo fmt --all` and
  `cargo clippy --workspace --all-targets -- -D warnings` pass on every commit.
- Each crate's `lib.rs` has `#![warn(missing_docs)]`. Public items carry `///`
  docs that say what the item does and any contract or unit (pixels, radians,
  seconds, sRGB or linear) a caller needs.
- Keep files under about 300 lines; split past 500. One concern per file.
- Errors: one `thiserror` enum per crate. No `unwrap`, `expect` or `panic!`
  in library code unless an invariant makes failure impossible, and then a
  comment says which invariant.
- Tests sit beside the code (`#[cfg(test)] mod tests`), table-driven where
  there are several cases. GPU tests create a headless wgpu device and skip
  with a printed message when no adapter exists.
- Linux windowing is native Wayland through winit with X11 as fallback. Never
  force XWayland (`WINIT_UNIX_BACKEND=x11` or similar) as a fix or default.
- Logging goes through the `log` crate; the engine installs a logger, crates
  never do.
- No TODO comments; open questions go in the commit message or the report.

## Platforms

Development happens on macOS, and only macOS is built and tested locally.
Do not cross-compile or cross-check Windows or Linux. Write the code so they
work: portable code, platform-specific code behind `cfg`, required system
packages documented. Do not change dependencies or design to make a local
cross-check pass. Windows and Linux are tested on those platforms later
(CI runs on all three).

## Examples

Every example accepts `--shot <file.png>` (render, save the frame, exit),
`--seconds <n>` (run for n seconds of game time, then exit) and honours
`YOWIE_FIXED_CLOCK=1` (advance time by exactly one fixed step per frame), so
it can verify itself headless. The engine parses these; examples do not.

The library is not finished until every one of these exists, each the
counterpart of the Bunyip example of the same name (see Bunyip's README
table for what each shows): animation, assets, audio, autotile, clear,
gallery, inputs, lighting, materials, network, particles, pathfinding,
physics_lab, physics2d, physics3d, probes, roguelike, shaders, softbody,
solar, space, sprites, terrain, tetris, text, tiled, tiles, vector, viewer,
window. Whoever builds a feature writes the examples that show it, with a
golden image in `examples/testdata/` when the output is deterministic.

## Prose

Documentation and comments state facts and instructions in plain technical
English: "To draw a sprite, call `Graphics::draw`." No metaphors, no
em-dashes, no filler adverbs.

## Commits

Author as `578676+matjam@users.noreply.github.com`. Imperative subject under
72 characters saying what changed and why.
