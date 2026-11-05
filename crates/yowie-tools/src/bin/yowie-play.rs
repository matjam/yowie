//! Plays a WAV, Ogg, MP3, FLAC, MOD, S3M, XM or IT file on the default output
//! device, or with `--dump out.wav` renders it offline to a 16-bit stereo WAV.

use clap::Parser;
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Parser)]
#[command(name = "yowie-play", version, about)]
struct Args {
    /// The file to play.
    file: PathBuf,
    /// Render offline to this WAV instead of playing.
    #[arg(long)]
    dump: Option<PathBuf>,
    /// Stop after this many seconds.
    #[arg(long)]
    seconds: Option<f64>,
}

fn main() -> ExitCode {
    let args = Args::parse();
    let result = match &args.dump {
        Some(out) => yowie_tools::play::dump(&args.file, out, args.seconds).map(|frames| {
            println!("wrote {} ({frames} frames)", out.display());
        }),
        None => yowie_tools::play::play(&args.file, args.seconds),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("yowie-play: {e}");
            ExitCode::FAILURE
        }
    }
}
