use super::*;
use crate::error::Error;
use crate::source::Source;

fn sample() -> Vec<(&'static str, Vec<u8>)> {
    vec![
        (
            "a.txt",
            b"hello hello hello hello hello hello hello".to_vec(),
        ),
        ("dir/b.bin", (0..=255u8).collect()),
        ("dir/sub/empty", Vec::new()),
        ("big.txt", "yowie ".repeat(5000).into_bytes()),
    ]
}

fn build(compression: Compression) -> Vec<u8> {
    let mut pack = PackWriter::new(Vec::new(), compression).unwrap();
    for (p, d) in sample() {
        pack.add(p, &d).unwrap();
    }
    pack.finish().unwrap()
}

#[test]
fn round_trip_with_and_without_compression() {
    let mut sizes = Vec::new();
    for compression in [
        Compression::None,
        Compression::Zstd(3),
        Compression::default(),
    ] {
        let bytes = build(compression);
        sizes.push(bytes.len());
        let src = PackSource::from_bytes(bytes).unwrap();
        assert_eq!(src.len(), 4);
        for (p, d) in sample() {
            assert_eq!(src.read(p).unwrap(), d, "{p} with {compression:?}");
        }
        src.verify().unwrap();
        assert!(src.read("missing").unwrap_err().is_not_found());
        assert!(src.exists("dir/sub"));
        let names: Vec<_> = src
            .list("dir")
            .unwrap()
            .into_iter()
            .map(|e| e.name)
            .collect();
        assert_eq!(names, ["b.bin", "sub"]);
    }
    assert!(sizes[1] < sizes[0], "zstd shrinks the pack: {sizes:?}");
}

#[test]
fn corrupted_packs_are_rejected() {
    let good = build(Compression::None);
    let len = good.len();
    let flip = |i: usize| {
        let mut b = good.clone();
        b[i] ^= 0xFF;
        b
    };
    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("bad header magic", flip(0)),
        ("bad version", flip(4)),
        ("bad footer magic", flip(len - 1)),
        ("bad index crc field", flip(len - 6)),
        ("index byte flipped", flip(len - 40)),
        ("truncated", good[..len - 10].to_vec()),
        ("too short", good[..5].to_vec()),
    ];
    for (name, bytes) in cases {
        assert!(
            matches!(PackSource::from_bytes(bytes), Err(Error::CorruptPack(_))),
            "{name}"
        );
    }
}

#[test]
fn data_corruption_fails_the_entry_checksum() {
    for compression in [Compression::None, Compression::Zstd(3)] {
        let mut bytes = build(compression);
        bytes[HEADER_LEN as usize + 2] ^= 0x55;
        let src = PackSource::from_bytes(bytes).unwrap();
        assert!(
            matches!(src.verify(), Err(Error::Checksum { .. })),
            "{compression:?}"
        );
    }
}

#[test]
fn writer_rejects_bad_paths() {
    let mut pack = PackWriter::new(Vec::new(), Compression::None).unwrap();
    pack.add("a", b"1").unwrap();
    assert!(matches!(
        pack.add("./a", b"2"),
        Err(Error::DuplicateEntry(_))
    ));
    assert!(matches!(pack.add("", b"2"), Err(Error::InvalidPath(_))));
    assert!(matches!(pack.add("../x", b"2"), Err(Error::InvalidPath(_))));
}

#[test]
fn pack_dir_round_trip() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("assets");
    std::fs::create_dir_all(src.join("sprites")).unwrap();
    std::fs::write(src.join("readme.txt"), "text ".repeat(100)).unwrap();
    std::fs::write(src.join("sprites/hero.png"), b"\x89PNGfake").unwrap();
    std::fs::write(src.join(".hidden"), b"x").unwrap();
    let out = tmp.path().join("assets.pak");
    pack_dir(&src, &out, Compression::default()).unwrap();
    let pak = PackSource::open(&out).unwrap();
    assert_eq!(pak.paths(), ["readme.txt", "sprites/hero.png"]);
    assert_eq!(pak.read("sprites/hero.png").unwrap(), b"\x89PNGfake");
    assert_eq!(
        pak.read("readme.txt").unwrap(),
        "text ".repeat(100).as_bytes()
    );
}
