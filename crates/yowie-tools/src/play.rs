//! Playing or rendering an audio file through `yowie-audio`.

use crate::error::Result;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use yowie_audio::tracker::{Module, Player};
use yowie_audio::{Mixer, Music, MusicOptions, Output, Pcm, PlayOptions, decode};

/// Sample rate of an offline render.
pub const DUMP_RATE: u32 = 44_100;

/// Longest offline render of a tracker song when no limit is given, in seconds.
const MODULE_LIMIT_SECONDS: usize = 600;

/// True when the extension names a tracker module (MOD, S3M, XM or IT).
pub fn is_module(path: &Path) -> bool {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase);
    matches!(ext.as_deref(), Some("mod" | "s3m" | "xm" | "it"))
}

/// Mixes `file` offline and writes it as a 16-bit stereo WAV at
/// [`DUMP_RATE`] to `out`, without touching an audio device.
///
/// `seconds` stops the render early; without it a sound plays to its end and
/// a tracker song to its end or ten minutes. Returns the frames written.
pub fn dump(file: &Path, out: &Path, seconds: Option<f64>) -> Result<usize> {
    let (mixer, mut renderer) = Mixer::offline(DUMP_RATE)?;
    let limit = seconds.map(|s| (s.max(0.0) * f64::from(DUMP_RATE)) as usize);
    let mut samples = Vec::new();
    if is_module(file) {
        let module = Arc::new(Module::load(&std::fs::read(file)?)?);
        let player = Player::new(module, DUMP_RATE);
        let control = player.control();
        mixer.play_stream(player, PlayOptions::default());
        let max = limit.unwrap_or(DUMP_RATE as usize * MODULE_LIMIT_SECONDS);
        while samples.len() / 2 < max && !control.finished() {
            samples.extend(renderer.render_frames(4096));
        }
        samples.truncate(max * 2);
    } else {
        let sound = mixer.sound(&decode::decode_file(file)?)?;
        let frames = limit.unwrap_or(sound.frames()).min(sound.frames());
        mixer.play(&sound, PlayOptions::default());
        samples = renderer.render_frames(frames);
    }
    let frames = samples.len() / 2;
    Pcm {
        samples,
        channels: 2,
        rate: DUMP_RATE,
    }
    .save_wav(out)?;
    Ok(frames)
}

/// Plays `file` on the default output device until it ends, or until
/// `seconds` have passed. Prints the device and, for a module, its title.
pub fn play(file: &Path, seconds: Option<f64>) -> Result<()> {
    let output = Output::open_default()?;
    let mixer = output.mixer();
    println!("playing on {}", output.device_name());
    let voice = if is_module(file) {
        let module = Arc::new(Module::load(&std::fs::read(file)?)?);
        println!(
            "{} ({}, {} channels)",
            module.title,
            module.format.name(),
            module.channels
        );
        mixer.play_stream(Player::new(module, mixer.rate()), PlayOptions::default())
    } else {
        let music = Music::open_file(&mixer, file, MusicOptions::default())?;
        mixer.play_stream(music, PlayOptions::default())
    };
    let started = std::time::Instant::now();
    while !voice.is_finished() {
        if seconds.is_some_and(|s| started.elapsed().as_secs_f64() >= s) {
            voice.stop();
        }
        mixer.update();
        std::thread::sleep(Duration::from_millis(50));
    }
    // Let the final block leave the device.
    std::thread::sleep(Duration::from_millis(200));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine_wav(path: &Path, seconds: f32) {
        let rate = 22_050u32;
        let samples = (0..(seconds * rate as f32) as usize)
            .flat_map(|i| {
                let v = (i as f32 * 440.0 * std::f32::consts::TAU / rate as f32).sin() * 0.5;
                [v, v]
            })
            .collect();
        Pcm {
            samples,
            channels: 2,
            rate,
        }
        .save_wav(path)
        .unwrap();
    }

    #[test]
    fn dump_renders_the_whole_file() {
        let dir = tempfile::tempdir().unwrap();
        let (src, out) = (dir.path().join("in.wav"), dir.path().join("out.wav"));
        sine_wav(&src, 1.0);
        let frames = dump(&src, &out, None).unwrap();
        assert!(out.is_file());
        assert!(frames > 20_000 && frames < 70_000, "{frames}");
        let limited = dump(&src, &out, Some(0.25)).unwrap();
        assert_eq!(limited, (0.25 * f64::from(DUMP_RATE)) as usize);
    }

    #[test]
    fn module_extensions() {
        assert!(is_module(Path::new("a/Song.XM")));
        assert!(!is_module(Path::new("a.wav")));
    }
}
