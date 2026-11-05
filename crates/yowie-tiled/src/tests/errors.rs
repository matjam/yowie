//! Malformed and unsupported input.

use super::*;
use crate::{Error, Tileset};

const HEADER: &str = r#""width":2,"height":1,"tilewidth":8,"tileheight":8"#;

#[test]
fn malformed_maps_are_errors() {
    let cases: Vec<(&str, String)> = vec![
        ("empty", String::new()),
        ("not json or xml", "hello".into()),
        ("truncated json", r#"{"width":"#.into()),
        ("truncated xml", "<map><layer>".into()),
        ("wrong xml root", "<foo/>".into()),
        (
            "cell count mismatch",
            format!(r#"{{{HEADER},"layers":[{{"type":"tilelayer","name":"a","width":2,"height":1,"data":[1]}}]}}"#),
        ),
        (
            "missing data",
            format!(r#"{{{HEADER},"layers":[{{"type":"tilelayer","name":"a","width":2,"height":1}}]}}"#),
        ),
        (
            "corrupt zlib",
            format!(
                r#"{{{HEADER},"layers":[{{"type":"tilelayer","name":"a","width":2,"height":1,
                "encoding":"base64","compression":"zlib","data":"AAECAwQF"}}]}}"#
            ),
        ),
        (
            "bad xml gid",
            r#"<map><layer name="a" width="1" height="1"><data><tile gid="x"/></data></layer></map>"#.into(),
        ),
        (
            "bad polygon",
            r#"<map><objectgroup name="o"><object id="1"><polygon points="1;2"/></object></objectgroup></map>"#.into(),
        ),
    ];
    for (name, doc) in cases {
        assert!(Map::parse_embedded(doc.as_bytes()).is_err(), "{name}");
    }
}

#[test]
fn unsupported_features_are_named() {
    let layer = format!(r#"{{{HEADER},"layers":[{{"type":"hologram","name":"x"}}]}}"#);
    assert!(
        matches!(Map::parse_embedded(layer.as_bytes()), Err(Error::Unsupported(m)) if m.contains("hologram"))
    );
    let comp = format!(
        r#"{{{HEADER},"layers":[{{"type":"tilelayer","name":"a","width":2,"height":1,
        "encoding":"base64","compression":"lz4","data":"AAAAAAAAAAA="}}]}}"#
    );
    assert!(matches!(
        Map::parse_embedded(comp.as_bytes()),
        Err(Error::Unsupported(_))
    ));
    let enc = r#"<map><layer name="a" width="1" height="1"><data encoding="rot13">x</data></layer></map>"#;
    assert!(matches!(
        Map::parse_embedded(enc.as_bytes()),
        Err(Error::Unsupported(_))
    ));
}

#[test]
fn external_tileset_errors() {
    assert!(
        matches!(Map::parse_embedded(LEVEL_TMX), Err(Error::NoResolver(p)) if p == "sets/props.tsx")
    );
    let err = Map::parse(LEVEL_TMX, |_| Err("disk on fire".into())).unwrap_err();
    assert!(
        matches!(&err, Error::Resolve { path, .. } if path == "sets/props.tsx"),
        "{err}"
    );
    assert!(err.to_string().contains("disk on fire"));
    let garbage = Map::parse(LEVEL_TMX, |_| Ok(b"not a tileset".to_vec()));
    assert!(garbage.is_err());
}

#[test]
fn wangid_with_too_many_values_is_rejected() {
    let tsj = r#"{"name":"t","wangsets":[{"name":"w","wangtiles":[{"tileid":1,"wangid":[0,0,0,0,0,0,0,0,1]}]}]}"#;
    assert!(Tileset::parse(tsj.as_bytes()).is_err());
    let tsx = r#"<tileset name="t"><wangsets><wangset name="w"><wangtile tileid="1" wangid="1,x"/></wangset></wangsets></tileset>"#;
    assert!(Tileset::parse(tsx.as_bytes()).is_err());
}

#[test]
fn huge_chunks_are_rejected_without_allocating() {
    let doc = r#"{"infinite":true,"layers":[{"type":"tilelayer","name":"a","chunks":[
        {"x":0,"y":0,"width":2000000000,"height":2000000000,"data":[]}]}]}"#;
    assert!(Map::parse_embedded(doc.as_bytes()).is_err());
}
