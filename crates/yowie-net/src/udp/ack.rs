//! Packet sequence numbers, acknowledgement bitfields, round-trip and loss
//! estimation for one link.

use std::time::{Duration, Instant};

use super::seq::{Rtt, seq_gt};

const RING: usize = 512;
/// Weight of each new packet in the loss estimate.
const LOSS_ALPHA: f32 = 0.05;

#[derive(Clone, Copy)]
struct Slot {
    seq: u16,
    valid: bool,
    acked: bool,
    time: Option<Instant>,
    /// Reliable message number carried by the packet, if any.
    rel: Option<u16>,
}

pub(crate) struct AckTracker {
    next_seq: u16,
    sent: Vec<Slot>,
    newest_acked: Option<u16>,
    loss_next: u16,
    loss: f32,
    rtt: Rtt,
    remote_seq: Option<u16>,
    recv_bits: u32,
}

impl AckTracker {
    pub(crate) fn new() -> Self {
        Self {
            // Sequence 0 stays unused so an ack field of 0 means "nothing yet".
            next_seq: 1,
            sent: vec![
                Slot {
                    seq: 0,
                    valid: false,
                    acked: false,
                    time: None,
                    rel: None,
                };
                RING
            ],
            newest_acked: None,
            loss_next: 1,
            loss: 0.0,
            rtt: Rtt::default(),
            remote_seq: None,
            recv_bits: 0,
        }
    }

    /// Records an outgoing packet and returns its sequence number.
    pub(crate) fn register_sent(&mut self, now: Instant, rel: Option<u16>) -> u16 {
        let seq = self.next_seq;
        self.next_seq = self.next_seq.wrapping_add(1);
        self.sent[seq as usize % RING] = Slot {
            seq,
            valid: true,
            acked: false,
            time: Some(now),
            rel,
        };
        seq
    }

    /// The `(ack, bits)` header fields describing what has been received.
    pub(crate) fn ack_fields(&self) -> (u16, u32) {
        (self.remote_seq.unwrap_or(0), self.recv_bits)
    }

    /// Records an incoming packet sequence. Returns false for a duplicate or
    /// a packet too old to track.
    pub(crate) fn register_recv(&mut self, seq: u16) -> bool {
        let Some(newest) = self.remote_seq else {
            self.remote_seq = Some(seq);
            self.recv_bits = 0;
            return true;
        };
        if seq == newest {
            return false;
        }
        if seq_gt(seq, newest) {
            let d = u32::from(seq.wrapping_sub(newest));
            self.recv_bits = if d > 32 {
                0
            } else {
                self.recv_bits.checked_shl(d).unwrap_or(0) | (1u32 << (d - 1))
            };
            self.remote_seq = Some(seq);
            return true;
        }
        let d = u32::from(newest.wrapping_sub(seq));
        if d > 32 {
            return false;
        }
        let bit = 1u32 << (d - 1);
        if self.recv_bits & bit != 0 {
            return false;
        }
        self.recv_bits |= bit;
        true
    }

    /// Applies the acknowledgement fields of an incoming packet. The reliable
    /// message numbers of newly acknowledged packets are appended to `rels`.
    pub(crate) fn process_acks(&mut self, now: Instant, ack: u16, bits: u32, rels: &mut Vec<u16>) {
        self.mark(ack, Some(now), rels);
        for i in 0..32u16 {
            if bits & (1 << i) != 0 {
                self.mark(ack.wrapping_sub(i + 1), None, rels);
            }
        }
        if self.newest_acked.is_none_or(|n| seq_gt(ack, n)) {
            self.newest_acked = Some(ack);
        }
        self.evaluate_loss();
    }

    fn mark(&mut self, seq: u16, rtt_at: Option<Instant>, rels: &mut Vec<u16>) {
        let slot = &mut self.sent[seq as usize % RING];
        if !slot.valid || slot.seq != seq || slot.acked {
            return;
        }
        slot.acked = true;
        if let (Some(now), Some(sent)) = (rtt_at, slot.time) {
            self.rtt.update(now.saturating_duration_since(sent));
        }
        if let Some(r) = slot.rel {
            rels.push(r);
        }
    }

    /// Packets more than 32 behind the newest acknowledged one can no longer
    /// be acknowledged; count them as delivered or lost.
    fn evaluate_loss(&mut self) {
        let Some(newest) = self.newest_acked else {
            return;
        };
        for _ in 0..RING {
            let dist = newest.wrapping_sub(self.loss_next);
            if !(33..0x8000).contains(&dist) {
                break;
            }
            let slot = self.sent[self.loss_next as usize % RING];
            let delivered = slot.valid && slot.seq == self.loss_next && slot.acked;
            let sample = if delivered { 0.0 } else { 1.0 };
            self.loss += (sample - self.loss) * LOSS_ALPHA;
            self.loss_next = self.loss_next.wrapping_add(1);
        }
    }

    pub(crate) fn rtt(&self) -> Option<Duration> {
        self.rtt.smoothed()
    }

    pub(crate) fn rto(&self) -> Duration {
        self.rtt.rto()
    }

    pub(crate) fn loss(&self) -> f32 {
        self.loss
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn receive_window_detects_duplicates() {
        let mut a = AckTracker::new();
        let steps = [
            (10u16, true),
            (10, false),
            (12, true),
            (11, true),
            (11, false),
            (100, true),
            (12, false),
            (60, false),
        ];
        for (seq, want) in steps {
            assert_eq!(a.register_recv(seq), want, "seq {seq}");
        }
        let (ack, bits) = a.ack_fields();
        assert_eq!(ack, 100);
        assert_eq!(bits, 0);
    }

    #[test]
    fn acks_report_loss_and_rtt() {
        let mut a = AckTracker::new();
        let t0 = Instant::now();
        let mut rels = Vec::new();
        // Send 100 packets, acknowledge all but every fourth.
        let seqs: Vec<u16> = (0..100).map(|_| a.register_sent(t0, None)).collect();
        for &s in &seqs {
            if s % 4 != 0 {
                let mut bits = 0u32;
                for i in 0..32u16 {
                    let prev = s.wrapping_sub(i + 1);
                    if prev >= 1 && prev % 4 != 0 {
                        bits |= 1 << i;
                    }
                }
                a.process_acks(t0 + Duration::from_millis(30), s, bits, &mut rels);
            }
        }
        let rtt = a.rtt().map(|d| d.as_millis()).unwrap_or(0);
        assert_eq!(rtt, 30);
        assert!(a.loss() > 0.05, "loss {}", a.loss());
    }
}
