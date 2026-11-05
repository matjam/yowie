//! Wang sets, legacy terrain, tile offsets and image collections.

use yowie_core::{Color, Vec2};

use crate::{Tileset, WangKind};

const TSX: &str = r##"<?xml version="1.0" encoding="UTF-8"?>
<tileset version="1.10" name="terrain" tilewidth="16" tileheight="16" tilecount="6" columns="3">
 <tileoffset x="-2" y="3"/>
 <image source="terrain.png" width="48" height="32"/>
 <terraintypes>
  <terrain name="grass" tile="0"/>
  <terrain name="water" tile="-1"/>
 </terraintypes>
 <tile id="0" terrain="0,0,0,0"/>
 <tile id="1" terrain="0,,1,1" probability="0.25"/>
 <wangsets>
  <wangset name="ground" type="corner" tile="-1">
   <properties><property name="layer" value="base"/></properties>
   <wangcolor name="grass" color="#00ff00" tile="0" probability="1"/>
   <wangcolor name="water" color="#0000ff" tile="3" probability="0.5"/>
   <wangtile tileid="0" wangid="0,1,0,1,0,1,0,1"/>
   <wangtile tileid="4" wangid="0,2,0,2,0,1,0,2"/>
  </wangset>
  <wangset name="paths" type="edge" tile="2">
   <wangcolor name="dirt" color="#804020" tile="-1" probability="1"/>
   <wangtile tileid="2" wangid="1,0,1,0,0,0,0,0"/>
  </wangset>
 </wangsets>
</tileset>"##;

const TSJ: &str = r##"{
 "type": "tileset", "name": "terrain", "tilewidth": 16, "tileheight": 16, "tilecount": 6, "columns": 3,
 "tileoffset": {"x": -2, "y": 3},
 "image": "terrain.png", "imagewidth": 48, "imageheight": 32,
 "terrains": [{"name": "grass", "tile": 0}, {"name": "water", "tile": -1}],
 "tiles": [
   {"id": 0, "terrain": [0, 0, 0, 0]},
   {"id": 1, "terrain": [0, -1, 1, 1], "probability": 0.25}
 ],
 "wangsets": [
  {"name": "ground", "type": "corner", "tile": -1,
   "properties": [{"name": "layer", "type": "string", "value": "base"}],
   "colors": [
     {"name": "grass", "color": "#00ff00", "tile": 0, "probability": 1},
     {"name": "water", "color": "#0000ff", "tile": 3, "probability": 0.5}],
   "wangtiles": [
     {"tileid": 0, "wangid": [0, 1, 0, 1, 0, 1, 0, 1]},
     {"tileid": 4, "wangid": [0, 2, 0, 2, 0, 1, 0, 2]}]},
  {"name": "paths", "type": "edge", "tile": 2,
   "colors": [{"name": "dirt", "color": "#804020", "tile": -1, "probability": 1}],
   "wangtiles": [{"tileid": 2, "wangid": [1, 0, 1, 0, 0, 0, 0, 0]}]}
 ]
}"##;

#[test]
fn tsx_and_tsj_agree() {
    assert_eq!(
        Tileset::parse(TSX.as_bytes()).unwrap(),
        Tileset::parse(TSJ.as_bytes()).unwrap()
    );
}

#[test]
fn wang_sets_and_terrain_are_read() {
    let ts = Tileset::parse(TSJ.as_bytes()).unwrap();
    assert_eq!(ts.tile_offset, Vec2::new(-2.0, 3.0));
    assert_eq!(
        ts.terrains
            .iter()
            .map(|t| (t.name.as_str(), t.tile))
            .collect::<Vec<_>>(),
        [("grass", 0), ("water", -1)]
    );
    assert_eq!(ts.tiles[&0].terrain, Some([0, 0, 0, 0]));
    assert_eq!(ts.tiles[&1].terrain, Some([0, -1, 1, 1]));
    assert_eq!(ts.tiles[&1].probability, 0.25);
    assert_eq!(ts.tiles[&0].probability, 1.0);

    let ground = &ts.wang_sets[0];
    assert_eq!(
        (ground.name.as_str(), ground.kind, ground.tile),
        ("ground", WangKind::Corner, -1)
    );
    assert_eq!(ground.properties.string("layer"), Some("base"));
    assert_eq!(ground.colors[1].color, Color::rgb8(0, 0, 255));
    assert_eq!(
        (ground.colors[1].tile, ground.colors[1].probability),
        (3, 0.5)
    );
    assert_eq!(ground.tiles.len(), 2);
    assert_eq!(ground.tiles[1].corners(), [2, 2, 1, 2]);
    assert_eq!(ground.tiles[1].edges(), [0, 0, 0, 0]);
    assert_eq!(ground.weight(&ground.tiles[1]), 0.5);
    assert_eq!(
        ground
            .tiles_matching([0, 1, 0, 1, 0, 1, 0, 1])
            .next()
            .map(|t| t.tile_id),
        Some(0)
    );

    let paths = &ts.wang_sets[1];
    assert_eq!(paths.kind, WangKind::Edge);
    assert_eq!(paths.tiles[0].edges(), [1, 1, 0, 0]);
}

#[test]
fn image_collection_tilesets_have_per_tile_images() {
    let tsx = r#"<tileset name="bits" tilewidth="32" tileheight="32" tilecount="2" columns="0">
        <tile id="0"><image source="a.png" width="16" height="16"/></tile>
        <tile id="3"><image source="sub/b.png" width="32" height="24"/></tile></tileset>"#;
    let ts = Tileset::parse(tsx.as_bytes()).unwrap();
    assert!(ts.image.is_none());
    assert_eq!(ts.tile_rect(0), None);
    let b = ts.tiles[&3].image.as_ref().unwrap();
    assert_eq!(
        (b.source.as_str(), b.width, b.height),
        ("sub/b.png", 32, 24)
    );
    assert_eq!(ts.tiles[&0].image.as_ref().unwrap().source, "a.png");
}
