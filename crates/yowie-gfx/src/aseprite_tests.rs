//! Tests against an Aseprite file built in memory.

use super::*;

fn word(out: &mut Vec<u8>, v: u16) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn dword(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn string(out: &mut Vec<u8>, s: &str) {
    word(out, s.len() as u16);
    out.extend_from_slice(s.as_bytes());
}

fn chunk(kind: u16, body: &[u8]) -> Vec<u8> {
    let mut c = Vec::new();
    dword(&mut c, body.len() as u32 + 6);
    word(&mut c, kind);
    c.extend_from_slice(body);
    c
}

/// A 4x2 RGBA sprite with one layer, frames of the given duration (ms)
/// and solid colours, the tag `walk` over frames 1 to 3 and a slice `hit`.
fn build(durations: &[u16]) -> Vec<u8> {
    let (w, h) = (4u16, 2u16);
    let mut frames = Vec::new();
    for (i, &d) in durations.iter().enumerate() {
        let mut chunks = Vec::new();
        if i == 0 {
            let mut layer = Vec::new();
            word(&mut layer, 3); // visible, editable
            word(&mut layer, 0); // normal layer
            word(&mut layer, 0); // child level
            word(&mut layer, 0);
            word(&mut layer, 0);
            word(&mut layer, 0); // blend mode normal
            layer.push(255);
            layer.extend_from_slice(&[0; 3]);
            string(&mut layer, "Layer 1");
            chunks.push(chunk(0x2004, &layer));
        }
        let mut cel = Vec::new();
        word(&mut cel, 0); // layer 0
        cel.extend_from_slice(&0i16.to_le_bytes());
        cel.extend_from_slice(&0i16.to_le_bytes());
        cel.push(255);
        word(&mut cel, 0); // raw cel
        cel.extend_from_slice(&0i16.to_le_bytes());
        cel.extend_from_slice(&[0; 5]);
        word(&mut cel, w);
        word(&mut cel, h);
        for _ in 0..(w * h) {
            cel.extend_from_slice(&[(i as u8 + 1) * 40, 10, 20, 255]);
        }
        chunks.push(chunk(0x2005, &cel));
        if i == 0 {
            let mut tags = Vec::new();
            word(&mut tags, 1);
            tags.extend_from_slice(&[0; 8]);
            word(&mut tags, 1); // from
            word(&mut tags, 3); // to
            tags.push(2); // ping-pong
            word(&mut tags, 0);
            tags.extend_from_slice(&[0; 6]);
            tags.extend_from_slice(&[0, 0, 0, 0]);
            string(&mut tags, "walk");
            chunks.push(chunk(0x2018, &tags));
            let mut slice = Vec::new();
            dword(&mut slice, 1);
            dword(&mut slice, 0);
            dword(&mut slice, 0);
            string(&mut slice, "hit");
            dword(&mut slice, 0);
            for v in [1i32, 0] {
                slice.extend_from_slice(&v.to_le_bytes());
            }
            dword(&mut slice, 2);
            dword(&mut slice, 2);
            chunks.push(chunk(0x2022, &slice));
        }
        let mut frame = Vec::new();
        let body: Vec<u8> = chunks.concat();
        dword(&mut frame, 16 + body.len() as u32);
        word(&mut frame, 0xF1FA);
        word(&mut frame, chunks.len() as u16);
        word(&mut frame, d);
        frame.extend_from_slice(&[0; 2]);
        dword(&mut frame, chunks.len() as u32);
        frame.extend_from_slice(&body);
        frames.push(frame);
    }
    let body: Vec<u8> = frames.concat();
    let mut file = Vec::new();
    dword(&mut file, 128 + body.len() as u32);
    word(&mut file, 0xA5E0);
    word(&mut file, durations.len() as u16);
    word(&mut file, w);
    word(&mut file, h);
    word(&mut file, 32);
    dword(&mut file, 1);
    word(&mut file, 100);
    dword(&mut file, 0);
    dword(&mut file, 0);
    file.push(0);
    file.extend_from_slice(&[0; 3]);
    word(&mut file, 0);
    file.push(1);
    file.push(1);
    file.extend_from_slice(&0i16.to_le_bytes());
    file.extend_from_slice(&0i16.to_le_bytes());
    word(&mut file, 0);
    word(&mut file, 0);
    file.extend_from_slice(&[0; 84]);
    assert_eq!(file.len(), 128);
    file.extend_from_slice(&body);
    file
}

#[test]
fn frames_durations_tags_and_slices_come_through() {
    let data = AsepriteData::from_bytes(&build(&[100, 200, 300, 400, 500])).expect("parses");
    assert_eq!(data.size, uvec2(4, 2));
    assert_eq!(data.frames.len(), 5);
    assert_eq!(data.durations, [0.1, 0.2, 0.3, 0.4, 0.5]);
    // Each frame is flattened to its own colour.
    for (i, f) in data.frames.iter().enumerate() {
        assert_eq!(f.get_pixel(0, 0).0[0], (i as u8 + 1) * 40, "frame {i}");
    }
    let tag = data.tag("walk").expect("tag");
    assert_eq!(
        (tag.from, tag.to, tag.direction),
        (1, 3, TagDirection::PingPong)
    );
    assert_eq!(data.tag_frames("walk"), Some(vec![1, 2, 3, 2]));
    let slice = data.slice("hit").expect("slice");
    assert_eq!(
        slice.key_at(4).map(|k| k.bounds),
        Some(Rect::new(1.0, 0.0, 2.0, 2.0))
    );
    assert!(data.tag_frames("missing").is_none());
}

#[test]
fn flipbook_carries_durations_and_tags() {
    let data = AsepriteData::from_bytes(&build(&[100, 100, 100, 100])).expect("parses");
    let book = data.flipbook();
    assert_eq!(book.frames.len(), 4);
    assert_eq!(
        book.frames[2],
        FlipFrame {
            index: 2,
            duration: 0.1
        }
    );
    let tag = book.tag("walk").expect("tag");
    assert_eq!((tag.from, tag.to), (1, 3));
    let mut player = yowie_anim::FlipbookPlayer::new(std::sync::Arc::new(book));
    assert!(player.play_tag("walk", yowie_anim::LoopMode::Loop));
    assert_eq!(player.frame(), Some(1));
}

#[test]
fn pack_grid_places_frames() {
    let data = AsepriteData::from_bytes(&build(&[100; 5])).expect("parses");
    let (img, grid) = data.pack_grid();
    assert_eq!((grid.columns, grid.rows), (3, 2));
    assert_eq!(img.dimensions(), (12, 4));
    let r = grid.frame_rect(4).expect("frame 4");
    assert_eq!(img.get_pixel(r.x as u32, r.y as u32).0[0], 200);
}

#[test]
fn bad_bytes_are_an_error() {
    assert!(matches!(
        AsepriteData::from_bytes(b"not an aseprite file at all"),
        Err(GfxError::Aseprite(_))
    ));
}
