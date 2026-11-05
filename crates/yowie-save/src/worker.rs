use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crossbeam_channel::{Receiver, Sender, TryRecvError};

use crate::SaveError;

/// Work for the background writer.
pub(crate) enum Job {
    Write {
        name: String,
        bytes: Vec<u8>,
        reply: Sender<Result<(), SaveError>>,
    },
    /// Answered once every earlier job is finished.
    Barrier(Sender<()>),
}

/// The result of a background save, to poll each frame.
///
/// Dropping the handle does not cancel the save.
#[derive(Debug)]
pub struct SaveHandle {
    rx: Receiver<Result<(), SaveError>>,
    result: Option<Result<(), SaveError>>,
}

impl SaveHandle {
    pub(crate) fn new(rx: Receiver<Result<(), SaveError>>) -> Self {
        Self { rx, result: None }
    }

    /// A handle that is already finished with `result`, for failures found
    /// before the job could be queued.
    pub(crate) fn finished(result: Result<(), SaveError>) -> Self {
        let (_, rx) = crossbeam_channel::bounded(0);
        Self {
            rx,
            result: Some(result),
        }
    }

    /// Returns the outcome once the save has finished, or `None` while it
    /// is still queued or running. Does not block.
    pub fn poll(&mut self) -> Option<&Result<(), SaveError>> {
        if self.result.is_none() {
            self.result = match self.rx.try_recv() {
                Ok(r) => Some(r),
                Err(TryRecvError::Empty) => None,
                Err(TryRecvError::Disconnected) => Some(Err(SaveError::WorkerGone)),
            };
        }
        self.result.as_ref()
    }

    /// Returns true once the save has finished, successfully or not.
    pub fn is_done(&mut self) -> bool {
        self.poll().is_some()
    }

    /// Blocks until the save has finished and returns its outcome.
    pub fn wait(mut self) -> Result<(), SaveError> {
        match self.result.take() {
            Some(r) => r,
            None => self.rx.recv().unwrap_or(Err(SaveError::WorkerGone)),
        }
    }
}

/// The loop run by the writer thread. Jobs run in the order sent, so two
/// saves of one slot land in the order they were requested.
pub(crate) fn run(dir: PathBuf, jobs: Receiver<Job>) {
    for job in jobs {
        match job {
            Job::Write { name, bytes, reply } => {
                // The caller may have dropped the handle; that is fine.
                let _ = reply.send(write_atomic(&dir, &name, &bytes));
            }
            Job::Barrier(reply) => {
                let _ = reply.send(());
            }
        }
    }
}

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// Writes `<name>.json` atomically: temporary file, sync, rename.
pub(crate) fn write_atomic(dir: &Path, name: &str, bytes: &[u8]) -> Result<(), SaveError> {
    let io = |source| SaveError::Io {
        name: name.to_string(),
        source,
    };
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let tmp = dir.join(format!(".{name}.{}.{n}.tmp", std::process::id()));
    let result = (|| {
        let mut file = File::create(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&tmp, dir.join(format!("{name}.json")))
    })();
    if let Err(e) = result {
        let _ = fs::remove_file(&tmp);
        return Err(io(e));
    }
    Ok(())
}
