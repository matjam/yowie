//! Tracker music: ProTracker MOD (4 to 32 channels), ScreamTracker 3 S3M, FastTracker 2
//! XM and Impulse Tracker IT.
//!
//! [`Player`] implements [`Stream`](crate::Stream), so a module plays through the mixer
//! as any other voice does. The four formats share one [`Module`] model, and
//! [`Module::format`] and the flags the loaders set select the format-specific behaviour
//! (period tables, slide units, effect semantics).
//!
//! To load a module, call [`Module::load`], which detects the format; [`load_mod`],
//! [`load_s3m`], [`load_xm`] and [`load_it`] each read one format. A module carries the
//! song's name, orders, patterns, samples and instruments, so a game can show what is
//! playing or drive visuals from the pattern data. Playback is deterministic.
//!
//! ```no_run
//! use std::sync::Arc;
//! use yowie_audio::tracker::{Module, Player, PlayerOptions};
//! use yowie_audio::{Mixer, PlayOptions};
//!
//! # fn run(mixer: &Mixer, bytes: &[u8]) -> yowie_audio::Result<()> {
//! let module = Arc::new(Module::load(bytes)?);
//! let player = Player::with_options(module, mixer.rate(), PlayerOptions { looping: true, ..Default::default() });
//! let control = player.control();
//! mixer.play_stream(player, PlayOptions::default());
//! control.mute(3, true);
//! # Ok(())
//! # }
//! ```

mod bytes;
mod effects;
mod envelope;
mod it_decompress;
mod load_it;
mod load_mod;
mod load_s3m;
mod load_xm;
mod module;
mod player;
mod render;
mod row;
mod row_effects;
mod state;
mod tables;
#[cfg(test)]
mod test_modules;
mod tick;

#[cfg(test)]
mod tests;

pub use effects::{Effect, VolCmd};
pub use load_it::load_it;
pub use load_mod::load_mod;
pub use load_s3m::load_s3m;
pub use load_xm::load_xm;
pub use module::{
    AutoVibrato, Cell, EnvPoint, Envelope, Format, Instrument, LoopType, Module, NOTE_CUT,
    NOTE_FADE, NOTE_NONE, NOTE_OFF, Nna, Pattern, Sample,
};
pub use player::{Player, PlayerControl, PlayerOptions};

use crate::error::{Error, Result};

impl Module {
    /// Detects the format of `data` and parses it.
    pub fn load(data: &[u8]) -> Result<Module> {
        if data.len() > 48 && &data[44..48] == b"SCRM" {
            load_s3m(data)
        } else if data.starts_with(b"Extended Module: ") {
            load_xm(data)
        } else if data.starts_with(b"IMPM") {
            load_it(data)
        } else {
            load_mod(data).map_err(|e| Error::Module(format!("not a recognised module: {e}")))
        }
    }
}
