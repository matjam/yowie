//! The TCP client.

use std::marker::PhantomData;
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::thread::{self, JoinHandle};

use crossbeam_channel::{Receiver, Sender, TrySendError, bounded};
use serde::Serialize;
use serde::de::DeserializeOwned;

use super::stream::Stream;
use super::{EventTx, Frame, TcpConfig, io};
use crate::error::{DisconnectReason, Error, Result, WakeSlot, WakerFn};
use crate::frame::{decode_payload, encode_frame};
use crate::tls::TlsClient;

/// Something that happened on a [`TcpClient`], returned by `poll()`.
#[derive(Debug)]
pub enum ClientEvent<T> {
    /// The connection (and TLS handshake, if any) is established.
    Connected,
    /// The connection ended, or could not be established.
    Disconnected(DisconnectReason),
    /// The server sent a message.
    Message(T),
}

/// Connection state of a [`TcpClient`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnState {
    /// The background thread is connecting.
    Connecting,
    /// The connection is up.
    Connected,
    /// The connection ended or was never established.
    Closed,
}

const CONNECTING: u8 = 0;
const CONNECTED: u8 = 1;
const CLOSED: u8 = 2;

/// A TCP client. `In` is the type the server sends; `Out` is the type this
/// client sends.
///
/// [`TcpClient::connect`] returns at once; a background thread connects and
/// reports the outcome as a [`ClientEvent`]. Messages sent while connecting
/// are queued and written after the connection is up. Dropping the client
/// cancels a connection attempt in progress or closes the connection after
/// writing queued messages.
pub struct TcpClient<In, Out> {
    tx: Sender<Frame>,
    events: Receiver<ClientEvent<In>>,
    stop: Arc<AtomicBool>,
    state: Arc<AtomicU8>,
    waker: WakeSlot,
    max_message: usize,
    handle: Option<JoinHandle<()>>,
    _out: PhantomData<fn(Out)>,
}

impl<In, Out> TcpClient<In, Out>
where
    In: DeserializeOwned + Send + 'static,
    Out: Serialize,
{
    /// Starts connecting to `addr` over plain TCP. Name resolution happens
    /// here; the connection itself is made in the background.
    pub fn connect(addr: impl ToSocketAddrs, cfg: TcpConfig) -> Result<Self> {
        Self::start(addr, cfg, None)
    }

    /// Starts connecting to `addr` over TLS.
    pub fn connect_tls(addr: impl ToSocketAddrs, cfg: TcpConfig, tls: TlsClient) -> Result<Self> {
        Self::start(addr, cfg, Some(tls))
    }

    fn start(addr: impl ToSocketAddrs, cfg: TcpConfig, tls: Option<TlsClient>) -> Result<Self> {
        let addrs: Vec<SocketAddr> = addr.to_socket_addrs()?.collect();
        if addrs.is_empty() {
            return Err(Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "address resolved to nothing",
            )));
        }
        let (tx, rx) = bounded(cfg.send_queue.max(1));
        let (etx, events) = bounded(cfg.event_queue.max(1));
        let waker = WakeSlot::default();
        let stop = Arc::new(AtomicBool::new(false));
        let state = Arc::new(AtomicU8::new(CONNECTING));
        let max_message = cfg.max_message;
        let (s2, st2) = (stop.clone(), state.clone());
        let etx = EventTx {
            tx: etx,
            waker: waker.clone(),
        };
        let handle = thread::Builder::new()
            .name("yowie-net-client".into())
            .spawn(move || run(addrs, cfg, tls, rx, etx, s2, st2))?;
        Ok(Self {
            tx,
            events,
            stop,
            state,
            waker,
            max_message,
            handle: Some(handle),
            _out: PhantomData,
        })
    }

    /// The current connection state.
    pub fn state(&self) -> ConnState {
        match self.state.load(Ordering::Acquire) {
            CONNECTING => ConnState::Connecting,
            CONNECTED => ConnState::Connected,
            _ => ConnState::Closed,
        }
    }

    /// Sets a callback run on a background thread whenever an event becomes
    /// available, so a sleeping event loop can wake and call `poll()`.
    pub fn set_waker(&self, waker: WakerFn) {
        self.waker.set(Some(waker));
    }

    /// Returns the next pending event without blocking.
    pub fn poll(&self) -> Option<ClientEvent<In>> {
        self.events.try_recv().ok()
    }

    /// Queues `msg` for the server.
    ///
    /// # Errors
    /// [`Error::Closed`] once the connection has ended, [`Error::QueueFull`]
    /// when the send queue is full, or an encoding or size error.
    pub fn send(&self, msg: &Out) -> Result<()> {
        if self.state() == ConnState::Closed {
            return Err(Error::Closed);
        }
        let frame = Arc::new(encode_frame(msg, self.max_message)?);
        match self.tx.try_send(frame) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(_)) => Err(Error::QueueFull),
            Err(TrySendError::Disconnected(_)) => Err(Error::Closed),
        }
    }

    /// Closes the connection after queued messages are written, or cancels
    /// a connection attempt in progress, and waits for the thread to end.
    pub fn disconnect(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

impl<In, Out> Drop for TcpClient<In, Out> {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

fn run<In: DeserializeOwned>(
    addrs: Vec<SocketAddr>,
    cfg: TcpConfig,
    tls: Option<TlsClient>,
    rx: Receiver<Frame>,
    etx: EventTx<ClientEvent<In>>,
    stop: Arc<AtomicBool>,
    state: Arc<AtomicU8>,
) {
    let stopped = || stop.load(Ordering::Acquire);
    let finish = |reason: DisconnectReason| {
        state.store(CLOSED, Ordering::Release);
        etx.push(ClientEvent::Disconnected(reason), stopped);
    };
    let mut last_err = String::from("no address to connect to");
    let mut sock = None;
    for addr in &addrs {
        if stopped() {
            return finish(DisconnectReason::Local);
        }
        match TcpStream::connect_timeout(addr, cfg.connect_timeout) {
            Ok(s) => {
                sock = Some(s);
                break;
            }
            Err(e) => last_err = format!("{addr}: {e}"),
        }
    }
    let Some(sock) = sock else {
        return finish(DisconnectReason::Io(last_err));
    };
    let stream = Stream::connect(
        sock,
        tls.as_ref().map(|t| (&t.config, t.name.clone())),
        cfg.handshake_timeout,
    );
    let mut stream = match stream {
        Ok(s) => s,
        Err(e) => return finish(DisconnectReason::Io(e.to_string())),
    };
    if let Err(e) = stream.set_timeouts(cfg.tick, cfg.write_timeout) {
        return finish(DisconnectReason::Io(e.to_string()));
    }
    state.store(CONNECTED, Ordering::Release);
    if !etx.push(ClientEvent::Connected, stopped) {
        state.store(CLOSED, Ordering::Release);
        return;
    }
    let reason = io::run(&mut stream, &rx, cfg.max_message, stopped, |payload| {
        let message = decode_payload::<In>(&payload)
            .map_err(|e| DisconnectReason::Protocol(e.to_string()))?;
        if etx.push(ClientEvent::Message(message), stopped) {
            Ok(())
        } else {
            Err(DisconnectReason::Local)
        }
    });
    stream.close();
    finish(reason);
}
