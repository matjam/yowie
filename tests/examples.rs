//! Runs each golden-covered example headless under the fixed clock and
//! compares its last frame with `examples/testdata/<name>.png`.
//!
//! Frames are compared on the GPU named in `examples/testdata/gpu.txt`
//! (where the images were recorded); on other GPUs the test only checks
//! that each example ran and drew something. `YOWIE_UPDATE_GOLDEN=1`
//! rewrites the images and `gpu.txt` from this machine. The `window`
//! example is not covered: it needs a desktop. Skipped when there is no
//! GPU adapter.

use std::path::{Path, PathBuf};
use std::process::Command;

use image::RgbaImage;

/// Examples with a golden image, and the arguments each runs with besides
/// `--shot`.
const GOLDEN: &[(&str, &[&str])] = &[
    ("audio", &["--seconds", "2"]),
    ("clear", &[]),
    ("inputs", &[]),
    ("lighting", &["--seconds", "2"]),
    ("pathfinding", &[]),
    ("physics3d", &["--seconds", "3"]),
    ("roguelike", &["--autoplay", "--seconds", "0.5"]),
    ("shaders", &["--seconds", "1"]),
    ("sprites", &["--seconds", "1"]),
    ("text", &[]),
    ("viewer", &["--seconds", "1"]),
];

/// The mean absolute difference per channel (0 to 255) the whole frame may
/// show.
const MEAN_LIMIT: f64 = 1.5;
/// The largest difference per channel the mean of any 16 by 16 block may
/// show, which catches something moving rather than changing shade.
const BLOCK_LIMIT: f64 = 12.0;
const BLOCK: u32 = 16;

fn testdata() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/testdata")
}

/// Cargo builds the examples into `target/<profile>/examples`, beside the
/// `deps` directory holding this test.
fn examples_dir() -> PathBuf {
    let exe = std::env::current_exe().expect("test executable path");
    exe.parent()
        .and_then(Path::parent)
        .expect("target profile directory")
        .join("examples")
}

