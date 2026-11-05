//! Packages a built game for distribution: a `.app` bundle on macOS, or a
//! folder holding the executable and assets elsewhere. The result is not
//! signed or notarized, and the executable is not cross-compiled.

use clap::{Parser, ValueEnum};
use std::path::PathBuf;
use std::process::ExitCode;
use yowie_tools::bundle::{BundleOptions, Layout, bundle};

#[derive(Clone, Copy, ValueEnum)]
enum Target {
    Mac,
    Folder,
}

#[derive(Parser)]
#[command(name = "yowie-bundle", version, about)]
struct Args {
    /// Display name of the game; also the name of the output.
    #[arg(long)]
    name: String,
    /// The built executable.
    #[arg(long)]
    exe: PathBuf,
    /// Asset directory (copied whole) or pack file to ship with the game.
    #[arg(long)]
    assets: Option<PathBuf>,
    /// PNG used to build the macOS .icns (1024 pixels is best).
    #[arg(long)]
    icon: Option<PathBuf>,
    /// Bundle identifier (macOS), such as com.example.game.
    #[arg(long)]
    id: Option<String>,
    /// Version string written to the Info.plist.
    #[arg(long, default_value = "1.0")]
    version_string: String,
    /// Output directory.
    #[arg(short, long, default_value = "dist")]
    output: PathBuf,
    /// Layout to write (default: an app on macOS, a folder elsewhere).
    #[arg(long, value_enum)]
    target: Option<Target>,
}

fn main() -> ExitCode {
    let args = Args::parse();
    let layout = match args.target {
        Some(Target::Mac) => Layout::MacApp,
        Some(Target::Folder) => Layout::Folder,
        None => Layout::host(),
    };
    let opts = BundleOptions {
        name: args.name,
        exe: args.exe,
        assets: args.assets,
        icon: args.icon,
        id: args.id,
        version: args.version_string,
        out: args.output,
        layout,
    };
    match bundle(&opts) {
        Ok(path) => {
            println!("wrote {}", path.display());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("yowie-bundle: {e}");
            ExitCode::FAILURE
        }
    }
}
