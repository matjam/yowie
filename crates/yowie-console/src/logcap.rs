//! Capturing log records for the console while passing them on.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, OnceLock};

use log::{Level, LevelFilter, Log, Metadata, Record, SetLoggerError};

/// One captured log record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogLine {
    /// The record's level.
    pub level: Level,
    /// The module that logged it.
    pub target: String,
    /// The formatted message.
    pub text: String,
}

#[derive(Debug)]
struct Ring {
    lines: VecDeque<LogLine>,
    cap: usize,
    level: LevelFilter,
    /// The most verbose level the logger behind the tee accepts.
    passthrough: LevelFilter,
    pushed: u64,
}

/// A shared ring of the most recent log records. Cloning gives another
/// handle to the same ring.
#[derive(Clone, Debug)]
pub struct LogCapture {
    ring: Arc<Mutex<Ring>>,
}

static GLOBAL: OnceLock<LogCapture> = OnceLock::new();

impl LogCapture {
    /// A ring that keeps the last `cap` records at `Info` and above.
    pub fn new(cap: usize) -> LogCapture {
        LogCapture {
            ring: Arc::new(Mutex::new(Ring {
                lines: VecDeque::new(),
                cap: cap.max(1),
                level: LevelFilter::Info,
                passthrough: LevelFilter::Off,
                pushed: 0,
            })),
        }
    }

    /// The ring fed by the logger [`install_logger`] set up, if that ran.
    pub fn global() -> Option<LogCapture> {
        GLOBAL.get().cloned()
    }

    fn with<R>(&self, f: impl FnOnce(&mut Ring) -> R) -> Option<R> {
        self.ring.lock().ok().map(|mut g| f(&mut g))
    }

    /// Adds a record, dropping the oldest when full.
    pub fn push(&self, line: LogLine) {
        self.with(|r| {
            if r.lines.len() >= r.cap {
                r.lines.pop_front();
            }
            r.lines.push_back(line);
            r.pushed += 1;
        });
    }

    /// How many records have ever been added, for noticing new ones.
    pub fn pushed(&self) -> u64 {
        self.with(|r| r.pushed).unwrap_or(0)
    }

    /// How many records are held.
    pub fn len(&self) -> usize {
        self.with(|r| r.lines.len()).unwrap_or(0)
    }

    /// Whether no records are held.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Up to `count` records ending `skip` records before the newest,
    /// oldest first.
    pub fn window(&self, count: usize, skip: usize) -> Vec<LogLine> {
        self.with(|r| {
            let end = r.lines.len().saturating_sub(skip);
            let start = end.saturating_sub(count);
            r.lines.range(start..end).cloned().collect()
        })
        .unwrap_or_default()
    }

    /// Empties the ring.
    pub fn clear(&self) {
        self.with(|r| r.lines.clear());
    }

    /// The level at and above which records are captured.
    pub fn level(&self) -> LevelFilter {
        self.with(|r| r.level).unwrap_or(LevelFilter::Info)
    }

    /// Captures records at `level` and above from now on. Raises the
    /// global maximum level when the logger behind the tee would not pass
    /// them anyway.
    pub fn set_level(&self, level: LevelFilter) {
        let pass = self
            .with(|r| {
                r.level = level;
                r.passthrough
            })
            .unwrap_or(LevelFilter::Off);
        log::set_max_level(level.max(pass));
    }
}

/// A logger that copies records into a [`LogCapture`] and then hands them
/// to the logger it replaced.
pub struct TeeLogger {
    capture: LogCapture,
    previous: Option<Box<dyn Log>>,
}

impl Log for TeeLogger {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        metadata.level() <= self.capture.level()
            || self.previous.as_ref().is_some_and(|p| p.enabled(metadata))
    }

    fn log(&self, record: &Record<'_>) {
        if record.level() <= self.capture.level() {
            self.capture.push(LogLine {
                level: record.level(),
                target: record.target().to_owned(),
                text: record.args().to_string(),
            });
        }
        if let Some(p) = &self.previous {
            p.log(record);
        }
    }

    fn flush(&self) {
        if let Some(p) = &self.previous {
            p.flush();
        }
    }
}

/// Installs a logger that records into the global [`LogCapture`] and
/// passes every record on to `previous`, which keeps its own filtering.
/// `previous_filter` is the most verbose level `previous` accepts, so the
/// global maximum level stays high enough for it. Fails when a logger is
/// already installed.
pub fn install_logger(
    previous: Option<Box<dyn Log>>,
    previous_filter: LevelFilter,
    capacity: usize,
) -> Result<LogCapture, SetLoggerError> {
    let capture = GLOBAL.get_or_init(|| LogCapture::new(capacity)).clone();
    capture.with(|r| r.passthrough = previous_filter);
    log::set_boxed_logger(Box::new(TeeLogger {
        capture: capture.clone(),
        previous,
    }))?;
    capture.set_level(capture.level());
    Ok(capture)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(n: usize) -> LogLine {
        LogLine {
            level: Level::Info,
            target: "t".into(),
            text: format!("line {n}"),
        }
    }

    #[test]
    fn the_ring_keeps_the_newest_records() {
        let c = LogCapture::new(3);
        for i in 0..5 {
            c.push(line(i));
        }
        assert_eq!(c.len(), 3);
        assert_eq!(c.pushed(), 5);
        let texts: Vec<String> = c.window(10, 0).into_iter().map(|l| l.text).collect();
        assert_eq!(texts, ["line 2", "line 3", "line 4"]);
        let older: Vec<String> = c.window(2, 1).into_iter().map(|l| l.text).collect();
        assert_eq!(older, ["line 2", "line 3"]);
        c.clear();
        assert!(c.is_empty());
    }

    #[test]
    fn the_tee_captures_by_level_and_forwards() {
        struct Sink(Arc<Mutex<Vec<String>>>);
        impl Log for Sink {
            fn enabled(&self, _: &Metadata<'_>) -> bool {
                true
            }
            fn log(&self, r: &Record<'_>) {
                if let Ok(mut v) = self.0.lock() {
                    v.push(r.args().to_string());
                }
            }
            fn flush(&self) {}
        }
        let seen = Arc::new(Mutex::new(Vec::new()));
        let capture = LogCapture::new(10);
        let tee = TeeLogger {
            capture: capture.clone(),
            previous: Some(Box::new(Sink(seen.clone()))),
        };
        let rec = |level, msg: &'static str| {
            tee.log(
                &Record::builder()
                    .level(level)
                    .target("test")
                    .args(format_args!("{msg}"))
                    .build(),
            );
        };
        rec(Level::Info, "kept");
        rec(Level::Debug, "debug hidden at first");
        capture.set_level(LevelFilter::Debug);
        rec(Level::Debug, "debug kept");
        let texts: Vec<String> = capture.window(10, 0).into_iter().map(|l| l.text).collect();
        assert_eq!(texts, ["kept", "debug kept"]);
        let forwarded = seen.lock().map(|v| v.len()).unwrap_or(0);
        assert_eq!(forwarded, 3, "every record reaches the previous logger");
    }
}
