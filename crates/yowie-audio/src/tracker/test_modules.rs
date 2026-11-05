//! Small modules built byte by byte for the tests. Each holds one 32-frame looping
//! square wave and one note on the first channel's first row.

fn put16(b: &mut [u8], at: usize, v: u16) {
    b[at..at + 2].copy_from_slice(&v.to_le_bytes());
}

fn put32(b: &mut [u8], at: usize, v: u32) {
    b[at..at + 4].copy_from_slice(&v.to_le_bytes());
}

/// One cycle of a square wave as signed bytes.
pub(super) fn square() -> Vec<u8> {
    (0..32)
        .map(|i| if i < 16 { 100u8 } else { 156u8 })
        .collect()
}

/// A 4-channel MOD: C-2 with instrument 1 on channel 0, row 0. `row1` is an optional
/// effect (command, parameter) on channel 0 row 1; `row0` the same for row 0.
pub(super) fn build_mod(row0: Option<(u8, u8)>, row1: Option<(u8, u8)>) -> Vec<u8> {
    let mut b = vec![0u8; 20];
    for i in 0..31 {
        let mut h = vec![0u8; 30];
        if i == 0 {
            h[..6].copy_from_slice(b"square");
            h[22..24].copy_from_slice(&16u16.to_be_bytes());
            h[25] = 64;
            h[28..30].copy_from_slice(&16u16.to_be_bytes());
        }
        b.extend(h);
    }
    b.extend([1, 0]);
    b.extend(vec![0u8; 128]);
    b.extend(b"M.K.");
    let mut pattern = vec![0u8; 64 * 4 * 4];
    pattern[0] = (428 >> 8) as u8;
    pattern[1] = (428 & 0xFF) as u8;
    pattern[2] = 0x10;
    if let Some((c, p)) = row0 {
        pattern[2] |= c;
        pattern[3] = p;
    }
    if let Some((c, p)) = row1 {
        let at = 4 * 4;
        pattern[at + 2] = c;
        pattern[at + 3] = p;
    }
    b.extend(pattern);
    b.extend(square());
    b
}

/// A one-channel S3M with C-4 and instrument 1 on row 0.
pub(super) fn build_s3m() -> Vec<u8> {
    let mut b = vec![0u8; 96];
    b[..6].copy_from_slice(b"square");
    b[28] = 0x1A;
    b[29] = 16;
    put16(&mut b, 32, 2);
    put16(&mut b, 34, 1);
    put16(&mut b, 36, 1);
    put16(&mut b, 40, 0x1320);
    put16(&mut b, 42, 2);
    b[44..48].copy_from_slice(b"SCRM");
    b[48] = 64;
    b[49] = 6;
    b[50] = 125;
    b[51] = 0x80 | 48;
    b[64..96].fill(255);
    b[64] = 0;
    b.extend([0, 0]);
    b.extend([7, 0, 12, 0]);
    b.resize(0x70, 0);
    let mut ins = vec![0u8; 80];
    ins[0] = 1;
    ins[14] = 16;
    put32(&mut ins, 16, 32);
    put32(&mut ins, 24, 32);
    ins[28] = 64;
    ins[31] = 1;
    put32(&mut ins, 32, 8363);
    ins[76..80].copy_from_slice(b"SCRS");
    b.extend(ins);
    b.resize(0xC0, 0);
    let mut pat = vec![0, 0, 0x20, 0x40, 1, 0];
    pat.extend([0u8; 64]);
    let n = (pat.len() - 2) as u16;
    pat[..2].copy_from_slice(&n.to_le_bytes());
    b.extend(pat);
    b.resize(0x100, 0);
    b.extend(square().iter().map(|s| s.wrapping_add(128)));
    b
}

/// A one-channel XM with a linear-slide instrument playing C-5 (note 49) on row 0.
pub(super) fn build_xm() -> Vec<u8> {
    let mut b = vec![0u8; 336];
    b[..17].copy_from_slice(b"Extended Module: ");
    b[17..23].copy_from_slice(b"square");
    b[37] = 0x1A;
    put32(&mut b, 60, 276);
    put16(&mut b, 64, 1);
    put16(&mut b, 68, 1);
    put16(&mut b, 70, 1);
    put16(&mut b, 72, 1);
    put16(&mut b, 74, 1);
    put16(&mut b, 76, 6);
    put16(&mut b, 78, 125);
    // Pattern: 8 rows of one channel. Row 0: note 49, instrument 1; the rest empty.
    let mut data = vec![0x80 | 1 | 2, 49, 1];
    data.extend([0x80u8; 7]);
    let mut hdr = vec![0u8; 9];
    put32(&mut hdr, 0, 9);
    put16(&mut hdr, 5, 8);
    put16(&mut hdr, 7, data.len() as u16);
    b.extend(hdr);
    b.extend(data);
    // Instrument with one sample.
    let mut ins = vec![0u8; 263];
    put32(&mut ins, 0, 263);
    ins[4..10].copy_from_slice(b"square");
    put16(&mut ins, 27, 1);
    put32(&mut ins, 29, 40);
    b.extend(ins);
    let mut sh = vec![0u8; 40];
    put32(&mut sh, 0, 32);
    put32(&mut sh, 4, 0);
    put32(&mut sh, 8, 32);
    sh[12] = 64;
    sh[14] = 1;
    b.extend(sh);
    let raw = square();
    let mut prev = 0u8;
    for v in raw {
        b.push(v.wrapping_sub(prev));
        prev = v;
    }
    b
}

/// A one-channel IT in sample mode with linear slides playing C-5 on row 0.
pub(super) fn build_it() -> Vec<u8> {
    let mut b = vec![0u8; 192];
    b[..4].copy_from_slice(b"IMPM");
    b[4..10].copy_from_slice(b"square");
    put16(&mut b, 32, 1);
    put16(&mut b, 36, 1);
    put16(&mut b, 38, 1);
    put16(&mut b, 40, 0x214);
    put16(&mut b, 44, 1 | 8);
    b[48] = 128;
    b[49] = 128;
    b[50] = 6;
    b[51] = 125;
    b[64..128].fill(32);
    b[128..192].fill(64);
    b.push(0); // order list
    let smp_ptr = b.len() + 8;
    let sample_at = smp_ptr;
    let pat_at = sample_at + 80;
    let data_at = pat_at + 8 + 70;
    b.extend((sample_at as u32).to_le_bytes());
    b.extend((pat_at as u32).to_le_bytes());
    let mut s = vec![0u8; 80];
    s[..4].copy_from_slice(b"IMPS");
    s[17] = 64;
    s[18] = 1 | 16;
    s[19] = 64;
    s[46] = 1;
    s[47] = 0;
    put32(&mut s, 48, 32);
    put32(&mut s, 52, 0);
    put32(&mut s, 56, 32);
    put32(&mut s, 60, 8363);
    put32(&mut s, 72, data_at as u32);
    b.extend(s);
    let mut p = vec![0u8; 8];
    let mut packed = vec![0x81, 3, 60, 1];
    packed.extend([0u8; 64]);
    put16(&mut p, 0, packed.len() as u16);
    put16(&mut p, 2, 64);
    b.extend(p);
    b.extend(&packed);
    b.resize(data_at, 0);
    b.extend(square());
    b
}
