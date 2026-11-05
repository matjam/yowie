//! Every tile layer encoding, in both formats, plus flip flags and chunks.

use super::*;
use crate::data::tests::encode;
use crate::{Gid, LayerKind, TileLayer};

/// Cells with each flag bit set at least once, as a 4x2 layer.
const CELLS: [u32; 8] = [
    1,
    0x8000_0002,
    0x4000_0003,
    0x2000_0004,
    0x1000_0005,
    0xE000_0006,
    0,
    7,
];

fn layer_of(m: &Map) -> &TileLayer {
    match &m.layers[0].kind {
        LayerKind::Tiles(t) => t,
        other => panic!("{other:?}"),
    }
}

fn json_map(data_fields: &str) -> String {
    format!(
        r#"{{"width":4,"height":2,"tilewidth":8,"tileheight":8,"layers":[
            {{"type":"tilelayer","name":"l","width":4,"height":2,{data_fields}}}]}}"#
    )
}

fn xml_map(data: &str) -> String {
    format!(
        r#"<map width="4" height="2" tilewidth="8" tileheight="8">
             <layer name="l" width="4" height="2">{data}</layer></map>"#
    )
}

fn cell_list() -> String {
    CELLS
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(",")
}

#[test]
fn every_encoding_gives_the_same_cells() {
    let mut docs: Vec<(String, String)> = vec![
        (
            "json array".into(),
            json_map(&format!(r#""data":[{}]"#, cell_list())),
        ),
        (
            "xml csv".into(),
            xml_map(&format!(r#"<data encoding="csv">{}</data>"#, cell_list())),
        ),
        (
            "xml csv multi-line".into(),
            xml_map(
                "<data encoding=\"csv\">\n1,2147483650,1073741827,536870916,\n268435461,3758096390,0,7\n</data>",
            ),
        ),
        (
            "xml tile elements".into(),
            xml_map(&format!(
                "<data>{}</data>",
                CELLS
                    .iter()
                    .map(|c| format!(r#"<tile gid="{c}"/>"#))
                    .collect::<String>()
            )),
        ),
    ];
    for comp in ["", "zlib", "gzip", "zstd"] {
        let text = encode(&CELLS, comp);
        let attr = if comp.is_empty() {
            String::new()
        } else {
            format!(r#" compression="{comp}""#)
        };
        let jcomp = if comp.is_empty() {
            String::new()
        } else {
            format!(r#","compression":"{comp}""#)
        };
        docs.push((
            format!("json base64 {comp:?}"),
            json_map(&format!(r#""encoding":"base64"{jcomp},"data":"{text}""#)),
        ));
        docs.push((
            format!("xml base64 {comp:?}"),
            xml_map(&format!(
                "<data encoding=\"base64\"{attr}>\n   {text}\n  </data>"
            )),
        ));
    }
    for (name, doc) in &docs {
        let map = Map::parse_embedded(doc.as_bytes()).unwrap_or_else(|e| panic!("{name}: {e}"));
        let got: Vec<u32> = layer_of(&map).data.iter().map(|g| g.0).collect();
        assert_eq!(got, CELLS, "{name}");
    }
}

#[test]
fn flip_and_hex_rotation_flags_survive() {
    let m =
        Map::parse_embedded(json_map(&format!(r#""data":[{}]"#, cell_list())).as_bytes()).unwrap();
    let l = layer_of(&m);
    let f = |i: usize| l.data[i].flags();
    assert!(f(1).horizontal && !f(1).vertical && !f(1).diagonal);
    assert!(f(2).vertical && !f(2).horizontal);
    assert!(f(3).diagonal);
    assert!(f(4).rotate_120 && !f(4).horizontal);
    assert!(f(5).horizontal && f(5).vertical && f(5).diagonal && !f(5).rotate_120);
    assert_eq!(l.data[5].tile_id(), 6);
    assert!(l.data[6].is_empty());
}

#[test]
fn infinite_maps_keep_chunks_in_both_formats() {
    let a = [1u32, 2, 3, 4];
    let b = [5u32, 0, 0, 6];
    let json = format!(
        r#"{{"infinite":true,"tilewidth":8,"tileheight":8,"layers":[{{"type":"tilelayer","name":"l",
        "width":4,"height":4,"startx":-2,"starty":-2,"encoding":"base64","compression":"zlib","chunks":[
        {{"x":-2,"y":-2,"width":2,"height":2,"data":"{}"}},
        {{"x":0,"y":0,"width":2,"height":2,"data":"{}"}}]}}]}}"#,
        encode(&a, "zlib"),
        encode(&b, "zlib")
    );
    let xml = r#"<map infinite="1" tilewidth="8" tileheight="8"><layer name="l" width="4" height="4">
        <data encoding="csv">
          <chunk x="-2" y="-2" width="2" height="2">1,2,3,4</chunk>
          <chunk x="0" y="0" width="2" height="2">5,0,0,6</chunk>
        </data></layer></map>"#;
    let from_json = Map::parse_embedded(json.as_bytes()).unwrap();
    let from_xml = Map::parse_embedded(xml.as_bytes()).unwrap();
    assert!(from_json.infinite && from_xml.infinite);
    // The XML layer element has no compression attribute, so only the cells compare.
    assert_eq!(layer_of(&from_json), layer_of(&from_xml));
    let l = layer_of(&from_json);
    assert_eq!((l.x, l.y, l.width, l.height), (-2, -2, 4, 4));
    assert!(l.data.is_empty() && l.chunks.len() == 2);
    assert_eq!(l.gid_at(-2, -2), Gid(1));
    assert_eq!(l.gid_at(-1, -1), Gid(4));
    assert_eq!(l.gid_at(0, 0), Gid(5));
    assert_eq!(l.gid_at(1, 1), Gid(6));
    assert_eq!(
        l.gid_at(1, -2),
        Gid(0),
        "inside the bounds but outside any chunk"
    );
    let dense: Vec<u32> = l.dense().iter().map(|g| g.0).collect();
    assert_eq!(dense, [1, 2, 0, 0, 3, 4, 0, 0, 0, 0, 5, 0, 0, 0, 0, 6]);
}
