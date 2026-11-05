//! TCP server and client with length-prefixed typed messages and optional
//! TLS. Each connection runs on its own background thread; the game loop
//! calls `poll()` to collect events without blocking.

mod client;
mod io;
mod server;
mod stream;
#[cfg(test)]
mod tests;

use std::sync::Arc;
use std::time::Duration;

use crossbeam_channel::Sender;

use crate::error::WakeSlot;

pub use client::{ClientEvent, ConnState, TcpClient};
pub use server::{ServerEvent, TcpServer};

/// Settings shared by [`TcpServer`] and [`TcpClient`].
#[derive(Debug, Clone)]
pub struct TcpConfig {
    /// Largest message payload in bytes, in either direction. A peer that
    /// announces a larger frame is disconnected. Default 1 MiB.
    pub max_message: usize,
    /// Messages that may wait to be written per connection. Default 256.
    pub send_queue: usize,
    /// Events that may wait for `poll()`. When full, connection threads
    /// pause reading. Default 4096.
    pub event_queue: usize,
    /// Most simultaneous connections a server accepts. Default 64.
    pub max_clients: usize,
    /// How often a connection thread checks its send queue and the stop
    /// flag while the socket is idle. Outgoing messages wait at most this
    /// long. Default 5 ms.
    pub tick: Duration,
    /// Time allowed for a TLS handshake. Default 10 s.
    pub handshake_timeout: Duration,
    /// Time allowed for a client to establish the TCP connection. Default 5 s.
    pub connect_timeout: Duration,
    /// Time a single socket write may block before the connection is
    /// dropped. Default 10 s.
    pub write_timeout: Duration,
}

impl Default for TcpConfig {
    fn default() -> Self {
        Self {
            max_message: 1 << 20,
            send_queue: 256,
            event_queue: 4096,
            max_clients: 64,
            tick: Duration::from_millis(5),
            handshake_timeout: Duration::from_secs(10),
            connect_timeout: Duration::from_secs(5),
            write_timeout: Duration::from_secs(10),
        }
    }
}

/// A pre-framed message shared between queues so a broadcast encodes once.
pub(crate) type Frame = Arc<Vec<u8>>;

/// Delivers events to the polling side, waking the game loop.
pub(crate) struct EventTx<E> {
    pub(crate) tx: Sender<E>,
    pub(crate) waker: WakeSlot,
}

impl<E> Clone for EventTx<E> {
    fn clone(&self) -> Self {
        Self {
            tx: self.tx.clone(),
            waker: self.waker.clone(),
        }
    }
}

impl<E> EventTx<E> {
    /// Queues an event, waiting while the queue is full. Returns false when
    /// `stopped()` becomes true or the receiver is gone.
    pub(crate) fn push(&self, mut event: E, stopped: impl Fn() -> bool) -> bool {
        loop {
            match self.tx.send_timeout(event, Duration::from_millis(10)) {
                Ok(()) => {
                    self.waker.wake();
                    return true;
                }
                Err(crossbeam_channel::SendTimeoutError::Timeout(e)) => {
                    if stopped() {
                        return false;
                    }
                    event = e;
                }
                Err(crossbeam_channel::SendTimeoutError::Disconnected(_)) => return false,
            }
        }
    }
}
