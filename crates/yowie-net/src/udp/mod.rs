//! UDP with a connection protocol: a salted handshake, per-packet sequence
//! numbers with acknowledgement bitfields, three channels, resends and
//! fragmentation for large reliable messages, round-trip and loss
//! estimates, keepalives and timeouts.
//!
//! Datagrams never exceed [`MTU`] bytes. Unreliable and sequenced messages
//! must fit in one datagram; reliable messages of any size up to
//! [`UdpConfig::max_message`] are fragmented and reassembled.
//!
//! One background thread per [`UdpServer`] or [`UdpClient`] reads the
//! socket, runs resends and keepalives, and queues events for `poll()`.

mod ack;
mod client;
#[cfg(test)]
mod e2e_tests;
mod link;
#[cfg(test)]
mod link_tests;
mod lossy;
mod proto;
mod seq;
mod server;
mod socket;

use std::time::Duration;

pub use client::UdpClient;
pub use lossy::{LossConfig, LossySocket};
pub use proto::MTU;
pub use server::UdpServer;
pub use socket::Datagram;

/// How a message is delivered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    /// Sent once; may be lost, duplicated never, and may arrive out of order.
    Unreliable,
    /// Sent once; a message older than one already delivered is dropped.
    Sequenced,
    /// Resent until acknowledged and delivered exactly once, in send order,
    /// with other reliable messages from the same sender.
    Reliable,
}

/// Link quality measurements for one connection.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Stats {
    /// Smoothed round-trip time; zero before the first acknowledgement.
    pub rtt: Duration,
    /// Fraction of recent packets that went unacknowledged, 0 to 1.
    pub loss: f32,
    /// Reliable message fragments sent but not yet acknowledged.
    pub pending: usize,
}

/// Settings for [`UdpServer`] and [`UdpClient`].
#[derive(Debug, Clone)]
pub struct UdpConfig {
    /// Largest message in bytes. Applies to reliable messages (which are
    /// fragmented) in both directions. Default 1 MiB.
    pub max_message: usize,
    /// Reliable fragments that may wait to be acknowledged per connection.
    /// Default 1024.
    pub max_queue: usize,
    /// Commands (sends, disconnects) that may wait for the I/O thread.
    /// Default 4096.
    pub command_queue: usize,
    /// Events that may wait for `poll()`. When full, further events are
    /// dropped. Default 4096.
    pub event_queue: usize,
    /// A connection with no packets for this long is dropped. Default 5 s.
    pub timeout: Duration,
    /// Idle links send a keepalive after this long. Default 500 ms.
    pub keepalive: Duration,
    /// Time a client waits for the handshake to complete. Default 5 s.
    pub connect_timeout: Duration,
    /// Most simultaneous connections a server accepts. Default 64.
    pub max_clients: usize,
    /// Socket read timeout, which is also how often resends and keepalives
    /// are checked; outgoing messages wait at most this long. Default 2 ms.
    pub tick: Duration,
}

impl Default for UdpConfig {
    fn default() -> Self {
        Self {
            max_message: 1 << 20,
            max_queue: 1024,
            command_queue: 4096,
            event_queue: 4096,
            timeout: Duration::from_secs(5),
            keepalive: Duration::from_millis(500),
            connect_timeout: Duration::from_secs(5),
            max_clients: 64,
            tick: Duration::from_millis(2),
        }
    }
}

impl UdpConfig {
    pub(crate) fn link(&self) -> link::LinkConfig {
        link::LinkConfig {
            max_message: self.max_message,
            max_queue: self.max_queue,
            timeout: self.timeout,
            keepalive: self.keepalive,
        }
    }
}

/// Encodes a message, checking the limit of the channel it will use.
pub(crate) fn encode_for<T: serde::Serialize>(
    channel: Channel,
    msg: &T,
    max_message: usize,
) -> crate::error::Result<Vec<u8>> {
    let bytes = postcard::to_stdvec(msg)?;
    let max = match channel {
        Channel::Unreliable => proto::MAX_UNRELIABLE,
        Channel::Sequenced => proto::MAX_SEQUENCED,
        Channel::Reliable => max_message,
    };
    if bytes.len() > max {
        return Err(crate::error::Error::TooLarge {
            size: bytes.len(),
            max,
        });
    }
    Ok(bytes)
}

/// An unpredictable 64-bit value for handshake salts. Seeded by the
/// operating system through the standard library's hash randomisation.
pub(crate) fn random_u64() -> u64 {
    use std::hash::{BuildHasher, Hasher};
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u64(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos() as u64),
    );
    h.finish()
}
