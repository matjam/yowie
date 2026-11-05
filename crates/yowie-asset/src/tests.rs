//! Tests for loading, progress and hot reload through [`Assets`].

use std::time::{Duration, Instant};

use super::*;

struct Text;

impl Loader for Text {
    type Asset = String;
    fn load(
        &self,
        bytes: &[u8],
        _path: &str,
        _ctx: &LoadContext<'_>,
    ) -> std::result::Result<String, BoxError> {
        Ok(String::from_utf8(bytes.to_vec())?)
    }
}

/// A loader that reads a second file named by the first line.
struct Include;

impl Loader for Include {
    type Asset = Vec<u8>;
    fn load(
        &self,
        bytes: &[u8],
        _path: &str,
        ctx: &LoadContext<'_>,
    ) -> std::result::Result<Vec<u8>, BoxError> {
        let name = std::str::from_utf8(bytes)?.trim();
        Ok(ctx.read_relative(name)?)
    }
}

fn wait_until(what: &str, mut cond: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if cond() {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("timed out waiting for {what}");
}

fn memory_assets(n: usize) -> Assets {
    let mut mem = MemorySource::new();
    for i in 0..n {
        mem.insert(&format!("f{i}.txt"), format!("file {i}").into_bytes())
            .unwrap();
    }
    let mut vfs = Vfs::new();
    vfs.mount_memory(mem);
    let assets = Assets::new(vfs, 2).unwrap();
    assets.register(Text);
    assets
}

#[test]
fn background_loading_completes_and_reports_progress() {
    let assets = memory_assets(20);
    assert!(assets.progress().is_complete());
    let handles: Vec<_> = (0..20)
        .map(|i| assets.load::<String>(&format!("f{i}.txt")))
        .collect();
    let missing = assets.load::<String>("missing.txt");
    assert_eq!(assets.progress().total, 21);
    wait_until("all loads", || assets.progress().is_complete());
    assert_eq!(assets.progress().fraction(), 1.0);
    for (i, h) in handles.iter().enumerate() {
        assert_eq!(h.state(), LoadState::Ready);
        assert_eq!(h.get().unwrap().as_str(), format!("file {i}"));
        assert_eq!(h.generation(), 1);
    }
    assert_eq!(missing.state(), LoadState::Failed);
    assert!(missing.error().unwrap().is_not_found());
    let events = assets.poll();
    let loaded = events
        .iter()
        .filter(|e| matches!(e, AssetEvent::Loaded { .. }))
        .count();
    let failed = events
        .iter()
        .filter(|e| matches!(e, AssetEvent::Failed { .. }))
        .count();
    assert_eq!((loaded, failed), (20, 1));
    assert!(assets.poll().is_empty());
}

#[test]
fn same_path_shares_one_asset_and_load_now_blocks() {
    let assets = memory_assets(1);
    let a = assets.load::<String>("f0.txt");
    let b = assets.load_now::<String>("./f0.txt").unwrap();
    assert!(std::sync::Arc::ptr_eq(
        &a.wait().unwrap(),
        &b.get().unwrap()
    ));
    assert!(assets.load_now::<String>("nope.txt").is_err());
}

#[test]
fn missing_loader_and_bad_path_fail_the_handle() {
    let assets = memory_assets(1);
    let h = assets.load::<u32>("f0.txt");
    assert!(matches!(h.error().as_deref(), Some(Error::NoLoader(_))));
    let h = assets.load::<String>("../escape");
    assert_eq!(h.state(), LoadState::Failed);
}

#[test]
fn hot_reload_sees_a_rewritten_file_and_its_dependents() {
    let tmp = tempfile::tempdir().unwrap();
    let write = |name: &str, text: &str| std::fs::write(tmp.path().join(name), text).unwrap();
    write("greeting.txt", "hello");
    write("main.inc", "part.txt");
    write("part.txt", "one");
    let mut vfs = Vfs::new();
    vfs.mount_dir(tmp.path()).unwrap();
    let assets = Assets::new(vfs, 2).unwrap();
    assets.register(Text);
    assets.register(Include);
    assets.enable_hot_reload().unwrap();
    let greeting = assets.load_now::<String>("greeting.txt").unwrap();
    let include = assets.load_now::<Vec<u8>>("main.inc").unwrap();
    assert_eq!(greeting.generation(), 1);
    assert_eq!(include.get().unwrap().as_slice(), b"one");

    write("greeting.txt", "goodbye");
    let mut events = Vec::new();
    wait_until("greeting reload", || {
        events.extend(assets.poll());
        greeting.generation() >= 2
    });
    assert_eq!(greeting.get().unwrap().as_str(), "goodbye");
    assert!(
        events
            .iter()
            .any(|e| matches!(e, AssetEvent::Changed { path } if path == "greeting.txt"))
    );
    assert_eq!(include.generation(), 1, "unrelated asset is untouched");

    write("part.txt", "two");
    wait_until("dependent reload", || {
        assets.poll();
        include.generation() >= 2
    });
    assert_eq!(include.get().unwrap().as_slice(), b"two");
}

#[test]
fn failed_reload_keeps_the_old_value() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("a.txt"), "fine").unwrap();
    let mut vfs = Vfs::new();
    vfs.mount_dir(tmp.path()).unwrap();
    let assets = Assets::new(vfs, 1).unwrap();
    assets.register(Text);
    assets.enable_hot_reload().unwrap();
    let h = assets.load_now::<String>("a.txt").unwrap();
    std::fs::write(tmp.path().join("a.txt"), [0xFF, 0xFE]).unwrap();
    wait_until("failed reload", || {
        assets.poll();
        h.error().is_some()
    });
    assert_eq!(h.get().unwrap().as_str(), "fine");
    assert_eq!(h.state(), LoadState::Ready);
    assert_eq!(h.generation(), 1);
}
