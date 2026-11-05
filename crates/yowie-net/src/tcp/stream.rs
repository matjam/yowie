//! One connection's byte stream, plain or wrapped in TLS.

use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpStream};
use std::sync::Arc;
use std::time::Duration;

use rustls::pki_types::ServerName;
use rustls::{ClientConfig, ClientConnection, ServerConfig, ServerConnection, StreamOwned};

use crate::error::{Error, Result};

pub(crate) enum Stream {
    Plain(TcpStream),
    Server(Box<StreamOwned<ServerConnection, TcpStream>>),
    Client(Box<StreamOwned<ClientConnection, TcpStream>>),
}

fn handshake<T: rustls::SideData>(
    conn: &mut rustls::ConnectionCommon<T>,
    sock: &mut TcpStream,
) -> io::Result<()> {
    while conn.is_handshaking() {
        conn.complete_io(sock)?;
    }
    Ok(())
}

impl Stream {
    /// Wraps an accepted socket, completing the TLS handshake when
    /// `tls` is set. `limit` bounds the handshake duration.
    pub(crate) fn accept(
        mut sock: TcpStream,
        tls: Option<&Arc<ServerConfig>>,
        limit: Duration,
    ) -> Result<Self> {
        sock.set_nodelay(true)?;
        let Some(cfg) = tls else {
            return Ok(Self::Plain(sock));
        };
        sock.set_read_timeout(Some(limit))?;
        sock.set_write_timeout(Some(limit))?;
        let mut conn = ServerConnection::new(cfg.clone())?;
        handshake(&mut *conn, &mut sock).map_err(Error::Io)?;
        Ok(Self::Server(Box::new(StreamOwned::new(conn, sock))))
    }

    /// Wraps a connected client socket.
    pub(crate) fn connect(
        mut sock: TcpStream,
        tls: Option<(&Arc<ClientConfig>, ServerName<'static>)>,
        limit: Duration,
    ) -> Result<Self> {
        sock.set_nodelay(true)?;
        let Some((cfg, name)) = tls else {
            return Ok(Self::Plain(sock));
        };
        sock.set_read_timeout(Some(limit))?;
        sock.set_write_timeout(Some(limit))?;
        let mut conn = ClientConnection::new(cfg.clone(), name)?;
        handshake(&mut *conn, &mut sock).map_err(Error::Io)?;
        Ok(Self::Client(Box::new(StreamOwned::new(conn, sock))))
    }

    fn socket(&self) -> &TcpStream {
        match self {
            Self::Plain(s) => s,
            Self::Server(s) => &s.sock,
            Self::Client(s) => &s.sock,
        }
    }

    /// Sets the idle read timeout (the loop tick) and the write timeout.
    pub(crate) fn set_timeouts(&self, read: Duration, write: Duration) -> io::Result<()> {
        self.socket().set_read_timeout(Some(read))?;
        self.socket().set_write_timeout(Some(write))
    }

    /// Sends the TLS close notification if any, then shuts the socket down.
    pub(crate) fn close(&mut self) {
        match self {
            Self::Plain(_) => {}
            Self::Server(s) => {
                s.conn.send_close_notify();
                let _ = s.conn.complete_io(&mut s.sock);
            }
            Self::Client(s) => {
                s.conn.send_close_notify();
                let _ = s.conn.complete_io(&mut s.sock);
            }
        }
        let _ = self.socket().shutdown(Shutdown::Both);
    }
}

impl Read for Stream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            Self::Plain(s) => s.read(buf),
            Self::Server(s) => s.read(buf),
            Self::Client(s) => s.read(buf),
        }
    }
}

impl Write for Stream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            Self::Plain(s) => s.write(buf),
            Self::Server(s) => s.write(buf),
            Self::Client(s) => s.write(buf),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            Self::Plain(s) => s.flush(),
            Self::Server(s) => s.flush(),
            Self::Client(s) => s.flush(),
        }
    }
}
