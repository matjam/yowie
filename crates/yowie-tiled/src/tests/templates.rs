//! Object templates: inheritance, overrides, gid placement and errors.

use std::collections::HashMap;

use yowie_asset::BoxError;

use super::*;
use crate::{Error, Gid, LayerKind, Object, Shape};

const TSX: &str = r#"<tileset name="props" tilewidth="16" tileheight="16" tilecount="4" columns="2">
  <image source="props.png" width="32" height="32"/></tileset>"#;

const CRATE_TX: &str = r#"<template>
  <tileset firstgid="1" source="../sets/props.tsx"/>
  <object name="crate" type="Crate" gid="3" width="16" height="16">
    <properties>
      <property name="hp" type="int" value="5"/>
      <property name="loot" value="coin"/>
    </properties>
  </object></template>"#;

const COIN_TJ: &str = r#"{"type":"template","object":{"name":"coin","width":8,"height":8,
  "ellipse":true,"properties":[{"name":"value","type":"int","value":10}]}}"#;

fn files() -> HashMap<&'static str, &'static str> {
    HashMap::from([
        ("sets/props.tsx", TSX),
        ("templates/crate.tx", CRATE_TX),
        ("templates/coin.tj", COIN_TJ),
    ])
}

fn resolver(path: &str) -> Result<Vec<u8>, BoxError> {
    files()
        .get(path)
        .map(|s| s.as_bytes().to_vec())
        .ok_or_else(|| format!("no file {path}").into())
}

fn objects(map: &Map) -> &[Object] {
    let LayerKind::Objects(o) = &map.layers[0].kind else {
        panic!("not an object layer")
    };
    &o.objects
}

fn tmx(body: &str) -> String {
    format!(
        r#"<map width="2" height="2" tilewidth="16" tileheight="16">
  <tileset firstgid="5" source="sets/props.tsx"/>
  <objectgroup id="1" name="things">{body}</objectgroup></map>"#
    )
}

#[test]
fn an_instance_inherits_the_template_and_keeps_its_overrides() {
    let doc = tmx(r#"
      <object id="1" template="templates/crate.tx" x="32" y="48"/>
      <object id="2" template="templates/crate.tx" name="big" x="0" y="16" width="32">
        <properties><property name="hp" type="int" value="9"/>
        <property name="locked" type="bool" value="true"/></properties>
      </object>"#);
    let map = Map::parse(doc.as_bytes(), resolver).expect("parses");
    let objs = objects(&map);
    let (a, b) = (&objs[0], &objs[1]);
    assert_eq!((a.id, a.x, a.y), (1, 32.0, 48.0));
    assert_eq!((a.name.as_str(), a.class.as_str()), ("crate", "Crate"));
    assert_eq!((a.width, a.height), (16.0, 16.0));
    assert_eq!(a.properties.int("hp"), Some(5));
    assert_eq!(a.properties.string("loot"), Some("coin"));
    assert_eq!(a.template.as_deref(), Some("templates/crate.tx"));

    assert_eq!(b.name, "big");
    assert_eq!((b.width, b.height), (32.0, 16.0));
    assert_eq!(b.properties.int("hp"), Some(9));
    assert_eq!(b.properties.string("loot"), Some("coin"));
    assert_eq!(b.properties.bool("locked"), Some(true));
}

#[test]
fn the_template_gid_moves_to_the_maps_numbering() {
    let doc = tmx(
        r#"<object id="1" template="templates/crate.tx" x="0" y="0"/>
      <object id="2" template="templates/crate.tx" gid="6" x="0" y="0"/>"#,
    );
    let map = Map::parse(doc.as_bytes(), resolver).expect("parses");
    // The template says tile 3 of a tileset starting at 1, the map keeps
    // that tileset from 5, so the tile is 7 there; an own gid wins.
    assert_eq!(objects(&map)[0].shape, Shape::Tile(Gid(7)));
    assert_eq!(objects(&map)[1].shape, Shape::Tile(Gid(6)));
    assert_eq!(map.tilesets.len(), 1);
}

#[test]
fn a_template_tileset_the_map_lacks_is_added() {
    let doc = r#"<map width="1" height="1" tilewidth="16" tileheight="16">
      <objectgroup id="1" name="o"><object id="1" template="templates/crate.tx" x="0" y="0"/>
      </objectgroup></map>"#;
    let map = Map::parse(doc.as_bytes(), resolver).expect("parses");
    assert_eq!(map.tilesets.len(), 1);
    assert_eq!(map.tilesets[0].name, "props");
    assert_eq!(map.tilesets[0].first_gid, 1);
    assert_eq!(objects(&map)[0].shape, Shape::Tile(Gid(3)));
    // The tile resolves through the map like any other.
    assert!(map.tileset_for(Gid(3)).is_some());
}

#[test]
fn json_maps_and_templates_work_and_shapes_come_from_the_template() {
    let doc = r#"{"width":1,"height":1,"tilewidth":8,"tileheight":8,"tilesets":[],
      "layers":[{"type":"objectgroup","name":"o","objects":[
        {"id":3,"template":"templates/coin.tj","x":4,"y":5,
         "properties":[{"name":"value","type":"int","value":50}]}]}]}"#;
    let map = Map::parse(doc.as_bytes(), resolver).expect("parses");
    let o = &objects(&map)[0];
    assert_eq!((o.id, o.name.as_str(), o.x, o.y), (3, "coin", 4.0, 5.0));
    assert_eq!(o.shape, Shape::Ellipse);
    assert_eq!(o.properties.int("value"), Some(50));
}

#[test]
fn templates_in_groups_resolve_and_bad_ones_fail() {
    let grouped = r#"<map width="1" height="1" tilewidth="8" tileheight="8">
      <group id="9" name="g"><objectgroup id="1" name="o">
        <object id="1" template="templates/crate.tx" x="0" y="0"/></objectgroup></group></map>"#;
    let map = Map::parse(grouped.as_bytes(), resolver).expect("parses");
    let LayerKind::Group(children) = &map.layers[0].kind else {
        panic!("not a group")
    };
    let LayerKind::Objects(o) = &children[0].kind else {
        panic!("not objects")
    };
    assert_eq!(o.objects[0].name, "crate");

    let missing = tmx(r#"<object id="1" template="templates/none.tx" x="0" y="0"/>"#);
    assert!(matches!(
        Map::parse(missing.as_bytes(), resolver),
        Err(Error::Resolve { .. })
    ));
    assert!(matches!(
        Map::parse_embedded(missing.as_bytes()),
        Err(Error::NoResolver(_))
    ));
}
