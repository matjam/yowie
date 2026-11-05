//! Keyframe curves and clips for sprites, transforms and any value;
//! players with crossfades and events, flipbooks, skeletons with layers,
//! blend spaces, root motion, inverse kinematics and morph weights.
//!
//! The crate has no GPU dependency. A typical use:
//!
//! ```
//! use std::sync::Arc;
//! use yowie_anim::{Channel, Clip, Curve, LoopMode, Player, Value};
//! use yowie_core::Vec2;
//!
//! let clip = Clip::new("slide")
//!     .with_mode(LoopMode::Once)
//!     .with_track("pos", Channel::Vec2(Curve::linear([
//!         (0.0, Vec2::ZERO),
//!         (2.0, Vec2::new(10.0, 0.0)),
//!     ])));
//! let mut player = Player::new();
//! player.play(Arc::new(clip));
//! player.advance(1.0);
//! player.apply(&mut |name: &str, value: &Value| {
//!     assert_eq!(name, "pos");
//!     assert_eq!(*value, Value::Vec2(Vec2::new(5.0, 0.0)));
//! });
//! ```
//!
//! Clips, curves and blend trees implement `serde::Serialize` and
//! `Deserialize`, so they can be defined in data files.

#![warn(missing_docs)]

mod animatable;
pub mod blend;
pub mod chain;
mod clip;
mod curve;
mod error;
pub mod flipbook;
pub mod ik;
pub mod ik_pose;
pub mod layers;
pub mod morph;
mod playback;
mod player;
mod pose;
pub mod root_motion;
mod skeleton;
pub mod timeline;
mod value;

#[cfg(feature = "gltf")]
pub mod gltf_import;

pub use animatable::Animatable;
pub use blend::{Blend, BlendNode, BlendSpace1D, BlendSpace2D, ClipSet};
pub use clip::{Clip, ClipEvent, Track};
pub use curve::{Curve, Interp, Key, Tangents};
pub use error::{AnimError, Result};
pub use flipbook::{Flipbook, FlipbookPlayer, TagDirection};
pub use layers::{JointMask, Layer, LayerMode};
pub use playback::PlayerEvent;
pub use player::Player;
pub use pose::Pose;
pub use root_motion::RootMotion;
pub use skeleton::{Joint, Skeleton};
pub use timeline::LoopMode;
pub use value::{Channel, Target, Value};
