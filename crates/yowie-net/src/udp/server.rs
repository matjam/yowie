//! The UDP server.

use std::collections::HashMap;
use std::io;
use std::marker::PhantomData;
use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender, TrySendError, bounded};
use serde::Serialize;
use serde::de::DeserializeOwned;

use super::link::Link;
use super::socket::Datagram;
use super::{Channel, Stats, UdpConfig, proto, random_u64};
use crate::error::{ClientId, DisconnectReason, Error, Result, WakeSlot, WakerFn};
use crate::frame::decode_payload;
use crate::tcp::ServerEvent;

const PENDING_TTL: Duration = Duration::from_secs(5);
const MAX_PENDING: usize = 256;

enum Command {
    Send(ClientId, Channel, Arc<Vec<u8>>),
    Broadcast(Channel, Arc<Vec<u8>>, Option<ClientId>),
    Disconnect(ClientId),
}

#[derive(Default)]
struct Registry {
    clients: HashMap<ClientId, (SocketAddr, Stats)>,
}

/// A UDP server. `In` is the message type clients send; `Out` is the type
/// the server sends. Messages are postcard-encoded.
///
/// A background thread owns the socket. Call [`UdpServer::poll`] each
/// frame for events. Dropping the server tells every client goodbye and
/// stops the thread.
pub struct UdpServer<In, Out> {
    cmd: Sender<Command>,
    events: Receiver<ServerEvent<In>>,
    stop: Arc<AtomicBool>,
    waker: WakeSlot,
    registry: Arc<Mutex<Registry>>,
    addr: SocketAddr,
    max_message: usize,
    handle: Option<JoinHandle<()>>,
    _out: PhantomData<fn(Out)>,
}

impl<In, Out> UdpServer<In, Out>
where
    In: DeserializeOwned + Send + 'static,
    Out: Serialize,
{
    /// Binds a UDP socket on `addr` (port 0 picks a free port) and starts
    /// serving.
    pub fn bind(addr: impl ToSocketAddrs, cfg: UdpConfig) -> Result<Self> {
        Self::with_socket(UdpSocket::bind(addr)?, cfg)
    }

    /// Serves on an existing socket, for example a
    /// [`LossySocket`](super::LossySocket).
    pub fn with_socket<S: Datagram>(socket: S, cfg: UdpConfig) -> Result<Self> {
        socket.set_timeout(cfg.tick)?;
        let addr = socket.local_addr()?;
        let (cmd, cmd_rx) = bounded(cfg.command_queue.max(1));
        let (etx, events) = bounded(cfg.event_queue.max(1));
        let stop = Arc::new(AtomicBool::new(false));
        let waker = WakeSlot::default();
        let registry = Arc::new(Mutex::new(Registry::default()));
        let max_message = cfg.max_message;
        let mut core = Core {
            socket,
            cfg,
            cmd_rx,
            etx,
            waker: waker.clone(),
            registry: registry.clone(),
            conns: HashMap::new(),
            by_id: HashMap::new(),
            pending: HashMap::new(),
            next_id: 1,
            last_stats: Instant::now(),
            _in: PhantomData::<fn() -> In>,
        };
        let s2 = stop.clone();
        let handle = thread::Builder::new()
            .name("yowie-net-udp-server".into())
            .spawn(move || core.run(&s2))?;
        Ok(Self {
            cmd,
            events,
            stop,
            waker,
            registry,
            addr,
            max_message,
            handle: Some(handle),
            _out: PhantomData,
        })
    }

    /// The address the socket is bound to.
    pub fn local_addr(&self) -> SocketAddr {
        self.addr
    }

    /// Sets a callback run on the I/O thread whenever an event becomes
    /// available, so a sleeping event loop can wake and call `poll()`.
    pub fn set_waker(&self, waker: WakerFn) {
        self.waker.set(Some(waker));
    }

    /// Returns the next pending event without blocking.
    pub fn poll(&self) -> Option<ServerEvent<In>> {
        self.events.try_recv().ok()
    }

    /// Ids of the connected clients.
    pub fn clients(&self) -> Vec<ClientId> {
        self.reg().clients.keys().copied().collect()
    }

    /// Link statistics for a client, refreshed about every 100 ms.
    pub fn stats(&self, id: ClientId) -> Option<Stats> {
        self.reg().clients.get(&id).map(|c| c.1)
    }

    fn reg(&self) -> std::sync::MutexGuard<'_, Registry> {
        self.registry.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn encode(&self, channel: Channel, msg: &Out) -> Result<Arc<Vec<u8>>> {
        super::encode_for(channel, msg, self.max_message).map(Arc::new)
    }

    fn submit(&self, c: Command) -> Result<()> {
        match self.cmd.try_send(c) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(_)) => Err(Error::QueueFull),
            Err(TrySendError::Disconnected(_)) => Err(Error::Closed),
        }
    }

    /// Queues `msg` for one client on `channel`.
    ///
    /// # Errors
    /// [`Error::UnknownClient`] if the client is not connected,
    /// [`Error::TooLarge`] for an unreliable or sequenced message that does
    /// not fit one [`MTU`](super::MTU) datagram, [`Error::QueueFull`], or an
    /// encoding error.
    pub fn send(&self, id: ClientId, channel: Channel, msg: &Out) -> Result<()> {
        if !self.reg().clients.contains_key(&id) {
            return Err(Error::UnknownClient(id));
        }
        self.submit(Command::Send(id, channel, self.encode(channel, msg)?))
    }

    /// Queues `msg` for every client.
    pub fn broadcast(&self, channel: Channel, msg: &Out) -> Result<()> {
        self.submit(Command::Broadcast(
            channel,
            self.encode(channel, msg)?,
            None,
        ))
    }

    /// Queues `msg` for every client except `except`.
    pub fn broadcast_except(&self, channel: Channel, msg: &Out, except: ClientId) -> Result<()> {
        let bytes = self.encode(channel, msg)?;
        self.submit(Command::Broadcast(channel, bytes, Some(except)))
    }

    /// Disconnects a client, telling it goodbye.
    pub fn disconnect(&self, id: ClientId) {
        let _ = self.submit(Command::Disconnect(id));
    }
}

