//! Asset path normalisation.

use crate::error::{Error, Result};

/// Normalises an asset path to the form every mount uses.
///
/// Backslashes become forward slashes, empty and `.` components are dropped,
/// and `..` removes the previous component. The result has no leading or
/// trailing slash; the root is the empty string. A `..` that would climb
/// above the root is an [`Error::InvalidPath`].
pub fn normalize(path: &str) -> Result<String> {
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split(['/', '\\']) {
        match part {
            "" | "." => {}
            ".." => {
                if parts.pop().is_none() {
                    return Err(Error::InvalidPath(path.to_string()));
                }
            }
            p => parts.push(p),
        }
    }
    Ok(parts.join("/"))
}

/// Joins a directory and a relative path, then normalises the result.
///
/// To resolve a file next to another, pass the first file's directory.
pub fn join(dir: &str, rel: &str) -> Result<String> {
    normalize(&format!("{dir}/{rel}"))
}

/// Returns the directory part of a normalised path; the root for a bare name.
pub fn dir_of(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(dir, _)| dir)
}

/// Returns the extension of a path in lower case, without the dot.
pub fn extension(path: &str) -> Option<String> {
    let name = path.rsplit('/').next()?;
    let (stem, ext) = name.rsplit_once('.')?;
    (!stem.is_empty()).then(|| ext.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_table() {
        let cases = [
            ("a/b.txt", Some("a/b.txt")),
            ("/a//b.txt", Some("a/b.txt")),
            ("a\\b\\c.txt", Some("a/b/c.txt")),
            ("./a/./b", Some("a/b")),
            ("a/b/../c", Some("a/c")),
            ("", Some("")),
            ("a/", Some("a")),
            ("../a", None),
            ("a/../../b", None),
        ];
        for (input, want) in cases {
            assert_eq!(normalize(input).ok().as_deref(), want, "{input:?}");
        }
    }

    #[test]
    fn join_and_dir() {
        assert_eq!(join("maps", "../tiles/a.png").unwrap(), "tiles/a.png");
        assert_eq!(join("", "a.png").unwrap(), "a.png");
        assert_eq!(dir_of("a/b/c.txt"), "a/b");
        assert_eq!(dir_of("c.txt"), "");
    }

    #[test]
    fn extension_is_lower_case() {
        assert_eq!(extension("a/b.PNG").as_deref(), Some("png"));
        assert_eq!(extension("a/.hidden"), None);
        assert_eq!(extension("a/none"), None);
    }
}
