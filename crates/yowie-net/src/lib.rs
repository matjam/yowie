//! Typed messages over TCP (ordered, optionally TLS) and UDP (fast, with
//! reliable ordered channels); interpolation, prediction, lag compensation,
//! clock sync, snapshot deltas and interest management.
//!
//! Messages are any type implementing `serde::Serialize` and
//! `serde::de::DeserializeOwned`, encoded with postcard. Sockets are plain
//! `std::net` sockets serviced by background threads; the game loop calls
//! `poll()` on a server or client each frame to collect events without
//! blocking. To wake a sleeping event loop when traffic arrives, call
//! `set_waker` on the server or client.
//!
//! - [`tcp`]: [`TcpServer`] and [`TcpClient`], with optional TLS ([`tls`]).
//! - [`udp`]: [`UdpServer`] and [`UdpClient`] with a handshake, unreliable,
//!   sequenced and reliable-ordered [`Channel`]s, RTT and loss estimates.
//! - [`game`]: interpolation, prediction, lag compensation, clock sync,
//!   snapshot deltas and interest management.
#![warn(missing_docs)]

pub mod error;
pub mod frame;
pub mod game;
pub mod tcp;
pub mod tls;
pub mod udp;

pub use error::{ClientId, DisconnectReason, Error, Result, WakerFn};
pub use tcp::{ClientEvent, ConnState, ServerEvent, TcpClient, TcpConfig, TcpServer};
pub use tls::{TlsClient, TlsServer};
pub use udp::{Channel, UdpClient, UdpConfig, UdpServer};
