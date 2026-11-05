//! The command-line arguments and environment variables the engine reads.

use std::path::PathBuf;

/// What the engine takes from the command line and environment.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct RunArgs {
    /// `--shot <png>`: render headless and write the last frame here.
    pub(crate) shot: Option<PathBuf>,
    /// `--seconds <n>`: stop after this much game time.
    pub(crate) seconds: Option<f64>,
    /// `YOWIE_FIXED_CLOCK=1`: one fixed step per frame.
    pub(crate) fixed_clock: bool,
}

impl RunArgs {
    /// Reads the process's arguments and environment.
    pub(crate) fn from_env() -> Result<Self, String> {
        let mut args = Self::parse(std::env::args().skip(1))?;
        args.fixed_clock |= std::env::var("YOWIE_FIXED_CLOCK").is_ok_and(|v| v == "1");
        Ok(args)
    }

    /// Picks `--shot` and `--seconds` (as `--flag value` or `--flag=value`)
    /// out of `args`, ignoring every other argument so games can have their
    /// own.
    pub(crate) fn parse(args: impl IntoIterator<Item = String>) -> Result<Self, String> {
        let mut out = RunArgs::default();
        let mut it = args.into_iter();
        while let Some(arg) = it.next() {
            let (flag, inline) = match arg.split_once('=') {
                Some((f, v)) => (f.to_owned(), Some(v.to_owned())),
                None => (arg.clone(), None),
            };
            if flag != "--shot" && flag != "--seconds" {
                continue;
            }
            let value = match inline {
                Some(v) => v,
                None => it.next().ok_or_else(|| format!("{flag} needs a value"))?,
            };
            if flag == "--shot" {
                out.shot = Some(PathBuf::from(value));
            } else {
                let n: f64 = value
                    .parse()
                    .map_err(|_| format!("--seconds wants a number, got {value:?}"))?;
                if !n.is_finite() || n < 0.0 {
                    return Err(format!(
                        "--seconds wants a non-negative number, got {value}"
                    ));
                }
                out.seconds = Some(n);
            }
        }
        Ok(out)
    }

    /// Whether the run has no window: `--shot` renders headless.
    pub(crate) fn headless(&self) -> bool {
        self.shot.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<RunArgs, String> {
        RunArgs::parse(args.iter().map(|s| s.to_string()))
    }

    #[test]
    fn flags_and_unknown_arguments() {
        let cases: [(&[&str], RunArgs); 4] = [
            (&[], RunArgs::default()),
            (
                &["--shot", "a.png", "--seconds", "2.5"],
                RunArgs {
                    shot: Some("a.png".into()),
                    seconds: Some(2.5),
                    fixed_clock: false,
                },
            ),
            (
                &["--model=x.glb", "--seconds=3", "-v", "--shot=b.png"],
                RunArgs {
                    shot: Some("b.png".into()),
                    seconds: Some(3.0),
                    fixed_clock: false,
                },
            ),
            (&["--sorted", "extra"], RunArgs::default()),
        ];
        for (args, want) in cases {
            assert_eq!(parse(args), Ok(want), "{args:?}");
        }
    }

    #[test]
    fn bad_values_are_errors() {
        for args in [&["--shot"][..], &["--seconds", "x"], &["--seconds=-1"]] {
            assert!(parse(args).is_err(), "{args:?}");
        }
    }
}