impl<In, Out> Drop for UdpServer<In, Out> {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

struct Conn {
    id: ClientId,
    link: Link,
}

struct Pending {
    client_salt: u64,
    server_salt: u64,
    at: Instant,
}

struct Core<S, In> {
    socket: S,
    cfg: UdpConfig,
    cmd_rx: Receiver<Command>,
    etx: crossbeam_channel::Sender<ServerEvent<In>>,
    waker: WakeSlot,
    registry: Arc<Mutex<Registry>>,
    conns: HashMap<SocketAddr, Conn>,
    by_id: HashMap<ClientId, SocketAddr>,
    pending: HashMap<SocketAddr, Pending>,
    next_id: u64,
    last_stats: Instant,
    _in: PhantomData<fn() -> In>,
}

impl<S: Datagram, In: DeserializeOwned> Core<S, In> {
    fn run(&mut self, stop: &AtomicBool) {
        let mut buf = vec![0u8; 2048];
        while !stop.load(Ordering::Acquire) {
            self.drain_commands();
            for _ in 0..256 {
                match self.socket.recv_from(&mut buf) {
                    Ok((n, from)) => self.on_packet(Instant::now(), &buf[..n], from),
                    Err(e) if is_idle(&e) => break,
                    Err(e) if e.kind() == io::ErrorKind::ConnectionReset => {}
                    Err(e) => {
                        log::warn!("udp receive failed: {e}");
                        thread::sleep(self.cfg.tick);
                        break;
                    }
                }
            }
            self.tick(Instant::now());
        }
        for (addr, c) in &self.conns {
            let bye = proto::tagged(proto::DISCONNECT, c.link.session);
            for _ in 0..3 {
                let _ = self.socket.send_to(&bye, *addr);
            }
        }
    }

    fn emit(&self, e: ServerEvent<In>) {
        if self.etx.try_send(e).is_ok() {
            self.waker.wake();
        } else {
            log::warn!("udp server event queue full; event dropped");
        }
    }

    fn drain_commands(&mut self) {
        while let Ok(c) = self.cmd_rx.try_recv() {
            match c {
                Command::Send(id, ch, data) => self.queue(id, ch, &data),
                Command::Broadcast(ch, data, except) => {
                    let ids: Vec<_> = self.by_id.keys().copied().collect();
                    for id in ids.into_iter().filter(|i| Some(*i) != except) {
                        self.queue(id, ch, &data);
                    }
                }
                Command::Disconnect(id) => {
                    if let Some(addr) = self.by_id.get(&id).copied() {
                        let session = self.conns.get(&addr).map(|c| c.link.session);
                        if let Some(s) = session {
                            let _ = self
                                .socket
                                .send_to(&proto::tagged(proto::DISCONNECT, s), addr);
                        }
                        self.drop_conn(addr, DisconnectReason::Local);
                    }
                }
            }
        }
    }

    fn queue(&mut self, id: ClientId, ch: Channel, data: &[u8]) {
        let Some(addr) = self.by_id.get(&id) else {
            return;
        };
        if let Some(c) = self.conns.get_mut(addr)
            && let Err(e) = c.link.send(ch, data)
        {
            log::warn!("udp send to {id} failed: {e}");
        }
    }

