//! Eased value animation.
//!
//! - [`Ease`] maps linear progress (0 to 1) to eased progress: the Penner
//!   family, smoothstep, steps, cubic Béziers and custom functions.
//! - [`Lerp`] interpolates a value type. It is implemented for `f32`,
//!   `f64`, `Vec2`, `Vec3`, `Vec4`, `Quat` (spherical), `Color`,
//!   `Transform` and `Transform2D`.
//! - [`Tween`] animates from one value to another over time with delay,
//!   repeat and yoyo; [`Chain`] plays tweens one after another.
//!
//! Tweens run on the `dt` you pass to `update`, in seconds.
//!
//! ```
//! use yowie_tween::{Curve, Ease, Tween};
//!
//! let mut t = Tween::new(0.0_f32, 10.0, 2.0).ease(Ease::Out(Curve::Quad));
//! assert_eq!(t.update(2.0), 10.0);
//! assert!(t.done());
//! ```

#![warn(missing_docs)]

mod chain;
mod ease;
mod lerp;
mod tween;

pub use chain::Chain;
pub use ease::{Curve, Ease};
pub use lerp::Lerp;
pub use tween::{Repeat, Tween};
