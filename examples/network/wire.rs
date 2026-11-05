//! The messages both ends of the example exchange. Any type that
//! implements `serde::Serialize` and `Deserialize` can be a message.

use serde::{Deserialize, Serialize};

/// What a client says to the host over TCP.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum Up {
    /// The client's name, sent once connected.
    Join(String),
    /// A chat line.
    Chat { from: String, text: String },
}

/// What the host says to a client over TCP.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum Down {
    /// The host's UDP port, so the client can open its pointer link.
    Welcome { udp_port: u16 },
    /// A chat line.
    Chat { from: String, text: String },
}

/// What travels over UDP in both directions.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum Wire {
    /// Where a player's pointer is, in view units.
    Pointer { name: String, x: f32, y: f32 },
    /// A chat line sent over the reliable channel (`--reliable`); `seq`
    /// counts the sender's lines so the receiver can check their order.
    Chat {
        from: String,
        text: String,
        seq: u32,
    },
}

/// Turns `:7777` into `0.0.0.0:7777`; other addresses stay as given.
pub fn listen_address(arg: &str) -> String {
    if arg.starts_with(':') {
        format!("0.0.0.0{arg}")
    } else {
        arg.to_owned()
    }
}

/// The host part of `host:port`.
pub fn host_of(addr: &str) -> &str {
    addr.rsplit_once(':').map_or(addr, |(host, _)| host)
}
