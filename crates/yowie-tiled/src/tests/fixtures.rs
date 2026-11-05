//! The fixture map in both formats.

use yowie_core::{Color, Vec2};

use super::*;
use crate::{Gid, LayerKind, Orientation, PropertyValue, Shape, TileLayer};

fn tiles<'a>(m: &'a Map, name: &str) -> &'a TileLayer {
    match &m.find_layer(name).expect("layer exists").kind {
        LayerKind::Tiles(t) => t,
        other => panic!("{name} is {other:?}"),
    }
}

#[test]
fn tmx_and_tmj_parse_to_the_same_map() {
    let xml = parse(LEVEL_TMX);
    let json = parse(LEVEL_TMJ);
    assert_eq!(xml, json);
}

#[test]
fn external_tileset_may_be_in_either_format() {
    let as_tsj = String::from_utf8(LEVEL_TMJ.to_vec())
        .unwrap()
        .replace("props.tsx", "props.tsj");
    let mixed = parse(as_tsj.as_bytes());
    let mut want = parse(LEVEL_TMJ);
    for ts in &mut want.tilesets {
        if let Some(src) = &mut ts.source {
            *src = "sets/props.tsj".into();
        }
    }
    assert_eq!(mixed, want);
}

#[test]
fn map_header_and_properties() {
    let m = parse(LEVEL_TMX);
    assert_eq!(m.orientation, Orientation::Orthogonal);
    assert_eq!(
        (m.width, m.height, m.tile_width, m.tile_height),
        (4, 3, 16, 16)
    );
    assert!(!m.infinite);
    assert_eq!(m.background, Some(Color::rgba8(0x33, 0x66, 0x99, 0x80)));
    let p = &m.properties;
    assert_eq!(p.int("difficulty"), Some(3));
    assert_eq!(p.string("title"), Some("Test"));
    assert_eq!(p.float("gravity"), Some(9.5));
    assert_eq!(p.bool("dark"), Some(true));
    assert_eq!(p.color("tint"), Some(Color::rgba8(255, 0, 0, 0x80)));
    let spawn = p.class("spawn").expect("class property");
    assert_eq!(
        (spawn.int("hp"), spawn.float("rate"), spawn.string("name")),
        (Some(10), Some(0.5), Some("orc"))
    );
    assert!(matches!(p.get("spawn"), Some(PropertyValue::Class { class, .. }) if class == "Spawn"));
}

#[test]
fn tilesets_are_sorted_and_external_images_rebased() {
    let m = parse(LEVEL_TMX);
    assert_eq!(
        m.tilesets.iter().map(|t| t.first_gid).collect::<Vec<_>>(),
        [1, 9]
    );
    let ground = &m.tilesets[0];
    assert_eq!(
        ground.image.as_ref().map(|i| (i.source.as_str(), i.width)),
        Some(("img/ground.png", 64))
    );
    let anim = &ground.tiles[&4].animation;
    assert_eq!(
        anim.iter()
            .map(|f| (f.tile_id, f.duration))
            .collect::<Vec<_>>(),
        [(4, 0.25), (5, 0.5)]
    );
    let solid = &ground.tiles[&2];
    assert_eq!(solid.properties.bool("solid"), Some(true));
    assert_eq!(solid.collision.len(), 1);
    assert_eq!(solid.collision[0].rect().h, 8.0);

    let props = &m.tilesets[1];
    assert_eq!(props.source.as_deref(), Some("sets/props.tsx"));
    assert_eq!(props.image.as_ref().unwrap().source, "sets/props.png");
    assert_eq!((props.margin, props.spacing, props.columns), (1, 2, 2));

    assert_eq!(m.tileset_for(Gid(0)), None);
    assert_eq!(
        m.tileset_for(Gid(3)).map(|(t, l)| (t.name.as_str(), l)),
        Some(("ground", 2))
    );
    assert_eq!(
        m.tileset_for(Gid(10)).map(|(t, l)| (t.name.as_str(), l)),
        Some(("props", 1))
    );
    assert_eq!(m.tileset_for(Gid(11)), None, "past the last tile");
    assert_eq!(
        m.tileset_for(Gid(0x8000_0003)).map(|(_, l)| l),
        Some(2),
        "flags are ignored"
    );
    assert_eq!(
        m.tile(Gid(10)).and_then(|t| t.properties.float("weight")),
        Some(2.5)
    );
}

#[test]
fn tile_layers_decode() {
    let m = parse(LEVEL_TMX);
    let ground = tiles(&m, "ground");
    let cells: Vec<u32> = ground.data.iter().map(|g| g.0).collect();
    assert_eq!(cells, [1, 2, 3, 4, 5, 5, 5, 5, 9, 10, 0, 3]);
    assert_eq!(ground.gid_at(1, 2), Gid(10));
    assert_eq!(ground.gid_at(4, 0), Gid(0));
    assert_eq!(tiles(&m, "decor").data.len(), 12);
    let overlay = tiles(&m, "overlay");
    assert_eq!((overlay.data[0], overlay.data[11]), (Gid(9), Gid(10)));
}

#[test]
fn objects_cover_every_shape() {
    let m = parse(LEVEL_TMX);
    let layers = m.object_layers();
    assert_eq!(layers.len(), 1);
    let LayerKind::Objects(things) = &layers[0].layer.kind else {
        panic!("not objects")
    };
    let o = &things.objects;
    assert_eq!(o.len(), 6);
    assert_eq!(
        (o[0].name.as_str(), o[0].class.as_str()),
        ("zone", "trigger")
    );
    assert_eq!(o[0].properties.string("target"), Some("exit"));
    assert_eq!(o[0].properties.int("count"), Some(2));
    assert_eq!(
        o[0].shape,
        Shape::Polygon(vec![
            Vec2::ZERO,
            Vec2::new(32.0, 0.0),
            Vec2::new(16.0, 24.0)
        ])
    );
    assert_eq!(
        (o[1].shape.clone(), o[1].class.as_str()),
        (Shape::Point, "spawn")
    );
    assert_eq!(o[2].shape, Shape::Tile(Gid(0x8000_0002)));
    assert_eq!((o[2].rotation, o[2].width), (45.0, 16.0));
    assert_eq!(o[3].shape, Shape::Ellipse);
    assert!(!o[3].visible);
    assert_eq!(
        o[4].shape,
        Shape::Polyline(vec![Vec2::ZERO, Vec2::new(4.0, 4.0)])
    );
    let Shape::Text(t) = &o[5].shape else {
        panic!("not text")
    };
    assert_eq!((t.text.as_str(), t.wrap), ("hello", true));
}

#[test]
fn group_state_is_applied_to_children() {
    let m = parse(LEVEL_TMX);
    let resolved = m.walk_layers();
    assert_eq!(resolved.len(), 4, "groups are flattened away");
    let overlay = resolved.iter().find(|r| r.layer.name == "overlay").unwrap();
    assert_eq!(overlay.offset, Vec2::new(5.0, -2.0));
    assert_eq!(overlay.opacity, 0.25);
    assert!(!overlay.visible);
    assert_eq!(overlay.parallax, Vec2::new(0.5, 1.0));
    assert_eq!(overlay.tint, Some(Color::rgb8(255, 0x80, 0)));
    assert_eq!(overlay.layer.properties.float("speed"), Some(0.5));
    let ground = resolved.iter().find(|r| r.layer.name == "ground").unwrap();
    assert!(
        (ground.visible, ground.opacity, ground.parallax, ground.tint)
            == (true, 1.0, Vec2::ONE, None)
    );
    assert_eq!(m.tile_layers().len(), 3);
}
