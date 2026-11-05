//! The engine's mixer: on the default device in windowed runs, rendered
//! into nothing otherwise so game code behaves the same either way.

use yowie_audio::{Mixer, Output, Renderer};

use crate::Result;

/// The sample rate of the offline mixer used when no device plays.
const OFFLINE_RATE: u32 = 48_000;
/// The most time one frame renders offline, in seconds, so a long stall
/// does not render a long buffer.
const MAX_RENDER: f64 = 0.25;

/// What drives the mixer.
pub(crate) struct AudioBackend {
    /// Keeps the device stream alive.
    _output: Option<Output>,
    /// Drives the mixer by hand when there is no device.
    renderer: Option<Renderer>,
    /// Frames still owed from earlier fractions of a frame.
    carry: f64,
    scratch: Vec<f32>,
    rate: u32,
}

impl std::fmt::Debug for AudioBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AudioBackend")
            .field("device", &self._output.is_some())
            .field("rate", &self.rate)
            .finish()
    }
}

/// Opens the default output device when `device` is set, and falls back to
/// the offline mixer when it is not or when opening fails.
pub(crate) fn start(device: bool) -> Result<(Mixer, AudioBackend)> {
    if device {
        match Output::open_default() {
            Ok(output) => {
                let mixer = output.mixer();
                let rate = mixer.rate();
                let backend = AudioBackend {
                    _output: Some(output),
                    renderer: None,
                    carry: 0.0,
                    scratch: Vec::new(),
                    rate,
                };
                return Ok((mixer, backend));
            }
            Err(e) => log::warn!("no audio output, sound is silent: {e}"),
        }
    }
    let (mixer, renderer) = Mixer::offline(OFFLINE_RATE)?;
    let backend = AudioBackend {
        _output: None,
        renderer: Some(renderer),
        carry: 0.0,
        scratch: Vec::new(),
        rate: OFFLINE_RATE,
    };
    Ok((mixer, backend))
}

impl AudioBackend {
    /// Runs the mixer's end-of-voice callbacks and, without a device,
    /// renders `seconds` of game time so voices advance and finish.
    pub(crate) fn update(&mut self, mixer: &Mixer, seconds: f64) {
        if let Some(renderer) = &mut self.renderer {
            self.carry += seconds.clamp(0.0, MAX_RENDER) * f64::from(self.rate);
            let frames = self.carry.floor();
            self.carry -= frames;
            let samples = frames as usize * 2;
            self.scratch.resize(samples, 0.0);
            renderer.render(&mut self.scratch);
        }
        mixer.update();
    }

    /// Whether a real device plays the mixer.
    #[cfg(test)]
    pub(crate) fn has_device(&self) -> bool {
        self._output.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use yowie_audio::{Pcm, PlayOptions};

    #[test]
    fn offline_mixer_advances_and_finishes_voices() {
        let (mixer, mut backend) = start(false).unwrap_or_else(|e| panic!("start: {e}"));
        assert!(!backend.has_device());
        let sound = mixer
            .sound(&Pcm::sine(440.0, 0.1, 44_100))
            .unwrap_or_else(|e| panic!("sound: {e}"));
        let voice = mixer.play(&sound, PlayOptions::default());
        let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = done.clone();
        voice.on_done(move || flag.store(true, std::sync::atomic::Ordering::SeqCst));
        // 0.3 s of game time in 60 Hz frames outlasts the 0.1 s sound.
        for _ in 0..18 {
            backend.update(&mixer, 1.0 / 60.0);
        }
        assert!(voice.is_finished());
        // The callback runs on the game thread inside `update`.
        backend.update(&mixer, 0.0);
        assert!(done.load(std::sync::atomic::Ordering::SeqCst));
    }

    #[test]
    fn a_stall_renders_a_bounded_buffer() {
        let (mixer, mut backend) = start(false).unwrap_or_else(|e| panic!("start: {e}"));
        backend.update(&mixer, 100.0);
        assert!(backend.scratch.len() <= (MAX_RENDER * f64::from(OFFLINE_RATE)) as usize * 2);
    }
}
