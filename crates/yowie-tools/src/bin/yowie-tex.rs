//! Compresses PNG and JPEG images to BC1, BC3, BC4, BC5 or BC7 KTX2 files
//! with a full mip chain, ready for a GPU to take as they are.
//!
//! Choose the format by use: `bc7` for colour that must hold up close, `bc1`
//! for opaque colour at half the size, `bc3` for colour with alpha, `bc4` for
//! a one-channel mask or height map and `bc5` for a tangent-space normal map.
//! Normal maps, masks and roughness maps hold data rather than colour and want
//! `--linear`; `bc4` and `bc5` are always linear.

use clap::{Parser, ValueEnum};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;
use yowie_tools::encode::{EncodeOptions, encode_image};
use yowie_tools::mips::MipFilter;
use yowie_tools::{BlockFormat, Error, Result};

#[derive(Clone, Copy, ValueEnum)]
enum Filter {
    Box,
    Kaiser,
}

#[derive(Parser)]
#[command(name = "yowie-tex", version, about)]
struct Args {
    /// Images to compress (PNG or JPEG).
    #[arg(required = true)]
    files: Vec<PathBuf>,
    /// Block format: bc1, bc3, bc4, bc5 or bc7.
    #[arg(short, long, default_value = "bc7")]
    format: String,
    /// The images hold sRGB colour (the default).
    #[arg(long, conflicts_with = "linear")]
    srgb: bool,
    /// The images hold data rather than sRGB colour.
    #[arg(long)]
    linear: bool,
    /// Write the single output here (one input only).
    #[arg(short, long, conflicts_with = "outdir")]
    output: Option<PathBuf>,
    /// Write every output into this directory (default: beside each input).
    #[arg(long)]
    outdir: Option<PathBuf>,
    /// Write level 0 alone instead of the whole mip chain.
    #[arg(long)]
    no_mips: bool,
    /// Mip filter.
    #[arg(long, value_enum, default_value = "box")]
    filter: Filter,
    /// Use quicker, slightly worse BC7 settings.
    #[arg(long)]
    fast: bool,
    /// Keep alpha straight instead of premultiplying colour in linear light.
    #[arg(long)]
    straight_alpha: bool,
    /// Report each file's size and time.
    #[arg(short, long)]
    verbose: bool,
}

fn main() -> ExitCode {
    let args = Args::parse();
    if args.output.is_some() && args.files.len() != 1 {
        eprintln!("yowie-tex: --output takes one input file; use --outdir for several");
        return ExitCode::from(2);
    }
    let format = match BlockFormat::parse(&args.format) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("yowie-tex: {e}");
            return ExitCode::from(2);
        }
    };
    let mut opts = EncodeOptions::new(format);
    opts.srgb = !args.linear;
    opts.mips = !args.no_mips;
    opts.fast = args.fast;
    opts.premultiply = !args.straight_alpha;
    opts.filter = match args.filter {
        Filter::Box => MipFilter::Box,
        Filter::Kaiser => MipFilter::Kaiser,
    };
    let mut status = ExitCode::SUCCESS;
    for src in &args.files {
        let dst = destination(src, args.output.as_deref(), args.outdir.as_deref());
        if let Err(e) = convert(src, &dst, &opts, args.verbose) {
            eprintln!("yowie-tex: {}: {e}", src.display());
            status = ExitCode::FAILURE;
        }
    }
    status
}

fn destination(src: &Path, output: Option<&Path>, outdir: Option<&Path>) -> PathBuf {
    if let Some(o) = output {
        return o.to_path_buf();
    }
    let name = src.with_extension("ktx2");
    let name = name.file_name().map(PathBuf::from).unwrap_or_default();
    match outdir {
        Some(dir) => dir.join(name),
        None => src.with_file_name(name),
    }
}

fn convert(src: &Path, dst: &Path, opts: &EncodeOptions, verbose: bool) -> Result<()> {
    let start = Instant::now();
    let image = image::open(src)?.to_rgba8();
    let file = encode_image(&image, opts)?;
    let bytes = file.to_bytes()?;
    if let Some(dir) = dst.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(Error::from)?;
    }
    std::fs::write(dst, &bytes)?;
    if verbose {
        let raw = u64::from(file.width) * u64::from(file.height) * 4;
        println!(
            "{} -> {}  {}x{} {:?}  {} levels  {} bytes ({:.0}% of raw)  {:.0?}",
            src.display(),
            dst.display(),
            file.width,
            file.height,
            file.format,
            file.levels.len(),
            bytes.len(),
            100.0 * bytes.len() as f64 / raw as f64,
            start.elapsed()
        );
    } else {
        println!("{} -> {}", src.display(), dst.display());
    }
    Ok(())
}
