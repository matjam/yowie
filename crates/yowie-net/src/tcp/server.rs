//! The TCP server.

use std::collections::HashMap;
use std::io::ErrorKind;
use std::marker::PhantomData;
use std::net::{SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};

use crossbeam_channel::{Receiver, Sender, TrySendError, bounded};
use serde::Serialize;
use serde::de::DeserializeOwned;

use super::stream::Stream;
use super::{EventTx, Frame, TcpConfig, io};
use crate::error::{ClientId, DisconnectReason, Error, Result, WakeSlot, WakerFn};
use crate::frame::{decode_payload, encode_frame};
use crate::tls::TlsServer;

/// Something that happened on a [`TcpServer`], returned by `poll()`.
#[derive(Debug)]
pub enum ServerEvent<T> {
    /// A client finished connecting (and the TLS handshake, if any).
    Connected {
        /// The new client's id.
        id: ClientId,
        /// The client's remote address.
        addr: SocketAddr,
    },
    /// A client's connection ended.
    Disconnected {
        /// The client's id.
        id: ClientId,
        /// Why the connection ended.
        reason: DisconnectReason,
    },
    /// A client sent a message.
    Message {
        /// The sender.
        id: ClientId,
        /// The decoded message.
        message: T,
    },
}

struct Conn {
    tx: Sender<Frame>,
    addr: SocketAddr,
    kick: Arc<AtomicBool>,
}

struct Shared {
    stop: AtomicBool,
    active: AtomicUsize,
    next_id: AtomicU64,
    conns: Mutex<HashMap<ClientId, Conn>>,
    threads: Mutex<Vec<JoinHandle<()>>>,
    cfg: TcpConfig,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// A TCP server. `In` is the message type clients send; `Out` is the type
/// the server sends. Both use postcard encoding with a 4-byte length prefix.
///
/// The server runs an accept thread and one thread per connection. Call
/// [`TcpServer::poll`] each frame to receive events. Dropping the server
/// stops all threads after flushing queued messages.
pub struct TcpServer<In, Out> {
    shared: Arc<Shared>,
    events: Receiver<ServerEvent<In>>,
    waker: WakeSlot,
    addr: SocketAddr,
    accept: Option<JoinHandle<()>>,
    _out: PhantomData<fn(Out)>,
}

impl<In, Out> TcpServer<In, Out>
where
    In: DeserializeOwned + Send + 'static,
    Out: Serialize,
{
    /// Listens for plain TCP connections on `addr` (use port 0 for an
    /// OS-assigned port; see [`TcpServer::local_addr`]).
    pub fn listen(addr: impl ToSocketAddrs, cfg: TcpConfig) -> Result<Self> {
        Self::start(addr, cfg, None)
    }

    /// Listens for TLS connections on `addr`.
    pub fn listen_tls(addr: impl ToSocketAddrs, cfg: TcpConfig, tls: TlsServer) -> Result<Self> {
        Self::start(addr, cfg, Some(tls))
    }

    fn start(addr: impl ToSocketAddrs, cfg: TcpConfig, tls: Option<TlsServer>) -> Result<Self> {
        let listener = TcpListener::bind(addr)?;
        listener.set_nonblocking(true)?;
        let local = listener.local_addr()?;
        let (tx, events) = bounded(cfg.event_queue.max(1));
        let waker = WakeSlot::default();
        let shared = Arc::new(Shared {
            stop: AtomicBool::new(false),
            active: AtomicUsize::new(0),
            next_id: AtomicU64::new(1),
            conns: Mutex::new(HashMap::new()),
            threads: Mutex::new(Vec::new()),
            cfg,
        });
        let etx = EventTx {
            tx,
            waker: waker.clone(),
        };
        let sh = shared.clone();
        let accept = thread::Builder::new()
            .name("yowie-net-accept".into())
            .spawn(move || accept_loop::<In>(listener, sh, etx, tls))?;
        Ok(Self {
            shared,
            events,
            waker,
            addr: local,
            accept: Some(accept),
            _out: PhantomData,
        })
    }

    /// The address the server is bound to.
    pub fn local_addr(&self) -> SocketAddr {
        self.addr
    }

    /// Sets a callback run on a background thread whenever an event becomes
    /// available, so a sleeping event loop can wake and call `poll()`.
    pub fn set_waker(&self, waker: WakerFn) {
        self.waker.set(Some(waker));
    }

    /// Returns the next pending event without blocking.
    pub fn poll(&self) -> Option<ServerEvent<In>> {
        self.events.try_recv().ok()
    }

    /// Ids of the clients currently connected.
    pub fn clients(&self) -> Vec<ClientId> {
        lock(&self.shared.conns).keys().copied().collect()
    }

    /// The remote address of a connected client.
    pub fn client_addr(&self, id: ClientId) -> Option<SocketAddr> {
        lock(&self.shared.conns).get(&id).map(|c| c.addr)
    }

    /// Queues `msg` for one client.
    ///
    /// # Errors
    /// [`Error::UnknownClient`] if the client is gone, [`Error::QueueFull`]
    /// if its send queue is full, or an encoding or size error.
    pub fn send(&self, id: ClientId, msg: &Out) -> Result<()> {
        let frame = Arc::new(encode_frame(msg, self.shared.cfg.max_message)?);
        let conns = lock(&self.shared.conns);
        let conn = conns.get(&id).ok_or(Error::UnknownClient(id))?;
        match conn.tx.try_send(frame) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(_)) => Err(Error::QueueFull),
            Err(TrySendError::Disconnected(_)) => Err(Error::Closed),
        }
    }

    /// Queues `msg` for every client. Returns the ids whose queues were full
    /// or closed; the message was not queued for those.
    pub fn broadcast(&self, msg: &Out) -> Result<Vec<ClientId>> {
        self.broadcast_except(msg, None)
    }

    /// Like [`TcpServer::broadcast`], skipping `except`.
    pub fn broadcast_except(&self, msg: &Out, except: Option<ClientId>) -> Result<Vec<ClientId>> {
        let frame = Arc::new(encode_frame(msg, self.shared.cfg.max_message)?);
        let conns = lock(&self.shared.conns);
        let mut failed = Vec::new();
        for (id, conn) in conns.iter() {
            if Some(*id) != except && conn.tx.try_send(frame.clone()).is_err() {
                failed.push(*id);
            }
        }
        Ok(failed)
    }

    /// Closes a client's connection after its queued messages are written.
    pub fn disconnect(&self, id: ClientId) {
        if let Some(c) = lock(&self.shared.conns).get(&id) {
            c.kick.store(true, Ordering::Release);
        }
    }

    /// Stops all threads and closes every connection. Also done on drop.
    pub fn shutdown(&mut self) {
        self.shared.stop.store(true, Ordering::Release);
        if let Some(h) = self.accept.take() {
            let _ = h.join();
        }
        let handles: Vec<_> = lock(&self.shared.threads).drain(..).collect();
        for h in handles {
            let _ = h.join();
        }
    }
}

