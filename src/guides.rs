//! Compiles the Rust code blocks in `README.md` and `docs/guides/*.md` as
//! doctests, so the guides stay in step with the API. This module exists
//! only under `cargo test --doc`.
//!
//! Every fenced block in those files needs a language tag: rustdoc compiles
//! untagged blocks as Rust. Use `text` for output and plain listings.

/// `README.md`.
#[doc = include_str!("../README.md")]
pub struct Readme;

/// `docs/guides/introduction.md`.
#[doc = include_str!("../docs/guides/introduction.md")]
pub struct Introduction;

/// `docs/guides/why-rust.md`.
#[doc = include_str!("../docs/guides/why-rust.md")]
pub struct WhyRust;

/// `docs/guides/getting-started.md`.
#[doc = include_str!("../docs/guides/getting-started.md")]
pub struct GettingStarted;

/// `docs/guides/api-design.md`.
#[doc = include_str!("../docs/guides/api-design.md")]
pub struct ApiDesign;

/// `docs/guides/tetris.md`.
#[doc = include_str!("../docs/guides/tetris.md")]
pub struct Tetris;

/// `docs/guides/window.md`.
#[doc = include_str!("../docs/guides/window.md")]
pub struct Window;

/// `docs/guides/input.md`.
#[doc = include_str!("../docs/guides/input.md")]
pub struct Input;

/// `docs/guides/ecs.md`.
#[doc = include_str!("../docs/guides/ecs.md")]
pub struct Ecs;

/// `docs/guides/services.md`.
#[doc = include_str!("../docs/guides/services.md")]
pub struct Services;

/// `docs/guides/console.md`.
#[doc = include_str!("../docs/guides/console.md")]
pub struct Console;

/// `docs/guides/graphics-2d.md`.
#[doc = include_str!("../docs/guides/graphics-2d.md")]
pub struct Graphics2d;

/// `docs/guides/graphics-3d.md`.
#[doc = include_str!("../docs/guides/graphics-3d.md")]
pub struct Graphics3d;

/// `docs/guides/shaders.md`.
#[doc = include_str!("../docs/guides/shaders.md")]
pub struct Shaders;

/// `docs/guides/animation.md`.
#[doc = include_str!("../docs/guides/animation.md")]
pub struct Animation;

/// `docs/guides/ui.md`.
#[doc = include_str!("../docs/guides/ui.md")]
pub struct Ui;

/// `docs/guides/audio.md`.
#[doc = include_str!("../docs/guides/audio.md")]
pub struct Audio;

/// `docs/guides/physics.md`.
#[doc = include_str!("../docs/guides/physics.md")]
pub struct Physics;

/// `docs/guides/space.md`.
#[doc = include_str!("../docs/guides/space.md")]
pub struct Space;
