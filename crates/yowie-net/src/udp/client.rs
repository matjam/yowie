//! The UDP client.

use std::marker::PhantomData;
use std::net::{IpAddr, SocketAddr, ToSocketAddrs, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender, TrySendError, bounded};
use serde::Serialize;
use serde::de::DeserializeOwned;

use super::link::Link;
use super::server::is_idle;
use super::socket::Datagram;
use super::{Channel, Stats, UdpConfig, proto, random_u64};
use crate::error::{DisconnectReason, Error, Result, WakeSlot, WakerFn};
use crate::frame::decode_payload;
use crate::tcp::ClientEvent;

const RETRY: Duration = Duration::from_millis(100);

type SharedStats = Arc<Mutex<(bool, Stats)>>;

/// A UDP client. `In` is the message type the server sends; `Out` is the
/// type this client sends.
///
/// [`UdpClient::connect`] returns at once; the handshake runs on a
/// background thread and ends in [`ClientEvent::Connected`] or
/// [`ClientEvent::Disconnected`]. Messages sent before the connection is up
/// wait in the command queue and are sent once it is. Dropping the client
/// sends a goodbye and stops the thread.
pub struct UdpClient<In, Out> {
    cmd: Sender<(Channel, Vec<u8>)>,
    events: Receiver<ClientEvent<In>>,
    stop: Arc<AtomicBool>,
    waker: WakeSlot,
    stats: SharedStats,
    max_message: usize,
    handle: Option<JoinHandle<()>>,
    _out: PhantomData<fn(Out)>,
}

