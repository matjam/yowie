//! Helpers for networked games: snapshot interpolation, client-side
//! prediction with server reconciliation, lag compensation, clock
//! synchronisation, snapshot delta encoding and interest management.
//!
//! Times are `f64` seconds on whatever timeline the caller chooses (usually
//! the server's, via [`Clock`]). None of these types performs I/O.

mod clock;
mod delta;
mod history;
mod interest;
mod interp;
mod predict;
mod snapshot;

pub use clock::Clock;
pub use delta::{apply_delta, decode_delta, diff_bytes, encode_delta};
pub use history::History;
pub use interest::{Interest, InterestGrid};
pub use interp::{Interpolator, Lerp};
pub use predict::Predictor;
pub use snapshot::{SnapshotBuffer, SnapshotReceiver};
