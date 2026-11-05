//! The crate's error type.

/// Everything that can fail in `yowie-audio`.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The host has no audio device of the requested kind.
    #[error("no audio device available")]
    NoDevice,
    /// The audio backend reported a failure while enumerating, opening or running a device.
    #[error("audio device: {0}")]
    Device(#[from] cpal::Error),
    /// The device or the data uses a format this crate does not handle.
    #[error("unsupported audio format: {0}")]
    Unsupported(String),
    /// A file could not be decoded.
    #[error("decode: {0}")]
    Decode(String),
    /// A tracker module could not be parsed.
    #[error("module: {0}")]
    Module(String),
    /// A file or stream could not be read or written.
    #[error("i/o: {0}")]
    Io(#[from] std::io::Error),
    /// An argument was outside its documented range.
    #[error("invalid argument: {0}")]
    Invalid(&'static str),
    /// The mixer has no room for another bus.
    #[error("the mixer is limited to {0} buses")]
    TooManyBuses(usize),
    /// A recording reached its length limit before it was stopped.
    #[error("capture overran: {0} frames were dropped")]
    CaptureDropped(u64),
}

/// A `Result` that defaults to this crate's [`Error`].
pub type Result<T, E = Error> = std::result::Result<T, E>;
