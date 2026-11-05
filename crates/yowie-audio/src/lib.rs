//! A mixer with buses, streamed WAV, Ogg Vorbis, MP3 and FLAC, positional
//! voices with panning or a binaural head model, reverb, microphone capture,
//! tone synthesis, and a MOD, S3M, XM and IT player in [`tracker`].
//!
//! # Model
//!
//! A [`Mixer`] is a handle. Calls on it and on the [`Voice`] and [`Bus`] handles it
//! hands out queue messages for the audio thread, which applies them at the start of
//! its next chunk of 256 frames. The audio thread never waits for the game and never
//! allocates in steady state; everything that allocates is built before it is sent,
//! and finished voices are sent back to be dropped on the game thread by
//! [`Mixer::update`].
//!
//! To play through the default device:
//!
//! ```no_run
//! use yowie_audio::{Output, Pcm, PlayOptions};
//!
//! # fn main() -> yowie_audio::Result<()> {
//! let output = Output::open_default()?;
//! let mixer = output.mixer();
//! let beep = mixer.sound(&Pcm::sine(440.0, 0.5, 44_100))?;
//! mixer.play(&beep, PlayOptions { volume: 0.5, ..Default::default() });
//! # Ok(())
//! # }
//! ```
//!
//! To mix without a device, for tests or for rendering to a file, make the mixer with
//! [`Mixer::offline`] and call [`Renderer::render`].
//!
//! # Building on Linux
//!
//! The ALSA backend of `cpal` needs the ALSA development files. On Debian and Ubuntu,
//! install `libasound2-dev`; on Fedora, `alsa-lib-devel`. macOS and Windows need nothing.
//!
//! # Contents
//!
//! * Mixer, voices and buses: [`Mixer`], [`Voice`], [`Bus`], [`PlayOptions`].
//! * Sources: [`Sound`] (in memory), [`Stream`] and [`Music`] (decoded on a background thread),
//!   [`decode`] for WAV, Ogg Vorbis, MP3 and FLAC, [`synth`] for generated tones.
//! * World: [`Listener`], [`Positional`], [`SpatialSettings`] (stereo or binaural),
//!   [`ReverbSettings`], [`ReverbZone`].
//! * Devices: [`Output`], [`list_outputs`], [`Capture`], [`Recorder`].
//! * Tracker music: [`tracker`].

#![warn(missing_docs)]

mod binaural;
mod bus;
mod command;
mod core;
pub mod decode;
mod dsp;
mod error;
mod handle;
mod mix;
mod mixer;
mod reverb;
mod sound;
mod spatial;
mod stream;
#[cfg(test)]
mod tests;
mod voice;
mod wav;

pub use bus::{Bus, MAX_BUSES};
pub use error::{Error, Result};
pub use handle::Voice;
pub use mixer::{Mixer, Renderer};
pub use reverb::{ReverbSettings, ReverbZone};
pub use sound::{Pcm, Sound};
pub use spatial::{Attenuation, AttenuationModel, Cone, Listener, SpatialSettings};
pub use stream::Stream;
pub use voice::{PlayOptions, Positional};
