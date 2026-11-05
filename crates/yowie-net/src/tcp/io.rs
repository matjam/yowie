//! The per-connection I/O loop shared by the server and client.

use std::io::{ErrorKind, Read, Write};

use crossbeam_channel::{Receiver, TryRecvError};

use super::Frame;
use super::stream::Stream;
use crate::error::DisconnectReason;
use crate::frame::FrameReader;

/// Runs until the connection ends and returns why.
///
/// Each pass writes every queued frame, then reads with the stream's read
/// timeout (the tick). `stopped` is checked once per pass, after the send
/// queue has been drained, so frames queued before a local close are
/// still written. `on_frame` receives each complete payload and returns
/// false to end the connection.
pub(crate) fn run(
    stream: &mut Stream,
    out: &Receiver<Frame>,
    max_message: usize,
    stopped: impl Fn() -> bool,
    mut on_frame: impl FnMut(Vec<u8>) -> Result<(), DisconnectReason>,
) -> DisconnectReason {
    let mut reader = FrameReader::new(max_message);
    let mut buf = vec![0u8; 16 * 1024];
    let mut orphaned = false;
    loop {
        loop {
            match out.try_recv() {
                Ok(frame) => {
                    if let Err(e) = stream.write_all(&frame) {
                        return DisconnectReason::Io(e.to_string());
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    orphaned = true;
                    break;
                }
            }
        }
        if let Err(e) = stream.flush() {
            return DisconnectReason::Io(e.to_string());
        }
        if orphaned || stopped() {
            return DisconnectReason::Local;
        }
        match stream.read(&mut buf) {
            Ok(0) => return DisconnectReason::Remote,
            Ok(n) => {
                reader.push(&buf[..n]);
                loop {
                    match reader.next_frame() {
                        Ok(Some(payload)) => {
                            if let Err(reason) = on_frame(payload) {
                                return reason;
                            }
                        }
                        Ok(None) => break,
                        Err(e) => return DisconnectReason::Protocol(e.to_string()),
                    }
                }
            }
            Err(e) => match e.kind() {
                ErrorKind::WouldBlock | ErrorKind::TimedOut | ErrorKind::Interrupted => {}
                ErrorKind::UnexpectedEof
                | ErrorKind::ConnectionReset
                | ErrorKind::ConnectionAborted
                | ErrorKind::BrokenPipe => return DisconnectReason::Remote,
                _ => return DisconnectReason::Io(e.to_string()),
            },
        }
    }
}
