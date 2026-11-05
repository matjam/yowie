//! Loading through the VFS and the asset manager.

use std::time::{Duration, Instant};

use yowie_asset::{Assets, MemorySource, Vfs};

use super::*;
use crate::{MapLoader, Tileset};

/// A map in `maps/` using a tileset in the sibling `tilesets/` directory,
/// whose image sits in `tilesets/img/`.
const MAP: &str = r#"<map width="1" height="1" tilewidth="8" tileheight="8">
    <tileset firstgid="1" source="../tilesets/ground.tsx"/>
    <layer id="1" name="g" width="1" height="1"><data encoding="csv">1</data></layer></map>"#;

fn tsx(name: &str) -> String {
    format!(
        r#"<tileset name="{name}" tilewidth="8" tileheight="8" tilecount="1" columns="1">
             <image source="img/{name}.png" width="8" height="8"/></tileset>"#
    )
}

fn memory_vfs() -> Vfs {
    let mem = MemorySource::new()
        .with("maps/level.tmx", MAP.as_bytes().to_vec())
        .unwrap()
        .with("tilesets/ground.tsx", tsx("ground").into_bytes())
        .unwrap();
    let mut vfs = Vfs::new();
    vfs.mount_memory(mem);
    vfs
}

#[test]
fn external_tilesets_resolve_relative_to_the_map() {
    let map = Map::load(&memory_vfs(), "maps/level.tmx").unwrap();
    let ts = &map.tilesets[0];
    assert_eq!(ts.name, "ground");
    assert_eq!(ts.source.as_deref(), Some("../tilesets/ground.tsx"));
    // Image paths are rebased from the tileset's directory to the map's.
    assert_eq!(
        ts.image.as_ref().unwrap().source,
        "../tilesets/img/ground.png"
    );
}

#[test]
fn loading_a_missing_map_or_tileset_fails() {
    let vfs = memory_vfs();
    assert!(matches!(
        Map::load(&vfs, "maps/none.tmx"),
        Err(crate::Error::Asset(_))
    ));
    let mut broken = Vfs::new();
    broken.mount_memory(
        MemorySource::new()
            .with("maps/level.tmx", MAP.as_bytes().to_vec())
            .unwrap(),
    );
    assert!(matches!(
        Map::load(&broken, "maps/level.tmx"),
        Err(crate::Error::Resolve { .. })
    ));
    assert_eq!(
        Tileset::load(&vfs, "tilesets/ground.tsx").unwrap().name,
        "ground"
    );
}

#[test]
fn map_loader_loads_in_the_background() {
    let assets = Assets::new(memory_vfs(), 2).unwrap();
    assets.register(MapLoader);
    let h = assets.load::<Map>("maps/level.tmx");
    let map = h.wait().unwrap();
    assert_eq!(map.tilesets[0].name, "ground");
    assert_eq!(h.generation(), 1);
}

#[test]
fn editing_an_external_tileset_reloads_the_map() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("maps")).unwrap();
    std::fs::create_dir_all(tmp.path().join("tilesets")).unwrap();
    std::fs::write(tmp.path().join("maps/level.tmx"), MAP).unwrap();
    std::fs::write(tmp.path().join("tilesets/ground.tsx"), tsx("ground")).unwrap();
    let mut vfs = Vfs::new();
    vfs.mount_dir(tmp.path()).unwrap();
    let assets = Assets::new(vfs, 2).unwrap();
    assets.register(MapLoader);
    assets.enable_hot_reload().unwrap();
    let h = assets.load_now::<Map>("maps/level.tmx").unwrap();
    assert_eq!(h.get().unwrap().tilesets[0].name, "ground");

    std::fs::write(tmp.path().join("tilesets/ground.tsx"), tsx("renamed")).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while h.generation() < 2 && Instant::now() < deadline {
        assets.poll();
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(h.generation() >= 2, "map was not reloaded");
    assert_eq!(h.get().unwrap().tilesets[0].name, "renamed");
}
