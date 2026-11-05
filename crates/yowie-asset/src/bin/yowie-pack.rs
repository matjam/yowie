//! Bundles an asset directory into a Yowie pack file.
//!
//! ```text
//! yowie-pack [-o assets.pak] [--no-compress] [--level N] DIRECTORY
//! ```

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use yowie_asset::{Compression, PackSource, pack_dir};

/// Bundles an asset directory into a pack file that `Vfs::mount_pack` can read.
#[derive(Parser)]
#[command(version)]
struct Args {
    /// The directory whose files are packed, recursively.
    directory: PathBuf,
    /// The pack file to write.
    #[arg(short, long, default_value = "assets.pak")]
    output: PathBuf,
    /// Store every file without compression.
    #[arg(long)]
    no_compress: bool,
    /// The zstd compression level, 1 to 22.
    #[arg(long, default_value_t = 19, value_parser = clap::value_parser!(i32).range(1..=22))]
    level: i32,
}

fn main() -> ExitCode {
    let args = Args::parse();
    let compression = if args.no_compress {
        Compression::None
    } else {
        Compression::Zstd(args.level)
    };
    if let Err(e) = pack_dir(&args.directory, &args.output, compression) {
        eprintln!("yowie-pack: {e}");
        return ExitCode::FAILURE;
    }
    match PackSource::open(&args.output) {
        Ok(pack) => {
            let size = std::fs::metadata(&args.output)
                .map(|m| m.len())
                .unwrap_or(0);
            println!(
                "wrote {} ({} files, {size} bytes)",
                args.output.display(),
                pack.len()
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("yowie-pack: written pack does not verify: {e}");
            ExitCode::FAILURE
        }
    }
}
