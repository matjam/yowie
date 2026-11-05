//! Packaging a built game: a macOS `.app` bundle or a plain folder.

use crate::error::{Error, Result};
use crate::icns::icns_from_file;
use std::fs;
use std::path::{Path, PathBuf};

/// The layout [`bundle`] writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    /// `Name.app/Contents/{Info.plist, MacOS, Resources}`.
    MacApp,
    /// `Name/` holding the executable and the assets side by side.
    Folder,
}

impl Layout {
    /// The layout for the platform this tool runs on: an app bundle on
    /// macOS and a folder elsewhere.
    pub fn host() -> Self {
        if cfg!(target_os = "macos") {
            Self::MacApp
        } else {
            Self::Folder
        }
    }
}

/// What to package.
#[derive(Debug, Clone)]
pub struct BundleOptions {
    /// Display name of the game; also the name of the output.
    pub name: String,
    /// The built executable.
    pub exe: PathBuf,
    /// A directory (copied whole) or single file (such as a pack) to ship.
    pub assets: Option<PathBuf>,
    /// A PNG, ideally square and 1024 pixels, used to build the `.icns`.
    pub icon: Option<PathBuf>,
    /// Bundle identifier; defaults to `com.example.` plus the lower-case
    /// letters and digits of the name.
    pub id: Option<String>,
    /// Version string written to the Info.plist.
    pub version: String,
    /// The directory that receives the output.
    pub out: PathBuf,
    /// Which layout to write.
    pub layout: Layout,
}

/// Writes the bundle and returns its path. An existing output of the same
/// name is replaced. The result is not signed or notarized.
///
/// For [`Layout::MacApp`] the assets go to `Contents/Resources/<name of the
/// assets directory>`, so a game finds them next to its executable's
/// `../Resources`. For [`Layout::Folder`] the icon is copied as `icon.png`.
pub fn bundle(opts: &BundleOptions) -> Result<PathBuf> {
    if opts.name.is_empty() || opts.name.contains(['/', '\\']) {
        return Err(Error::Invalid(format!(
            "{:?} is not usable as a name",
            opts.name
        )));
    }
    let exe_name = opts
        .exe
        .file_name()
        .ok_or_else(|| Error::Invalid(format!("{} is not a file", opts.exe.display())))?;
    if !opts.exe.is_file() {
        return Err(Error::Invalid(format!(
            "executable {} not found",
            opts.exe.display()
        )));
    }
    let root = match opts.layout {
        Layout::MacApp => opts.out.join(format!("{}.app", opts.name)),
        Layout::Folder => opts.out.join(&opts.name),
    };
    // Build the icon first so a bad file leaves no half-written bundle.
    let icns = match (&opts.icon, opts.layout) {
        (Some(png), Layout::MacApp) => Some(icns_from_file(png)?),
        _ => None,
    };
    if root.exists() {
        fs::remove_dir_all(&root)?;
    }
    let (exe_dir, data_dir) = match opts.layout {
        Layout::MacApp => (root.join("Contents/MacOS"), root.join("Contents/Resources")),
        Layout::Folder => (root.clone(), root.clone()),
    };
    fs::create_dir_all(&exe_dir)?;
    fs::create_dir_all(&data_dir)?;
    copy_file(&opts.exe, &exe_dir.join(exe_name), true)?;
    if let Some(assets) = &opts.assets {
        let name = assets
            .file_name()
            .ok_or_else(|| Error::Invalid(format!("{} has no name", assets.display())))?;
        copy_tree(assets, &data_dir.join(name))?;
    }
    match opts.layout {
        Layout::MacApp => {
            let has_icon = icns.is_some();
            if let Some(bytes) = icns {
                fs::write(data_dir.join("icon.icns"), bytes)?;
            }
            let id = opts.id.clone().unwrap_or_else(|| default_id(&opts.name));
            let plist = info_plist(
                &opts.name,
                &id,
                &exe_name.to_string_lossy(),
                &opts.version,
                has_icon,
            );
            fs::write(root.join("Contents/Info.plist"), plist)?;
        }
        Layout::Folder => {
            if let Some(png) = &opts.icon {
                copy_file(png, &data_dir.join("icon.png"), false)?;
            }
        }
    }
    Ok(root)
}

/// `com.example.` plus the name reduced to lower-case letters and digits.
pub fn default_id(name: &str) -> String {
    let tail: String = name
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .collect::<String>()
        .to_lowercase();
    format!(
        "com.example.{}",
        if tail.is_empty() { "game" } else { &tail }
    )
}

