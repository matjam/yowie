//! Clock synchronisation from ping round trips.

/// Estimates the server's clock from ping replies, so a client can
/// timestamp what it sends and interpolate on the server's timeline.
///
/// For each ping, call [`Clock::sample`] with the local time the ping was
/// sent, the server time carried in the reply, and the local time the reply
/// arrived. The offset assumes the reply took half the round trip.
#[derive(Debug, Clone, Default)]
pub struct Clock {
    offset: f64,
    rtt: f64,
    samples: u32,
}

impl Clock {
    /// Creates a clock with no samples.
    pub fn new() -> Self {
        Self::default()
    }

    /// Records one ping round trip. Times are seconds. Offsets are smoothed
    /// across samples; a round trip quicker than the average counts for
    /// more, because less of it is queueing delay.
    pub fn sample(&mut self, sent: f64, server_time: f64, received: f64) {
        let rtt = (received - sent).max(0.0);
        let offset = server_time - (sent + rtt / 2.0);
        self.samples = self.samples.saturating_add(1);
        if self.samples == 1 {
            self.offset = offset;
            self.rtt = rtt;
            return;
        }
        let weight = if rtt < self.rtt { 0.5 } else { 0.1 };
        self.offset += (offset - self.offset) * weight;
        self.rtt += (rtt - self.rtt) * 0.2;
    }

    /// Converts a local time to the server's.
    pub fn server_time(&self, local: f64) -> f64 {
        local + self.offset
    }

    /// Converts a server time to the local timeline.
    pub fn local_time(&self, server: f64) -> f64 {
        server - self.offset
    }

    /// Smoothed round-trip time in seconds.
    pub fn rtt(&self) -> f64 {
        self.rtt
    }

    /// Server time minus local time, in seconds.
    pub fn offset(&self) -> f64 {
        self.offset
    }

    /// True once at least one sample has been recorded.
    pub fn ready(&self) -> bool {
        self.samples > 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converges_on_offset_and_rtt_despite_jitter() {
        let true_offset = 12.5;
        let mut c = Clock::new();
        assert!(!c.ready());
        for i in 0..40 {
            let sent = f64::from(i) * 0.5;
            // Symmetric 20 ms legs plus alternating queueing delay on the reply.
            let extra = if i % 3 == 0 { 0.03 } else { 0.0 };
            let received = sent + 0.04 + extra;
            let server_time = sent + 0.02 + true_offset;
            c.sample(sent, server_time, received);
        }
        assert!(c.ready());
        assert!(
            (c.offset() - true_offset).abs() < 0.02,
            "offset {}",
            c.offset()
        );
        assert!((c.rtt() - 0.05).abs() < 0.02, "rtt {}", c.rtt());
        assert!((c.local_time(c.server_time(3.0)) - 3.0).abs() < 1e-9);
    }
}