    fn drop_conn(&mut self, addr: SocketAddr, reason: DisconnectReason) {
        if let Some(c) = self.conns.remove(&addr) {
            self.by_id.remove(&c.id);
            self.lock().clients.remove(&c.id);
            self.emit(ServerEvent::Disconnected { id: c.id, reason });
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Registry> {
        self.registry.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn send(&self, p: &[u8], to: SocketAddr) {
        if let Err(e) = self.socket.send_to(p, to) {
            log::debug!("udp send to {to} failed: {e}");
        }
    }

    fn on_packet(&mut self, now: Instant, buf: &[u8], from: SocketAddr) {
        match buf.first().copied() {
            Some(proto::CONNECT) => self.on_connect(now, buf, from),
            Some(proto::RESPONSE) => self.on_response(now, buf, from),
            Some(proto::DISCONNECT) => {
                let session = proto::parse_tagged(buf);
                if self.conns.get(&from).map(|c| c.link.session) == session && session.is_some() {
                    self.drop_conn(from, DisconnectReason::Remote);
                }
            }
            Some(proto::ACK..=proto::RELIABLE) => self.on_data(now, buf, from),
            _ => {}
        }
    }

    fn on_connect(&mut self, now: Instant, buf: &[u8], from: SocketAddr) {
        let Some(client_salt) = proto::parse_connect(buf) else {
            return;
        };
        if self.pending.len() >= MAX_PENDING && !self.pending.contains_key(&from) {
            return;
        }
        let p = self.pending.entry(from).or_insert_with(|| Pending {
            client_salt,
            server_salt: random_u64(),
            at: now,
        });
        if p.client_salt != client_salt {
            *p = Pending {
                client_salt,
                server_salt: random_u64(),
                at: now,
            };
        }
        let reply = proto::challenge(p.client_salt, p.server_salt);
        self.send(&reply, from);
    }

    fn on_response(&mut self, now: Instant, buf: &[u8], from: SocketAddr) {
        let Some(session) = proto::parse_tagged(buf) else {
            return;
        };
        if let Some(c) = self.conns.get(&from)
            && c.link.session == session
        {
            self.send(&proto::tagged(proto::ACCEPT, session), from);
            return;
        }
        let Some(p) = self.pending.get(&from) else {
            return;
        };
        if p.client_salt ^ p.server_salt != session {
            return;
        }
        let client_salt = p.client_salt;
        self.pending.remove(&from);
        if self.conns.contains_key(&from) {
            self.drop_conn(from, DisconnectReason::Remote);
        }
        if self.conns.len() >= self.cfg.max_clients {
            self.send(&proto::tagged(proto::REJECT, client_salt), from);
            return;
        }
        let id = ClientId(self.next_id);
        self.next_id += 1;
        let link = Link::new(session, now, self.cfg.link());
        self.conns.insert(from, Conn { id, link });
        self.by_id.insert(id, from);
        self.lock().clients.insert(id, (from, Stats::default()));
        self.send(&proto::tagged(proto::ACCEPT, session), from);
        self.emit(ServerEvent::Connected { id, addr: from });
    }

    fn on_data(&mut self, now: Instant, buf: &[u8], from: SocketAddr) {
        let Some(c) = self.conns.get_mut(&from) else {
            return;
        };
        let id = c.id;
        let mut delivered = Vec::new();
        if let Err(e) = c.link.handle(now, buf, &mut delivered) {
            let reason = DisconnectReason::Protocol(e.to_string());
            let bye = proto::tagged(proto::DISCONNECT, c.link.session);
            self.send(&bye, from);
            self.drop_conn(from, reason);
            return;
        }
        for payload in delivered {
            match decode_payload::<In>(&payload) {
                Ok(message) => self.emit(ServerEvent::Message { id, message }),
                Err(e) => log::debug!("udp message from {id} undecodable: {e}"),
            }
        }
    }

    fn tick(&mut self, now: Instant) {
        let mut out = Vec::new();
        let mut dead = Vec::new();
        for (addr, c) in &mut self.conns {
            if c.link.timed_out(now) {
                dead.push(*addr);
                continue;
            }
            c.link.poll_transmit(now, &mut out);
            for p in out.drain(..) {
                if let Err(e) = self.socket.send_to(&p, *addr) {
                    log::debug!("udp send to {addr} failed: {e}");
                }
            }
        }
        for addr in dead {
            self.drop_conn(addr, DisconnectReason::Timeout);
        }
        self.pending
            .retain(|_, p| now.duration_since(p.at) < PENDING_TTL);
        if now.duration_since(self.last_stats) >= Duration::from_millis(100) {
            self.last_stats = now;
            let snapshot: Vec<_> = self
                .conns
                .values()
                .map(|c| (c.id, c.link.stats()))
                .collect();
            let mut reg = self.lock();
            for (id, st) in snapshot {
                if let Some(entry) = reg.clients.get_mut(&id) {
                    entry.1 = st;
                }
            }
        }
    }
}

pub(crate) fn is_idle(e: &io::Error) -> bool {
    matches!(
        e.kind(),
        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
    )
}
