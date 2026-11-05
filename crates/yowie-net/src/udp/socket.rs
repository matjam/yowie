//! The datagram socket abstraction.

use std::io;
use std::net::{SocketAddr, UdpSocket};
use std::time::Duration;

/// A datagram socket the UDP endpoints can run on. Implemented for
/// [`std::net::UdpSocket`] and by [`crate::udp::LossySocket`], which wraps
/// another socket to simulate loss, delay and reordering.
pub trait Datagram: Send + 'static {
    /// Sends one datagram.
    fn send_to(&self, buf: &[u8], addr: SocketAddr) -> io::Result<usize>;
    /// Receives one datagram, waiting at most the timeout set by
    /// [`Datagram::set_timeout`]. Returns `WouldBlock` or `TimedOut` when
    /// nothing arrived.
    fn recv_from(&self, buf: &mut [u8]) -> io::Result<(usize, SocketAddr)>;
    /// Sets how long `recv_from` waits.
    fn set_timeout(&self, timeout: Duration) -> io::Result<()>;
    /// The local address.
    fn local_addr(&self) -> io::Result<SocketAddr>;
}

impl Datagram for UdpSocket {
    fn send_to(&self, buf: &[u8], addr: SocketAddr) -> io::Result<usize> {
        UdpSocket::send_to(self, buf, addr)
    }

    fn recv_from(&self, buf: &mut [u8]) -> io::Result<(usize, SocketAddr)> {
        UdpSocket::recv_from(self, buf)
    }

    fn set_timeout(&self, timeout: Duration) -> io::Result<()> {
        self.set_read_timeout(Some(timeout.max(Duration::from_millis(1))))
    }

    fn local_addr(&self) -> io::Result<SocketAddr> {
        UdpSocket::local_addr(self)
    }
}
