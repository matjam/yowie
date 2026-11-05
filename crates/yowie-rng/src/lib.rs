//! Seeded PCG32 random numbers with forks, dice, weighted picks and
//! shuffles.
//!
//! [`Rng`] is a PCG32 (XSH RR) generator. The same seed and stream always
//! produce the same sequence on every platform, and the generator state
//! implements `serde::Serialize` and `Deserialize`, so a saved game can
//! resume a sequence exactly.
//!
//! ```
//! use yowie_rng::{Dice, Rng};
//!
//! let mut rng = Rng::new(42);
//! let damage = "3d6+2".parse::<Dice>().unwrap().roll(&mut rng);
//! assert!((5..=20).contains(&damage));
//! ```

#![warn(missing_docs)]

mod dice;
mod error;
mod noise;
mod pcg;
mod pick;

pub use dice::Dice;
pub use error::RngError;
pub use noise::Noise;
pub use pcg::Rng;
