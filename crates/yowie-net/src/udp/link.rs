//! The state machine for one established UDP connection: channels,
//! reliable resends, fragmentation, acknowledgements and keepalives.
//!
//! A `Link` performs no I/O. Callers pass the current time, feed it
//! incoming datagrams and collect outgoing ones, which keeps it testable
//! with simulated time and loss.

use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

use super::ack::AckTracker;
use super::proto::{self, Header};
use super::seq::seq_gt;
use super::{Channel, Stats};
use crate::error::{Error, Result};

/// Reliable messages that may be in flight at once.
const WINDOW: usize = 128;
/// Datagrams one `poll_transmit` call may emit for reliable data.
const MAX_BURST: usize = 32;
/// Unreliable messages that may wait to be sent.
const MAX_UNRELIABLE_QUEUE: usize = 256;
/// Delay before acknowledging packets that need no prompt acknowledgement.
const LAZY_ACK: Duration = Duration::from_millis(100);

#[derive(Debug, Clone)]
pub(crate) struct LinkConfig {
    pub(crate) max_message: usize,
    pub(crate) max_queue: usize,
    pub(crate) timeout: Duration,
    pub(crate) keepalive: Duration,
}

struct RelUnit {
    data: Vec<u8>,
    more: bool,
    acked: bool,
    last_sent: Option<Instant>,
    attempts: u32,
}

pub(crate) struct Link {
    pub(crate) session: u64,
    cfg: LinkConfig,
    acks: AckTracker,
    last_recv: Instant,
    last_send: Instant,
    ack_due: Option<Instant>,
    unacked_rx: u32,
    unrel_out: VecDeque<(bool, Vec<u8>)>,
    seq_out: u16,
    seq_in: Option<u16>,
    rel_out: VecDeque<RelUnit>,
    rel_base: u16,
    rel_in_next: u16,
    rel_in: HashMap<u16, (bool, Vec<u8>)>,
    partial: Vec<u8>,
    scratch: Vec<u16>,
}

impl Link {
    pub(crate) fn new(session: u64, now: Instant, cfg: LinkConfig) -> Self {
        Self {
            session,
            cfg,
            acks: AckTracker::new(),
            last_recv: now,
            last_send: now,
            ack_due: None,
            unacked_rx: 0,
            unrel_out: VecDeque::new(),
            seq_out: 0,
            seq_in: None,
            rel_out: VecDeque::new(),
            rel_base: 0,
            rel_in_next: 0,
            rel_in: HashMap::new(),
            partial: Vec::new(),
            scratch: Vec::new(),
        }
    }

    /// Queues a message for the next `poll_transmit`.
    pub(crate) fn send(&mut self, channel: Channel, data: &[u8]) -> Result<()> {
        let (limit, unreliable) = match channel {
            Channel::Unreliable => (proto::MAX_UNRELIABLE, true),
            Channel::Sequenced => (proto::MAX_SEQUENCED, true),
            Channel::Reliable => (self.cfg.max_message, false),
        };
        if data.len() > limit {
            return Err(Error::TooLarge {
                size: data.len(),
                max: limit,
            });
        }
        if unreliable {
            if self.unrel_out.len() >= MAX_UNRELIABLE_QUEUE {
                return Err(Error::QueueFull);
            }
            let sequenced = channel == Channel::Sequenced;
            self.unrel_out.push_back((sequenced, data.to_vec()));
            return Ok(());
        }
        let parts = data.len().div_ceil(proto::MAX_FRAGMENT).max(1);
        if self.rel_out.len() + parts > self.cfg.max_queue {
            return Err(Error::QueueFull);
        }
        let mut chunks = data.chunks(proto::MAX_FRAGMENT);
        for i in 0..parts {
            self.rel_out.push_back(RelUnit {
                data: chunks.next().unwrap_or(&[]).to_vec(),
                more: i + 1 < parts,
                acked: false,
                last_sent: None,
                attempts: 0,
            });
        }
        Ok(())
    }

    /// Processes one datagram for this link, appending delivered messages
    /// to `delivered`. An error means the peer violated the protocol.
    pub(crate) fn handle(
        &mut self,
        now: Instant,
        buf: &[u8],
        delivered: &mut Vec<Vec<u8>>,
    ) -> Result<()> {
        let Some(h) = Header::read(buf) else {
            return Ok(());
        };
        if h.session != self.session {
            return Ok(());
        }
        self.last_recv = now;
        let mut rels = std::mem::take(&mut self.scratch);
        rels.clear();
        self.acks.process_acks(now, h.ack, h.bits, &mut rels);
        for &r in &rels {
            self.mark_rel_acked(r);
        }
        self.scratch = rels;
        if !self.acks.register_recv(h.seq) {
            return Ok(());
        }
        let body = &buf[proto::HEADER..];
        match h.kind {
            proto::ACK => {}
            proto::UNRELIABLE => {
                self.lazy_ack(now);
                delivered.push(body.to_vec());
            }
            proto::SEQUENCED => {
                self.lazy_ack(now);
                if body.len() < 2 {
                    return Ok(());
                }
                let s = u16::from_le_bytes([body[0], body[1]]);
                if self.seq_in.is_none_or(|n| seq_gt(s, n)) {
                    self.seq_in = Some(s);
                    delivered.push(body[2..].to_vec());
                }
            }
            proto::RELIABLE => {
                self.ack_due = Some(now);
                if body.len() < proto::REL_HEADER - 1 {
                    return Ok(());
                }
                let n = u16::from_le_bytes([body[0], body[1]]);
                let more = body[2] & proto::FLAG_MORE != 0;
                self.receive_reliable(n, more, &body[3..], delivered)?;
            }
            _ => {}
        }
        Ok(())
    }