impl<In, Out> UdpClient<In, Out>
where
    In: DeserializeOwned + Send + 'static,
    Out: Serialize,
{
    /// Connects to the server at `server` using a new socket bound to an
    /// OS-assigned port (on loopback when the server is on loopback).
    pub fn connect(server: impl ToSocketAddrs, cfg: UdpConfig) -> Result<Self> {
        let addr = server.to_socket_addrs()?.next().ok_or_else(|| {
            Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "address resolved to nothing",
            ))
        })?;
        let ip: IpAddr = match (addr.is_ipv4(), addr.ip().is_loopback()) {
            (true, true) => [127, 0, 0, 1].into(),
            (true, false) => [0, 0, 0, 0].into(),
            (false, true) => std::net::Ipv6Addr::LOCALHOST.into(),
            (false, false) => std::net::Ipv6Addr::UNSPECIFIED.into(),
        };
        Self::with_socket(UdpSocket::bind((ip, 0))?, addr, cfg)
    }

    /// Connects over an existing socket, for example a
    /// [`LossySocket`](super::LossySocket).
    pub fn with_socket<S: Datagram>(socket: S, server: SocketAddr, cfg: UdpConfig) -> Result<Self> {
        socket.set_timeout(cfg.tick)?;
        let (cmd, cmd_rx) = bounded(cfg.command_queue.max(1));
        let (etx, events) = bounded(cfg.event_queue.max(1));
        let stop = Arc::new(AtomicBool::new(false));
        let waker = WakeSlot::default();
        let stats: SharedStats = Arc::new(Mutex::new((false, Stats::default())));
        let max_message = cfg.max_message;
        let task = Task {
            socket,
            server,
            cfg,
            cmd_rx,
            etx,
            waker: waker.clone(),
            stats: stats.clone(),
        };
        let s2 = stop.clone();
        let handle = thread::Builder::new()
            .name("yowie-net-udp-client".into())
            .spawn(move || task.run(&s2))?;
        Ok(Self {
            cmd,
            events,
            stop,
            waker,
            stats,
            max_message,
            handle: Some(handle),
            _out: PhantomData,
        })
    }

    /// Sets a callback run on the I/O thread whenever an event becomes
    /// available, so a sleeping event loop can wake and call `poll()`.
    pub fn set_waker(&self, waker: WakerFn) {
        self.waker.set(Some(waker));
    }

    /// Returns the next pending event without blocking.
    pub fn poll(&self) -> Option<ClientEvent<In>> {
        self.events.try_recv().ok()
    }

    /// Whether the handshake has completed and the link is up.
    pub fn is_connected(&self) -> bool {
        self.stats.lock().unwrap_or_else(|e| e.into_inner()).0
    }

    /// Link statistics, refreshed about every 100 ms.
    pub fn stats(&self) -> Stats {
        self.stats.lock().unwrap_or_else(|e| e.into_inner()).1
    }

    /// Queues `msg` on `channel`.
    ///
    /// # Errors
    /// [`Error::TooLarge`] for an unreliable or sequenced message that does
    /// not fit one [`MTU`](super::MTU) datagram, [`Error::QueueFull`],
    /// [`Error::Closed`] after the thread has ended, or an encoding error.
    pub fn send(&self, channel: Channel, msg: &Out) -> Result<()> {
        let bytes = super::encode_for(channel, msg, self.max_message)?;
        match self.cmd.try_send((channel, bytes)) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(_)) => Err(Error::QueueFull),
            Err(TrySendError::Disconnected(_)) => Err(Error::Closed),
        }
    }

    /// Sends a goodbye and stops the thread.
    pub fn disconnect(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

impl<In, Out> Drop for UdpClient<In, Out> {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

struct Task<S, In> {
    socket: S,
    server: SocketAddr,
    cfg: UdpConfig,
    cmd_rx: Receiver<(Channel, Vec<u8>)>,
    etx: Sender<ClientEvent<In>>,
    waker: WakeSlot,
    stats: SharedStats,
}

impl<S: Datagram, In: DeserializeOwned> Task<S, In> {
    fn emit(&self, e: ClientEvent<In>) {
        if self.etx.try_send(e).is_ok() {
            self.waker.wake();
        } else {
            log::warn!("udp client event queue full; event dropped");
        }
    }

    fn send(&self, p: &[u8]) {
        if let Err(e) = self.socket.send_to(p, self.server) {
            log::debug!("udp send failed: {e}");
        }
    }

    fn run(self, stop: &AtomicBool) {
        if let Some(link) = self.handshake(stop) {
            self.connected(link, stop);
        }
    }

    /// Returns the link, or `None` after emitting why it failed.
    fn handshake(&self, stop: &AtomicBool) -> Option<Link> {
        let start = Instant::now();
        let client_salt = random_u64();
        let mut server_salt: Option<u64> = None;
        let mut last = None::<Instant>;
        let mut buf = vec![0u8; 2048];
        loop {
            let now = Instant::now();
            if stop.load(Ordering::Acquire) {
                self.emit(ClientEvent::Disconnected(DisconnectReason::Local));
                return None;
            }
            if now.duration_since(start) >= self.cfg.connect_timeout {
                self.emit(ClientEvent::Disconnected(DisconnectReason::Timeout));
                return None;
            }
            if last.is_none_or(|t| now.duration_since(t) >= RETRY) {
                last = Some(now);
                match server_salt {
                    None => self.send(&proto::connect(client_salt)),
                    Some(s) => self.send(&proto::tagged(proto::RESPONSE, client_salt ^ s)),
                }
            }
            let (n, from) = match self.socket.recv_from(&mut buf) {
                Ok(v) => v,
                Err(e) if is_idle(&e) || e.kind() == std::io::ErrorKind::ConnectionReset => {
                    continue;
                }
                Err(e) => {
                    self.emit(ClientEvent::Disconnected(DisconnectReason::Io(
                        e.to_string(),
                    )));
                    return None;
                }
            };
            if from != self.server {
                continue;
            }
            let pkt = &buf[..n];
            match pkt.first().copied() {
                Some(proto::CHALLENGE) => {
                    if let Some((cs, ss)) = proto::parse_challenge(pkt)
                        && cs == client_salt
                    {
                        server_salt = Some(ss);
                        last = None;
                    }
                }
                Some(proto::ACCEPT) => {
                    let ok = server_salt
                        .zip(proto::parse_tagged(pkt))
                        .is_some_and(|(s, session)| session == client_salt ^ s);
                    if ok {
                        let session = client_salt ^ server_salt.unwrap_or(0);
                        return Some(Link::new(session, Instant::now(), self.cfg.link()));
                    }
                }
                Some(proto::REJECT) if proto::parse_tagged(pkt) == Some(client_salt) => {
                    self.emit(ClientEvent::Disconnected(DisconnectReason::Rejected));
                    return None;
                }
                _ => {}
            }
        }
    }

    fn connected(&self, mut link: Link, stop: &AtomicBool) {
        self.set_stats(true, link.stats());
        self.emit(ClientEvent::Connected);
        let mut buf = vec![0u8; 2048];
        let mut out = Vec::new();
        let mut last_stats = Instant::now();
        let reason = loop {
            if stop.load(Ordering::Acquire) {
                let bye = proto::tagged(proto::DISCONNECT, link.session);
                for _ in 0..3 {
                    self.send(&bye);
                }
                break DisconnectReason::Local;
            }
            while let Ok((ch, data)) = self.cmd_rx.try_recv() {
                if let Err(e) = link.send(ch, &data) {
                    log::warn!("udp send failed: {e}");
                }
            }
            let mut ended = None;
            for _ in 0..256 {
                match self.socket.recv_from(&mut buf) {
                    Ok((n, from)) if from == self.server => {
                        ended = ended.or(self.on_packet(&mut link, &buf[..n]));
                    }
                    Ok(_) => {}
                    Err(e) if is_idle(&e) => break,
                    Err(e) if e.kind() == std::io::ErrorKind::ConnectionReset => {}
                    Err(e) => {
                        ended = Some(DisconnectReason::Io(e.to_string()));
                        break;
                    }
                }
            }
            if let Some(r) = ended {
                break r;
            }
            let now = Instant::now();
            if link.timed_out(now) {
                break DisconnectReason::Timeout;
            }
            link.poll_transmit(now, &mut out);
            for p in out.drain(..) {
                self.send(&p);
            }
            if now.duration_since(last_stats) >= Duration::from_millis(100) {
                last_stats = now;
                self.set_stats(true, link.stats());
            }
        };
        self.set_stats(false, link.stats());
        self.emit(ClientEvent::Disconnected(reason));
    }

    fn on_packet(&self, link: &mut Link, pkt: &[u8]) -> Option<DisconnectReason> {
        match pkt.first().copied() {
            Some(proto::DISCONNECT) if proto::parse_tagged(pkt) == Some(link.session) => {
                Some(DisconnectReason::Remote)
            }
            Some(proto::ACK..=proto::RELIABLE) => {
                let mut delivered = Vec::new();
                if let Err(e) = link.handle(Instant::now(), pkt, &mut delivered) {
                    return Some(DisconnectReason::Protocol(e.to_string()));
                }
                for payload in delivered {
                    match decode_payload::<In>(&payload) {
                        Ok(m) => self.emit(ClientEvent::Message(m)),
                        Err(e) => log::debug!("udp message undecodable: {e}"),
                    }
                }
                None
            }
            _ => None,
        }
    }

    fn set_stats(&self, connected: bool, stats: Stats) {
        *self.stats.lock().unwrap_or_else(|e| e.into_inner()) = (connected, stats);
    }
}
