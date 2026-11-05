//! Link-to-link simulation with a virtual clock, loss, delay and reordering.

use std::time::{Duration, Instant};

use super::Channel;
use super::link::{Link, LinkConfig};

struct Sim {
    now: Instant,
    a: Link,
    b: Link,
    wire: Vec<(Instant, bool, Vec<u8>)>, // (arrival, to_b, datagram)
    rng: u64,
    drop: f32,
    delay: Duration,
    jitter: Duration,
    got_a: Vec<Vec<u8>>,
    got_b: Vec<Vec<u8>>,
}

fn cfg() -> LinkConfig {
    LinkConfig {
        max_message: 1 << 20,
        max_queue: 4096,
        timeout: Duration::from_secs(60),
        keepalive: Duration::from_millis(500),
    }
}

impl Sim {
    fn new(drop: f32, delay_ms: u64, jitter_ms: u64) -> Self {
        let now = Instant::now();
        Self {
            now,
            a: Link::new(7, now, cfg()),
            b: Link::new(7, now, cfg()),
            wire: Vec::new(),
            rng: 0x9e37_79b9_7f4a_7c15,
            drop,
            delay: Duration::from_millis(delay_ms),
            jitter: Duration::from_millis(jitter_ms),
            got_a: Vec::new(),
            got_b: Vec::new(),
        }
    }

    fn rand(&mut self) -> f32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        (self.rng >> 40) as f32 / (1u64 << 24) as f32
    }

    /// Advances one millisecond.
    fn step(&mut self) {
        self.now += Duration::from_millis(1);
        let mut out = Vec::new();
        for to_b in [true, false] {
            let link = if to_b { &mut self.a } else { &mut self.b };
            link.poll_transmit(self.now, &mut out);
            for p in out.drain(..) {
                if self.rand() < self.drop {
                    continue;
                }
                let j = self.jitter.mul_f32(self.rand());
                self.wire.push((self.now + self.delay + j, to_b, p));
            }
        }
        let now = self.now;
        let (due, rest): (Vec<_>, Vec<_>) = self.wire.drain(..).partition(|w| w.0 <= now);
        self.wire = rest;
        for (_, to_b, p) in due {
            let (link, got) = if to_b {
                (&mut self.b, &mut self.got_b)
            } else {
                (&mut self.a, &mut self.got_a)
            };
            link.handle(now, &p, got).expect("valid packet");
        }
    }

    fn run(&mut self, ms: u32) {
        for _ in 0..ms {
            self.step();
        }
    }
}

#[test]
fn reliable_ordered_under_loss_and_reordering() {
    let mut sim = Sim::new(0.3, 10, 25);
    let big: Vec<u8> = (0..20_000u32).map(|i| (i % 251) as u8).collect();
    let mut want = Vec::new();
    for i in 0..60u32 {
        let msg = if i == 20 {
            big.clone()
        } else {
            i.to_le_bytes().to_vec()
        };
        sim.a.send(Channel::Reliable, &msg).expect("queued");
        want.push(msg);
    }
    sim.run(20_000);
    assert_eq!(sim.got_b.len(), want.len(), "stats {:?}", sim.a.stats());
    assert_eq!(sim.got_b, want);
    assert_eq!(sim.a.stats().pending, 0);
}

#[test]
fn unreliable_and_sequenced_semantics() {
    let mut sim = Sim::new(0.0, 5, 0);
    for i in 0..10u8 {
        sim.a.send(Channel::Unreliable, &[i]).expect("queued");
        sim.a.send(Channel::Sequenced, &[100 + i]).expect("queued");
    }
    sim.run(100);
    let unrel: Vec<_> = sim.got_b.iter().filter(|m| m[0] < 100).collect();
    assert_eq!(unrel.len(), 10);
    let seq: Vec<_> = sim.got_b.iter().filter(|m| m[0] >= 100).collect();
    assert_eq!(seq.len(), 10);
    assert!(sim.a.send(Channel::Unreliable, &[0u8; 1300]).is_err());
}

#[test]
fn sequenced_drops_stale_messages() {
    let mut sim = Sim::new(0.0, 20, 40);
    for i in 0..40u8 {
        sim.a.send(Channel::Sequenced, &[i]).expect("queued");
        sim.run(1);
    }
    sim.run(200);
    let order: Vec<u8> = sim.got_b.iter().map(|m| m[0]).collect();
    assert!(order.windows(2).all(|w| w[0] < w[1]), "{order:?}");
    assert!(!order.is_empty());
}

#[test]
fn rtt_estimate_converges() {
    let mut sim = Sim::new(0.0, 30, 0);
    for round in 0..40 {
        sim.a.send(Channel::Reliable, &[round]).expect("queued");
        sim.run(100);
    }
    let rtt = sim.a.stats().rtt.as_millis();
    assert!((55..=75).contains(&rtt), "rtt {rtt} ms");
    assert!(sim.a.stats().loss < 0.01);
}

#[test]
fn loss_estimate_tracks_drop_rate() {
    let mut sim = Sim::new(0.2, 5, 0);
    for i in 0..600u32 {
        sim.a.send(Channel::Unreliable, &[1]).expect("queued");
        sim.b.send(Channel::Unreliable, &[2]).expect("queued");
        sim.run(5);
        let _ = i;
    }
    let loss = sim.a.stats().loss;
    assert!((0.08..0.4).contains(&loss), "loss {loss}");
}

#[test]
fn silent_link_times_out_and_keepalive_flows() {
    let mut sim = Sim::new(0.0, 5, 0);
    sim.run(2000);
    assert!(!sim.a.timed_out(sim.now));
    let mut c = cfg();
    c.timeout = Duration::from_millis(100);
    let lonely = Link::new(1, sim.now, c);
    assert!(lonely.timed_out(sim.now + Duration::from_millis(101)));
}
