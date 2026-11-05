//! Records the wgpu version from the workspace lock file for `yowie-info`.

use std::path::Path;

fn main() {
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
