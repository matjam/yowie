//! The crate error type and the types shared by TCP and UDP.

use std::fmt;
use std::io;
use std::sync::{Arc, Mutex};

/// Errors returned by this crate.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A socket or file operation failed.
    #[error("i/o error: {0}")]
    Io(#[from] io::Error),
    /// A message could not be encoded or decoded.
    #[error("message codec error: {0}")]
    Codec(#[from] postcard::Error),
    /// A message is larger than the configured or protocol limit.
    #[error("message of {size} bytes exceeds the limit of {max} bytes")]
    TooLarge {
        /// Size of the rejected message in bytes.
        size: usize,
        /// The limit that applies, in bytes.
        max: usize,
    },
    /// A send queue is full; the message was not queued.
    #[error("send queue is full")]
    QueueFull,
    /// The connection or endpoint is closed.
    #[error("connection closed")]
    Closed,
    /// No connected client has this id.
    #[error("unknown client {0}")]
    UnknownClient(ClientId),
    /// TLS configuration or handshake failed.
    #[error("tls error: {0}")]
    Tls(#[from] rustls::Error),
    /// Certificate or key PEM data could not be read.
    #[error("invalid certificate or key: {0}")]
    Pem(String),
    /// The peer broke the wire protocol.
    #[error("protocol error: {0}")]
    Protocol(String),
    /// A snapshot delta refers to a baseline that is not stored.
    #[error("baseline snapshot {0} is not available")]
    MissingBaseline(u32),
    /// A snapshot delta is malformed.
    #[error("corrupt delta: {0}")]
    BadDelta(&'static str),
}

/// Result alias using [`Error`].
pub type Result<T> = std::result::Result<T, Error>;

/// Identifies one connected client on a server. Ids are never reused within
/// the life of a server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ClientId(pub u64);

impl fmt::Display for ClientId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{}", self.0)
    }
}

/// Why a connection ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DisconnectReason {
    /// This side closed the connection.
    Local,
    /// The other side closed the connection.
    Remote,
    /// The other side went silent, or a connection attempt timed out.
    Timeout,
    /// The server refused the connection (for example, it was full).
    Rejected,
    /// The peer sent data that breaks the wire protocol.
    Protocol(String),
    /// A socket error ended the connection.
    Io(String),
}

/// A callback invoked from a background thread when an event is ready for
/// `poll()`. It must be quick and must not block.
pub type WakerFn = Arc<dyn Fn() + Send + Sync>;

/// Shared slot holding the optional waker.
#[derive(Clone, Default)]
pub(crate) struct WakeSlot(Arc<Mutex<Option<WakerFn>>>);

impl WakeSlot {
    pub(crate) fn set(&self, waker: Option<WakerFn>) {
        *self.0.lock().unwrap_or_else(|e| e.into_inner()) = waker;
    }

    pub(crate) fn wake(&self) {
        let waker = self.0.lock().unwrap_or_else(|e| e.into_inner()).clone();
        if let Some(w) = waker {
            w();
        }
    }
}
