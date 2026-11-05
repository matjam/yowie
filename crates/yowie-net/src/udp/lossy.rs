//! A socket wrapper that drops, delays and reorders datagrams, for tests
//! and for trying a game under bad network conditions.

use std::io;
use std::net::SocketAddr;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use super::socket::Datagram;

/// What a [`LossySocket`] does to traffic.
#[derive(Debug, Clone)]
pub struct LossConfig {
    /// Probability, 0 to 1, that a datagram is dropped, applied to each
    /// direction separately.
    pub drop: f32,
    /// Fixed delay added to every received datagram.
    pub delay: Duration,
    /// Random extra delay, up to this value, added to each received
    /// datagram. A non-zero jitter reorders datagrams.
    pub jitter: Duration,
    /// Seed for the pseudo-random generator, so runs are repeatable.
    pub seed: u64,
}

impl Default for LossConfig {
    fn default() -> Self {
        Self {
            drop: 0.0,
            delay: Duration::ZERO,
            jitter: Duration::ZERO,
            seed: 1,
        }
    }
}

struct Held {
    release: Instant,
    data: Vec<u8>,
    from: SocketAddr,
}

struct State {
    rng: u64,
    held: Vec<Held>,
}

impl State {
    fn next(&mut self) -> f32 {
        // xorshift64*: adequate statistical quality for simulation.
        self.rng ^= self.rng >> 12;
        self.rng ^= self.rng << 25;
        self.rng ^= self.rng >> 27;
        let v = self.rng.wrapping_mul(0x2545_f491_4f6c_dd1d) >> 40;
        v as f32 / (1u64 << 24) as f32
    }
}

/// Wraps a socket and degrades the traffic through it.
pub struct LossySocket<S> {
    inner: S,
    cfg: LossConfig,
    state: Mutex<State>,
    timeout: Mutex<Duration>,
}

impl<S: Datagram> LossySocket<S> {
    /// Wraps `inner` with the given degradation.
    pub fn new(inner: S, cfg: LossConfig) -> Self {
        let seed = if cfg.seed == 0 { 1 } else { cfg.seed };
        Self {
            inner,
            cfg,
            state: Mutex::new(State {
                rng: seed,
                held: Vec::new(),
            }),
            timeout: Mutex::new(Duration::from_millis(2)),
        }
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn pop_due(&self, now: Instant) -> Option<Held> {
        let mut st = self.state();
        let idx = st
            .held
            .iter()
            .enumerate()
            .filter(|(_, h)| h.release <= now)
            .min_by_key(|(_, h)| h.release)
            .map(|(i, _)| i)?;
        Some(st.held.swap_remove(idx))
    }
}

impl<S: Datagram> Datagram for LossySocket<S> {
    fn send_to(&self, buf: &[u8], addr: SocketAddr) -> io::Result<usize> {
        if self.state().next() < self.cfg.drop {
            return Ok(buf.len());
        }
        self.inner.send_to(buf, addr)
    }

    fn recv_from(&self, buf: &mut [u8]) -> io::Result<(usize, SocketAddr)> {
        let deadline = Instant::now() + *self.timeout.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            if let Some(h) = self.pop_due(Instant::now()) {
                let n = h.data.len().min(buf.len());
                buf[..n].copy_from_slice(&h.data[..n]);
                return Ok((n, h.from));
            }
            match self.inner.recv_from(buf) {
                Ok((n, from)) => {
                    let mut st = self.state();
                    if st.next() < self.cfg.drop {
                        continue;
                    }
                    let jitter = self.cfg.jitter.mul_f32(st.next());
                    st.held.push(Held {
                        release: Instant::now() + self.cfg.delay + jitter,
                        data: buf[..n].to_vec(),
                        from,
                    });
                }
                Err(e)
                    if matches!(
                        e.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                    ) =>
                {
                    if Instant::now() >= deadline {
                        return Err(io::ErrorKind::TimedOut.into());
                    }
                }
                Err(e) => return Err(e),
            }
        }
    }

    fn set_timeout(&self, timeout: Duration) -> io::Result<()> {
        *self.timeout.lock().unwrap_or_else(|e| e.into_inner()) = timeout;
        self.inner.set_timeout(timeout)
    }

    fn local_addr(&self) -> io::Result<SocketAddr> {
        self.inner.local_addr()
    }
}