/// Builds the examples with the profile this test was built with, so
/// running this test alone (`cargo test --test examples`) never runs stale
/// example binaries.
fn build_examples() -> Result<(), String> {
    let dir = examples_dir();
    let profile = dir
        .parent()
        .and_then(Path::file_name)
        .and_then(|n| n.to_str())
        .unwrap_or("debug")
        .to_owned();
    let mut cmd = Command::new(env!("CARGO"));
    cmd.args(["build", "--examples", "--quiet"])
        .current_dir(env!("CARGO_MANIFEST_DIR"));
    match profile.as_str() {
        "debug" => {}
        "release" => {
            cmd.arg("--release");
        }
        other => {
            cmd.args(["--profile", other]);
        }
    }
    let status = cmd.status().map_err(|e| format!("cannot run cargo: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("building the examples failed: {status}"))
    }
}

fn mean_diff(a: &RgbaImage, b: &RgbaImage) -> f64 {
    let total: u64 = a
        .as_raw()
        .iter()
        .zip(b.as_raw())
        .map(|(x, y)| u64::from(x.abs_diff(*y)))
        .sum();
    total as f64 / a.as_raw().len() as f64
}

/// The largest per-channel difference between the two images' block means.
fn block_diff(a: &RgbaImage, b: &RgbaImage) -> f64 {
    let mut worst = 0.0f64;
    for by in (0..a.height()).step_by(BLOCK as usize) {
        for bx in (0..a.width()).step_by(BLOCK as usize) {
            let mut sums = [0i64; 4];
            let mut n = 0i64;
            for y in by..(by + BLOCK).min(a.height()) {
                for x in bx..(bx + BLOCK).min(a.width()) {
                    let (pa, pb) = (a.get_pixel(x, y).0, b.get_pixel(x, y).0);
                    for c in 0..4 {
                        sums[c] += i64::from(pa[c]) - i64::from(pb[c]);
                    }
                    n += 1;
                }
            }
            for s in sums {
                worst = worst.max((s as f64 / n as f64).abs());
            }
        }
    }
    worst
}

fn run_example(name: &str, extra: &[&str], shot: &Path) -> Result<RgbaImage, String> {
    let exe = examples_dir().join(format!("{name}{}", std::env::consts::EXE_SUFFIX));
    let output = Command::new(&exe)
        .args(extra)
        .arg("--shot")
        .arg(shot)
        .env("YOWIE_FIXED_CLOCK", "1")
        .output()
        .map_err(|e| format!("{name}: cannot run {}: {e}", exe.display()))?;
    if !output.status.success() {
        return Err(format!(
            "{name}: exited with {}\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    image::open(shot)
        .map(|i| i.to_rgba8())
        .map_err(|e| format!("{name}: cannot read {}: {e}", shot.display()))
}

#[test]
fn examples_match_golden_images() {
    let Some(info) = yowie::gfx::default_adapter_info() else {
        println!("skipping examples test: no GPU adapter");
        return;
    };
    let update = std::env::var("YOWIE_UPDATE_GOLDEN").is_ok_and(|v| v == "1");
    let gpu_file = testdata().join("gpu.txt");
    let recorded = std::fs::read_to_string(&gpu_file).unwrap_or_default();
    let compare = recorded.trim() == info.name;
    if !compare && !update {
        println!(
            "golden images were recorded on {:?}; this is {:?}, so only checking the examples draw",
            recorded.trim(),
            info.name
        );
    }
    if let Err(e) = build_examples() {
        panic!("{e}");
    }
    let out = std::env::temp_dir().join(format!("yowie-examples-{}", std::process::id()));
    std::fs::create_dir_all(&out).expect("temporary directory");
    let mut failures = Vec::new();
    for (name, extra) in GOLDEN {
        let shot = out.join(format!("{name}.png"));
        let got = match run_example(name, extra, &shot) {
            Ok(img) => img,
            Err(e) => {
                failures.push(e);
                continue;
            }
        };
        let golden = testdata().join(format!("{name}.png"));
        if update {
            std::fs::copy(&shot, &golden).expect("write golden image");
            continue;
        }
        if !compare {
            let first = got.get_pixel(0, 0);
            if *name != "clear" && got.pixels().all(|p| p == first) {
                failures.push(format!("{name}: frame is one flat colour"));
            }
            continue;
        }
        let want = match image::open(&golden) {
            Ok(i) => i.to_rgba8(),
            Err(e) => {
                failures.push(format!("{name}: cannot read {}: {e}", golden.display()));
                continue;
            }
        };
        if want.dimensions() != got.dimensions() {
            failures.push(format!(
                "{name}: frame is {:?}, golden is {:?}",
                got.dimensions(),
                want.dimensions()
            ));
            continue;
        }
        let (mean, block) = (mean_diff(&got, &want), block_diff(&got, &want));
        if mean > MEAN_LIMIT || block > BLOCK_LIMIT {
            failures.push(format!(
                "{name}: mean difference {mean:.2} (limit {MEAN_LIMIT}), worst block {block:.2} \
                 (limit {BLOCK_LIMIT}); frame kept at {}",
                shot.display()
            ));
        }
    }
    if update {
        std::fs::write(&gpu_file, format!("{}\n", info.name)).expect("write gpu.txt");
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The `assets` example rewrites files while it runs, so its frame is only
/// checked for being more than one flat colour. It works in a private
/// directory so the test leaves nothing behind.
#[test]
fn assets_example_draws_something() {
    if yowie::gfx::default_adapter_info().is_none() {
        println!("skipping assets example test: no GPU adapter");
        return;
    }
    if let Err(e) = build_examples() {
        panic!("{e}");
    }
    let out = std::env::temp_dir().join(format!("yowie-assets-test-{}", std::process::id()));
    std::fs::create_dir_all(&out).expect("temporary directory");
    let dir = out.join("project").join("assets");
    let shot = out.join("assets.png");
    let args = ["--seconds", "1", "--dir", dir.to_str().expect("utf-8 path")];
    let img = run_example("assets", &args, &shot).unwrap_or_else(|e| panic!("{e}"));
    let first = img.get_pixel(0, 0);
    assert!(
        img.pixels().any(|p| p != first),
        "assets: frame is one flat colour"
    );
    let _ = std::fs::remove_dir_all(out);
}
