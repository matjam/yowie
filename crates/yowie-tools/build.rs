//! Records the wgpu version from the workspace lock file for `yowie-info`,
//! and links the C++ runtime on GNU/Linux.

use std::path::Path;

fn main() {
    // intel_tex_2 links a prebuilt C++ static library (its ASTC encoder) but
    // never asks for the C++ runtime. macOS and MSVC resolve it anyway; GNU
    // ld and lld do not, and fail on `__gxx_personality_v0`.
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if os == "linux" && env == "gnu" {
        println!("cargo:rustc-link-lib=dylib=stdc++");
    }

    let lock = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.lock");
    println!("cargo:rerun-if-changed={}", lock.display());
    let version = std::fs::read_to_string(&lock)
        .ok()
        .and_then(|text| {
            let mut lines = text.lines();
            while let Some(line) = lines.next() {
                if line == "name = \"wgpu\"" {
                    let v = lines.next()?.strip_prefix("version = \"")?;
                    return Some(v.trim_end_matches('"').to_string());
                }
            }
            None
        })
        .unwrap_or_else(|| "unknown".to_string());
    println!("cargo:rustc-env=YOWIE_WGPU_VERSION={version}");
}
