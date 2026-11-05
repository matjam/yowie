//! Plays an audio file on the default device, or renders it to a WAV file.
//!
//! ```text
//! cargo run -p yowie-audio --example play -- song.xm
//! cargo run -p yowie-audio --example play -- sound.ogg --dump out.wav
//! ```
//!
//! Supported files: WAV, Ogg Vorbis, MP3, FLAC, MOD, S3M, XM and IT. With `--dump`
//! nothing is played: the file is mixed offline and written as a 16-bit stereo WAV.
//! `--seconds N` stops after N seconds (the default is the length of the file, with a
//! limit of ten minutes for tracker songs).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use yowie_audio::tracker::{Module, Player};
use yowie_audio::{Mixer, Music, MusicOptions, Output, Pcm, PlayOptions, Result, decode};

struct Args {
    file: PathBuf,
    dump: Option<PathBuf>,
    seconds: Option<f64>,
}

fn parse_args() -> Option<Args> {
    let mut args = std::env::args().skip(1);
    let mut file = None;
    let mut dump = None;
    let mut seconds = None;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--dump" => dump = Some(PathBuf::from(args.next()?)),
            "--seconds" => seconds = Some(args.next()?.parse().ok()?),
            _ if file.is_none() => file = Some(PathBuf::from(a)),
            _ => return None,
        }
    }
    Some(Args {
        file: file?,
        dump,
        seconds,
    })
}

fn is_module(path: &std::path::Path) -> bool {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase);
    matches!(ext.as_deref(), Some("mod" | "s3m" | "xm" | "it"))
}

fn main() {
    let Some(args) = parse_args() else {
        eprintln!("usage: play <file> [--dump out.wav] [--seconds N]");
        std::process::exit(2);
    };
    if let Err(e) = run(&args) {
        eprintln!("play: {e}");
        std::process::exit(1);
    }
}

fn run(args: &Args) -> Result<()> {
    match &args.dump {
        Some(out) => dump(args, out),
        None => play(args),
    }
}

/// Mixes the file offline and writes it as a WAV.
fn dump(args: &Args, out: &std::path::Path) -> Result<()> {
    let rate = 44_100;
    let (mixer, mut renderer) = Mixer::offline(rate)?;
    let limit = args.seconds.map(|s| (s * f64::from(rate)) as usize);
    let mut samples = Vec::new();
    if is_module(&args.file) {
        let module = Arc::new(Module::load(&std::fs::read(&args.file)?)?);
        let player = Player::new(module, rate);
        let control = player.control();
        mixer.play_stream(player, PlayOptions::default());
        let max = limit.unwrap_or(rate as usize * 600);
        while samples.len() / 2 < max && !control.finished() {
            samples.extend(renderer.render_frames(4096));
        }
        samples.truncate(max * 2);
    } else {
        let sound = mixer.sound(&decode::decode_file(&args.file)?)?;
        let frames = limit.unwrap_or(sound.frames()).min(sound.frames());
        mixer.play(&sound, PlayOptions::default());
        samples = renderer.render_frames(frames);
    }
    Pcm {
        samples,
        channels: 2,
        rate,
    }
    .save_wav(out)?;
    println!("wrote {}", out.display());
    Ok(())
}

/// Plays the file on the default device until it ends.
fn play(args: &Args) -> Result<()> {
    let output = Output::open_default()?;
    let mixer = output.mixer();
    println!("playing on {}", output.device_name());
    let voice = if is_module(&args.file) {
        let module = Arc::new(Module::load(&std::fs::read(&args.file)?)?);
        println!(
            "{} ({}, {} channels)",
            module.title,
            module.format.name(),
            module.channels
        );
        mixer.play_stream(Player::new(module, mixer.rate()), PlayOptions::default())
    } else {
        let music = Music::open_file(&mixer, &args.file, MusicOptions::default())?;
        mixer.play_stream(music, PlayOptions::default())
    };
    let started = std::time::Instant::now();
    while !voice.is_finished() {
        if args
            .seconds
            .is_some_and(|s| started.elapsed().as_secs_f64() >= s)
        {
            voice.stop();
        }
        mixer.update();
        std::thread::sleep(Duration::from_millis(50));
    }
    // Let the final block leave the device.
    std::thread::sleep(Duration::from_millis(200));
    Ok(())
}
