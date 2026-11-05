//! Per-client snapshot delta streams with acknowledged baselines.

use std::collections::{HashMap, VecDeque};
use std::hash::Hash;

use serde::Serialize;
use serde::de::DeserializeOwned;

use super::delta::{apply_delta, diff_bytes};
use crate::error::{Error, Result};

/// Snapshots remembered per client or receiver.
const KEEP: usize = 32;

#[derive(Default)]
struct ClientState {
    sent: VecDeque<(u32, Vec<u8>)>,
    acked: Option<(u32, Vec<u8>)>,
}

/// Server side: encodes each client's snapshots as deltas against the last
/// snapshot that client acknowledged, or in full until it acknowledges one.
///
/// Sequence numbers are chosen by the caller, must be non-zero (0 means "no
/// baseline") and should increase. Send the returned baseline sequence,
/// the snapshot sequence and the bytes to the client; when the client
/// reports a snapshot sequence back, call [`SnapshotBuffer::ack`]. Send
/// snapshots on an unreliable channel: a lost delta costs nothing because
/// the next one is still relative to an acknowledged baseline.
pub struct SnapshotBuffer<K, S> {
    clients: HashMap<K, ClientState>,
    _s: std::marker::PhantomData<fn(&S)>,
}

impl<K: Hash + Eq, S: Serialize> Default for SnapshotBuffer<K, S> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K: Hash + Eq, S: Serialize> SnapshotBuffer<K, S> {
    /// Creates an empty buffer.
    pub fn new() -> Self {
        Self {
            clients: HashMap::new(),
            _s: std::marker::PhantomData,
        }
    }

    /// Encodes `snap` (sequence `seq`) for `client`. Returns the baseline
    /// sequence the delta is relative to (0 for none) and the delta bytes.
    pub fn encode(&mut self, client: K, seq: u32, snap: &S) -> Result<(u32, Vec<u8>)> {
        if seq == 0 {
            return Err(Error::BadDelta("sequence 0 is reserved"));
        }
        let bytes = postcard::to_stdvec(snap)?;
        let st = self.clients.entry(client).or_default();
        let (base, delta) = match &st.acked {
            Some((s, b)) => (*s, diff_bytes(b, &bytes)),
            None => (0, diff_bytes(&[], &bytes)),
        };
        st.sent.push_back((seq, bytes));
        while st.sent.len() > KEEP {
            st.sent.pop_front();
        }
        Ok((base, delta))
    }

    /// Records that `client` received snapshot `seq`; later deltas are
    /// relative to it. Unknown or older sequences are ignored.
    pub fn ack(&mut self, client: &K, seq: u32) {
        let Some(st) = self.clients.get_mut(client) else {
            return;
        };
        if st.acked.as_ref().is_some_and(|(s, _)| *s >= seq) {
            return;
        }
        if let Some(i) = st.sent.iter().position(|(s, _)| *s == seq) {
            st.sent.drain(..i);
            st.acked = st.sent.pop_front();
        }
    }

    /// Forgets a client (for example when it disconnects).
    pub fn forget(&mut self, client: &K) {
        self.clients.remove(client);
    }
}

/// Client side: rebuilds snapshots from deltas.
pub struct SnapshotReceiver<S> {
    recent: VecDeque<(u32, Vec<u8>)>,
    _s: std::marker::PhantomData<fn() -> S>,
}

impl<S: DeserializeOwned> Default for SnapshotReceiver<S> {
    fn default() -> Self {
        Self::new()
    }
}

impl<S: DeserializeOwned> SnapshotReceiver<S> {
    /// Creates a receiver with no snapshots.
    pub fn new() -> Self {
        Self {
            recent: VecDeque::new(),
            _s: std::marker::PhantomData,
        }
    }

    /// Decodes snapshot `seq`, sent as a delta against snapshot `base`
    /// (0 for a full snapshot). Report `seq` back to the server on success.
    ///
    /// # Errors
    /// [`Error::MissingBaseline`] if `base` is no longer stored (the server
    /// will keep sending relative to an older acknowledgement; discard this
    /// one), or [`Error::BadDelta`] / [`Error::Codec`] for corrupt data.
    pub fn decode(&mut self, base: u32, seq: u32, delta: &[u8]) -> Result<S> {
        let baseline: &[u8] = if base == 0 {
            &[]
        } else {
            self.recent
                .iter()
                .find(|(s, _)| *s == base)
                .map(|(_, b)| b.as_slice())
                .ok_or(Error::MissingBaseline(base))?
        };
        let bytes = apply_delta(baseline, delta)?;
        let snap = postcard::from_bytes(&bytes)?;
        self.recent.push_back((seq, bytes));
        while self.recent.len() > KEEP {
            self.recent.pop_front();
        }
        Ok(snap)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    struct State {
        tick: u32,
        pos: [f32; 3],
        hp: [u16; 8],
    }

    #[test]
    fn stream_uses_acknowledged_baselines() {
        let mut server: SnapshotBuffer<u8, State> = SnapshotBuffer::new();
        let mut client: SnapshotReceiver<State> = SnapshotReceiver::new();
        let mut state = State {
            tick: 0,
            pos: [0.0; 3],
            hp: [100; 8],
        };
        let mut sizes = Vec::new();
        for seq in 1..=10u32 {
            state.tick = seq;
            state.pos[0] += 1.0;
            if seq == 6 {
                state.hp[2] = 40;
            }
            let (base, bytes) = server.encode(1, seq, &state).expect("encodes");
            sizes.push((base, bytes.len()));
            // Snapshot 4 is lost on the wire; the rest arrive and are acked.
            if seq == 4 {
                continue;
            }
            let got = client.decode(base, seq, &bytes).expect("decodes");
            assert_eq!(got, state, "seq {seq}");
            server.ack(&1, seq);
        }
        assert_eq!(sizes[0].0, 0, "first snapshot is full");
        assert_eq!(sizes[4].0, 3, "after losing 4, baseline stays at 3");
        assert!(sizes[9].1 < sizes[0].1, "{sizes:?}");
    }

    #[test]
    fn missing_baseline_is_reported() {
        let mut c: SnapshotReceiver<u32> = SnapshotReceiver::new();
        assert!(matches!(
            c.decode(9, 10, &[1, 0, 0]),
            Err(Error::MissingBaseline(9))
        ));
        let mut s: SnapshotBuffer<u8, u32> = SnapshotBuffer::new();
        assert!(s.encode(0, 0, &1).is_err());
    }
}