    fn lazy_ack(&mut self, now: Instant) {
        self.unacked_rx += 1;
        let due = if self.unacked_rx >= 8 {
            now
        } else {
            now + LAZY_ACK
        };
        self.ack_due = Some(self.ack_due.map_or(due, |d| d.min(due)));
    }

    fn mark_rel_acked(&mut self, n: u16) {
        let idx = usize::from(n.wrapping_sub(self.rel_base));
        if let Some(u) = self.rel_out.get_mut(idx) {
            u.acked = true;
        }
        while self.rel_out.front().is_some_and(|u| u.acked) {
            self.rel_out.pop_front();
            self.rel_base = self.rel_base.wrapping_add(1);
        }
    }

    fn receive_reliable(
        &mut self,
        n: u16,
        more: bool,
        data: &[u8],
        delivered: &mut Vec<Vec<u8>>,
    ) -> Result<()> {
        let ahead = usize::from(n.wrapping_sub(self.rel_in_next));
        if ahead >= WINDOW {
            return Ok(()); // already delivered, or beyond the window
        }
        self.rel_in
            .entry(n)
            .or_insert_with(|| (more, data.to_vec()));
        while let Some((more, chunk)) = self.rel_in.remove(&self.rel_in_next) {
            self.rel_in_next = self.rel_in_next.wrapping_add(1);
            if self.partial.len() + chunk.len() > self.cfg.max_message {
                return Err(Error::TooLarge {
                    size: self.partial.len() + chunk.len(),
                    max: self.cfg.max_message,
                });
            }
            self.partial.extend_from_slice(&chunk);
            if !more {
                delivered.push(std::mem::take(&mut self.partial));
            }
        }
        Ok(())
    }

    fn build(
        &mut self,
        now: Instant,
        kind: u8,
        rel: Option<u16>,
        extra: &[u8],
        body: &[u8],
    ) -> Vec<u8> {
        let seq = self.acks.register_sent(now, rel);
        let (ack, bits) = self.acks.ack_fields();
        let mut out = Vec::with_capacity(proto::HEADER + extra.len() + body.len());
        Header {
            kind,
            session: self.session,
            seq,
            ack,
            bits,
        }
        .write(&mut out);
        out.extend_from_slice(extra);
        out.extend_from_slice(body);
        self.last_send = now;
        self.ack_due = None;
        self.unacked_rx = 0;
        out
    }

    /// Appends every datagram that should be sent now.
    pub(crate) fn poll_transmit(&mut self, now: Instant, out: &mut Vec<Vec<u8>>) {
        let start = out.len();
        while let Some((sequenced, data)) = self.unrel_out.pop_front() {
            if sequenced {
                let s = self.seq_out.to_le_bytes();
                self.seq_out = self.seq_out.wrapping_add(1);
                out.push(self.build(now, proto::SEQUENCED, None, &s, &data));
            } else {
                out.push(self.build(now, proto::UNRELIABLE, None, &[], &data));
            }
        }
        let rto = self.acks.rto();
        let mut burst = 0;
        for i in 0..self.rel_out.len().min(WINDOW) {
            if burst >= MAX_BURST {
                break;
            }
            let u = &self.rel_out[i];
            let due = match u.last_sent {
                None => true,
                Some(t) => {
                    let wait = rto * (1 << u.attempts.min(3));
                    !u.acked && now.saturating_duration_since(t) >= wait.min(Duration::from_secs(2))
                }
            };
            if u.acked || !due {
                continue;
            }
            let n = self.rel_base.wrapping_add(i as u16);
            let extra = [n.to_le_bytes()[0], n.to_le_bytes()[1], u8::from(u.more)];
            let body = u.data.clone();
            out.push(self.build(now, proto::RELIABLE, Some(n), &extra, &body));
            let u = &mut self.rel_out[i];
            u.attempts = if u.last_sent.is_some() {
                u.attempts + 1
            } else {
                0
            };
            u.last_sent = Some(now);
            burst += 1;
        }
        let nothing_sent = out.len() == start;
        let ack_now = self.ack_due.is_some_and(|d| d <= now);
        let keepalive = now.saturating_duration_since(self.last_send) >= self.cfg.keepalive;
        if nothing_sent && (ack_now || keepalive) {
            out.push(self.build(now, proto::ACK, None, &[], &[]));
        }
    }

    pub(crate) fn timed_out(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.last_recv) >= self.cfg.timeout
    }

    pub(crate) fn stats(&self) -> Stats {
        Stats {
            rtt: self.acks.rtt().unwrap_or(Duration::ZERO),
            loss: self.acks.loss(),
            pending: self.rel_out.iter().filter(|u| !u.acked).count(),
        }
    }
}