impl<In, Out> Drop for TcpServer<In, Out> {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Release);
        if let Some(h) = self.accept.take() {
            let _ = h.join();
        }
        let handles: Vec<_> = lock(&self.shared.threads).drain(..).collect();
        for h in handles {
            let _ = h.join();
        }
    }
}

fn accept_loop<In: DeserializeOwned + Send + 'static>(
    listener: TcpListener,
    shared: Arc<Shared>,
    etx: EventTx<ServerEvent<In>>,
    tls: Option<TlsServer>,
) {
    while !shared.stop.load(Ordering::Acquire) {
        match listener.accept() {
            Ok((sock, addr)) => {
                if shared.active.load(Ordering::Acquire) >= shared.cfg.max_clients {
                    log::debug!("rejecting {addr}: server full");
                    continue;
                }
                shared.active.fetch_add(1, Ordering::AcqRel);
                let sh = shared.clone();
                let tx = etx.clone();
                let tls = tls.clone();
                let spawned =
                    thread::Builder::new()
                        .name("yowie-net-conn".into())
                        .spawn(move || {
                            serve(sock, addr, &sh, tx, tls);
                            sh.active.fetch_sub(1, Ordering::AcqRel);
                        });
                match spawned {
                    Ok(h) => {
                        let mut threads = lock(&shared.threads);
                        threads.retain(|t| !t.is_finished());
                        threads.push(h);
                    }
                    Err(e) => {
                        log::warn!("cannot spawn connection thread: {e}");
                        shared.active.fetch_sub(1, Ordering::AcqRel);
                    }
                }
            }
            Err(e) if e.kind() == ErrorKind::WouldBlock => thread::sleep(shared.cfg.tick),
            Err(e) => {
                log::warn!("accept failed: {e}");
                thread::sleep(shared.cfg.tick);
            }
        }
    }
}

fn serve<In: DeserializeOwned + Send + 'static>(
    sock: TcpStream,
    addr: SocketAddr,
    shared: &Shared,
    etx: EventTx<ServerEvent<In>>,
    tls: Option<TlsServer>,
) {
    let cfg = &shared.cfg;
    let stopped_global = || shared.stop.load(Ordering::Acquire);
    let result = sock
        .set_nonblocking(false)
        .map_err(Error::Io)
        .and_then(|()| {
            Stream::accept(sock, tls.as_ref().map(|t| &t.config), cfg.handshake_timeout)
        });
    let mut stream = match result {
        Ok(s) => s,
        Err(e) => {
            log::debug!("connection from {addr} failed: {e}");
            return;
        }
    };
    if let Err(e) = stream.set_timeouts(cfg.tick, cfg.write_timeout) {
        log::debug!("connection from {addr}: {e}");
        return;
    }
    let id = ClientId(shared.next_id.fetch_add(1, Ordering::Relaxed));
    let (tx, rx) = bounded(cfg.send_queue.max(1));
    let kick = Arc::new(AtomicBool::new(false));
    lock(&shared.conns).insert(
        id,
        Conn {
            tx,
            addr,
            kick: kick.clone(),
        },
    );
    if !etx.push(ServerEvent::Connected { id, addr }, stopped_global) {
        lock(&shared.conns).remove(&id);
        return;
    }
    let reason = io::run(
        &mut stream,
        &rx,
        cfg.max_message,
        || stopped_global() || kick.load(Ordering::Acquire),
        |payload| {
            let message = decode_payload::<In>(&payload)
                .map_err(|e| DisconnectReason::Protocol(e.to_string()))?;
            if etx.push(ServerEvent::Message { id, message }, stopped_global) {
                Ok(())
            } else {
                Err(DisconnectReason::Local)
            }
        },
    );
    stream.close();
    lock(&shared.conns).remove(&id);
    etx.push(ServerEvent::Disconnected { id, reason }, stopped_global);
}