/// The text of an Info.plist for a game bundle.
pub fn info_plist(name: &str, id: &str, exe: &str, version: &str, has_icon: bool) -> String {
    let icon = if has_icon {
        "\t<key>CFBundleIconFile</key>\n\t<string>icon</string>\n"
    } else {
        ""
    };
    let (name, id, exe, version) = (escape(name), escape(id), escape(exe), escape(version));
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
<plist version=\"1.0\">\n<dict>\n\
\t<key>CFBundleName</key>\n\t<string>{name}</string>\n\
\t<key>CFBundleDisplayName</key>\n\t<string>{name}</string>\n\
\t<key>CFBundleIdentifier</key>\n\t<string>{id}</string>\n\
\t<key>CFBundleExecutable</key>\n\t<string>{exe}</string>\n\
\t<key>CFBundleVersion</key>\n\t<string>{version}</string>\n\
\t<key>CFBundleShortVersionString</key>\n\t<string>{version}</string>\n\
\t<key>CFBundlePackageType</key>\n\t<string>APPL</string>\n\
\t<key>CFBundleInfoDictionaryVersion</key>\n\t<string>6.0</string>\n\
\t<key>LSMinimumSystemVersion</key>\n\t<string>12.0</string>\n\
\t<key>NSHighResolutionCapable</key>\n\t<true/>\n\
\t<key>LSApplicationCategoryType</key>\n\t<string>public.app-category.games</string>\n\
{icon}</dict>\n</plist>\n"
    )
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn copy_file(src: &Path, dst: &Path, executable: bool) -> Result<()> {
    if let Some(dir) = dst.parent() {
        fs::create_dir_all(dir)?;
    }
    fs::copy(src, dst)?;
    #[cfg(unix)]
    if executable {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(dst, fs::Permissions::from_mode(0o755))?;
    }
    #[cfg(not(unix))]
    let _ = executable;
    Ok(())
}

/// Copies a file, or a directory with everything under it.
fn copy_tree(src: &Path, dst: &Path) -> Result<()> {
    if !src.is_dir() {
        return copy_file(src, dst, false);
    }
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        copy_tree(&entry.path(), &dst.join(entry.file_name()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options(dir: &Path, layout: Layout) -> BundleOptions {
        fs::write(dir.join("game"), b"#!/bin/sh\n").unwrap();
        fs::create_dir_all(dir.join("assets/sub")).unwrap();
        fs::write(dir.join("assets/a.txt"), b"a").unwrap();
        fs::write(dir.join("assets/sub/b.txt"), b"b").unwrap();
        image::RgbaImage::new(64, 64)
            .save(dir.join("icon.png"))
            .unwrap();
        BundleOptions {
            name: "My Game".into(),
            exe: dir.join("game"),
            assets: Some(dir.join("assets/")),
            icon: Some(dir.join("icon.png")),
            id: None,
            version: "1.2".into(),
            out: dir.join("dist"),
            layout,
        }
    }

    #[test]
    fn mac_app_structure() {
        let dir = tempfile::tempdir().unwrap();
        let app = bundle(&options(dir.path(), Layout::MacApp)).unwrap();
        assert_eq!(app, dir.path().join("dist/My Game.app"));
        for f in [
            "Contents/MacOS/game",
            "Contents/Resources/assets/sub/b.txt",
            "Contents/Resources/icon.icns",
        ] {
            assert!(app.join(f).is_file(), "{f}");
        }
        let plist = fs::read_to_string(app.join("Contents/Info.plist")).unwrap();
        for want in [
            "com.example.mygame",
            "<string>game</string>",
            "CFBundleIconFile",
            "1.2",
        ] {
            assert!(plist.contains(want), "{want}");
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(app.join("Contents/MacOS/game"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o111,
                0o111
            );
        }
    }

    #[test]
    fn folder_structure_and_rebundle() {
        let dir = tempfile::tempdir().unwrap();
        let opts = options(dir.path(), Layout::Folder);
        for _ in 0..2 {
            let out = bundle(&opts).unwrap();
            assert!(
                out.join("game").is_file()
                    && out.join("assets/a.txt").is_file()
                    && out.join("icon.png").is_file()
            );
        }
    }

    #[test]
    fn rejects_bad_input() {
        let dir = tempfile::tempdir().unwrap();
        let mut opts = options(dir.path(), Layout::Folder);
        opts.exe = dir.path().join("missing");
        assert!(bundle(&opts).is_err());
        opts.exe = dir.path().join("game");
        opts.name = "a/b".into();
        assert!(bundle(&opts).is_err());
    }

    #[test]
    fn plist_escapes() {
        assert!(info_plist("A & B", "x", "e", "1", false).contains("A &amp; B"));
        assert_eq!(default_id("!!"), "com.example.game");
    }
}
