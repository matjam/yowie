//! UDP wire format.
//!
//! Handshake packets start with a kind byte and carry the salts exchanged
//! to prove the client owns its address. Data packets carry a 17-byte
//! header: kind, session (u64), packet sequence (u16), newest received
//! sequence (u16) and a bitfield of the 32 sequences before it (u32), all
//! little-endian. Reliable packets add a message number (u16) and a flags
//! byte; sequenced packets add a message sequence (u16).

pub(crate) const CONNECT: u8 = 1;
pub(crate) const CHALLENGE: u8 = 2;
pub(crate) const RESPONSE: u8 = 3;
pub(crate) const ACCEPT: u8 = 4;
pub(crate) const REJECT: u8 = 5;
pub(crate) const DISCONNECT: u8 = 6;
pub(crate) const ACK: u8 = 0x10;
pub(crate) const UNRELIABLE: u8 = 0x11;
pub(crate) const SEQUENCED: u8 = 0x12;
pub(crate) const RELIABLE: u8 = 0x13;

/// Largest datagram this crate sends, in bytes. Chosen to fit common
/// Ethernet, VPN and tunnel paths without IP fragmentation.
pub const MTU: usize = 1200;
pub(crate) const HEADER: usize = 17;
pub(crate) const REL_HEADER: usize = 3;
pub(crate) const MAX_FRAGMENT: usize = MTU - HEADER - REL_HEADER;
pub(crate) const MAX_UNRELIABLE: usize = MTU - HEADER;
pub(crate) const MAX_SEQUENCED: usize = MAX_UNRELIABLE - 2;
/// Flag bit in a reliable packet: more fragments of this message follow.
pub(crate) const FLAG_MORE: u8 = 1;
const MAGIC: u32 = 0x594f_5749;
/// The size a CONNECT is padded to, so a spoofed one cannot amplify.
const CONNECT_SIZE: usize = 128;

pub(crate) struct Header {
    pub(crate) kind: u8,
    pub(crate) session: u64,
    pub(crate) seq: u16,
    pub(crate) ack: u16,
    pub(crate) bits: u32,
}

impl Header {
    pub(crate) fn write(&self, out: &mut Vec<u8>) {
        out.push(self.kind);
        out.extend_from_slice(&self.session.to_le_bytes());
        out.extend_from_slice(&self.seq.to_le_bytes());
        out.extend_from_slice(&self.ack.to_le_bytes());
        out.extend_from_slice(&self.bits.to_le_bytes());
    }

    pub(crate) fn read(b: &[u8]) -> Option<Header> {
        if b.len() < HEADER {
            return None;
        }
        Some(Header {
            kind: b[0],
            session: u64::from_le_bytes(b[1..9].try_into().ok()?),
            seq: u16::from_le_bytes(b[9..11].try_into().ok()?),
            ack: u16::from_le_bytes(b[11..13].try_into().ok()?),
            bits: u32::from_le_bytes(b[13..17].try_into().ok()?),
        })
    }
}

pub(crate) fn connect(client_salt: u64) -> Vec<u8> {
    let mut p = vec![0u8; CONNECT_SIZE];
    p[0] = CONNECT;
    p[1..5].copy_from_slice(&MAGIC.to_le_bytes());
    p[5..13].copy_from_slice(&client_salt.to_le_bytes());
    p
}

/// Returns the client salt of a CONNECT packet.
pub(crate) fn parse_connect(b: &[u8]) -> Option<u64> {
    if b.len() < CONNECT_SIZE || b[1..5] != MAGIC.to_le_bytes() {
        return None;
    }
    Some(u64::from_le_bytes(b[5..13].try_into().ok()?))
}

pub(crate) fn challenge(client_salt: u64, server_salt: u64) -> Vec<u8> {
    let mut p = vec![CHALLENGE];
    p.extend_from_slice(&client_salt.to_le_bytes());
    p.extend_from_slice(&server_salt.to_le_bytes());
    p
}

pub(crate) fn parse_challenge(b: &[u8]) -> Option<(u64, u64)> {
    if b.len() < 17 {
        return None;
    }
    Some((
        u64::from_le_bytes(b[1..9].try_into().ok()?),
        u64::from_le_bytes(b[9..17].try_into().ok()?),
    ))
}

/// A one-field packet: RESPONSE, ACCEPT, REJECT (client salt) or
/// DISCONNECT (session).
pub(crate) fn tagged(kind: u8, value: u64) -> Vec<u8> {
    let mut p = vec![kind];
    p.extend_from_slice(&value.to_le_bytes());
    p
}

pub(crate) fn parse_tagged(b: &[u8]) -> Option<u64> {
    if b.len() < 9 {
        return None;
    }
    Some(u64::from_le_bytes(b[1..9].try_into().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_round_trip() {
        let h = Header {
            kind: RELIABLE,
            session: 0xdead_beef_1234,
            seq: 65535,
            ack: 7,
            bits: 0x8000_0001,
        };
        let mut b = Vec::new();
        h.write(&mut b);
        assert_eq!(b.len(), HEADER);
        let r = Header::read(&b).expect("header parses");
        assert_eq!(
            (r.kind, r.session, r.seq, r.ack, r.bits),
            (h.kind, h.session, h.seq, h.ack, h.bits)
        );
        assert!(Header::read(&b[..HEADER - 1]).is_none());
    }

    #[test]
    fn handshake_packets_parse() {
        assert_eq!(parse_connect(&connect(99)), Some(99));
        assert_eq!(parse_connect(&[CONNECT; 20]), None);
        assert_eq!(parse_challenge(&challenge(1, 2)), Some((1, 2)));
        assert_eq!(parse_tagged(&tagged(ACCEPT, 5)), Some(5));
    }
}
