//! Tilesets from JSON.

use std::collections::BTreeMap;

use serde_json::Value;
use yowie_core::{Color, Vec2};

use super::object::objects;
use super::props::properties;
use super::util::{color, f32_or, int_or, list, num, string, string_any, uint};
use crate::error::{Error, Result};
use crate::tileset::{Frame, Tile, Tileset, TilesetImage};
use crate::wang::{Terrain, WangColor, WangKind, WangSet, WangTile};

/// Makes a path written relative to an external tileset relative to the
/// map instead. `dir` is the tileset's directory relative to the map.
pub(super) fn rebase(dir: &str, path: &str) -> String {
    if path.is_empty() || dir.is_empty() {
        return path.to_string();
    }
    // Unlike asset paths, these are relative to the map and may climb above
    // its directory, so leading `..` components are kept.
    let mut parts: Vec<&str> = Vec::new();
    for part in dir.split('/').chain(path.split('/')) {
        match part {
            "" | "." => {}
            ".." if parts.last().is_some_and(|p| *p != "..") => {
                parts.pop();
            }
            p => parts.push(p),
        }
    }
    parts.join("/")
}

fn image(v: &Value, dir: &str) -> Option<TilesetImage> {
    let source = string(v, "image");
    (!source.is_empty()).then(|| TilesetImage {
        source: rebase(dir, source),
        width: uint(v, "imagewidth"),
        height: uint(v, "imageheight"),
        transparent_color: color(v, "transparentcolor"),
    })
}

/// Reads an embedded tileset or the contents of an external one. `dir`
/// rebases image paths; it is empty for embedded tilesets.
pub(super) fn tileset(v: &Value, dir: &str) -> Result<Tileset> {
    let mut tiles = BTreeMap::new();
    for t in tile_values(v)? {
        let tile = tile(&t, dir)?;
        tiles.insert(tile.id, tile);
    }
    Ok(Tileset {
        first_gid: uint(v, "firstgid"),
        source: None,
        name: string(v, "name").to_string(),
        class: string(v, "class").to_string(),
        tile_width: uint(v, "tilewidth"),
        tile_height: uint(v, "tileheight"),
        spacing: uint(v, "spacing"),
        margin: uint(v, "margin"),
        tile_count: uint(v, "tilecount"),
        columns: uint(v, "columns"),
        image: image(v, dir),
        tile_offset: v.get("tileoffset").map_or(Vec2::ZERO, |o| {
            Vec2::new(f32_or(o, "x", 0.0), f32_or(o, "y", 0.0))
        }),
        tiles,
        wang_sets: list(v, "wangsets")
            .iter()
            .map(wang_set)
            .collect::<Result<_>>()?,
        terrains: list(v, "terrains")
            .iter()
            .map(|t| Terrain {
                name: string(t, "name").to_string(),
                tile: int_or(t, "tile", -1),
                properties: properties(t),
            })
            .collect(),
        properties: properties(v),
    })
}

/// The tileset's `tiles`: an array in current files, an object keyed by id
/// in older ones. Each item carries its id.
fn tile_values(v: &Value) -> Result<Vec<Value>> {
    match v.get("tiles") {
        Some(Value::Array(a)) => Ok(a.clone()),
        Some(Value::Object(o)) => {
            let mut out = Vec::new();
            for (k, t) in o {
                let id: u32 = k
                    .parse()
                    .map_err(|_| Error::Invalid(format!("tile id {k:?} is not a number")))?;
                let mut t = t.clone();
                if let Some(m) = t.as_object_mut() {
                    m.insert("id".into(), id.into());
                }
                out.push(t);
            }
            Ok(out)
        }
        _ => Ok(Vec::new()),
    }
}

fn tile(v: &Value, dir: &str) -> Result<Tile> {
    let terrain = v.get("terrain").and_then(Value::as_array).map(|a| {
        let mut t = [-1i32; 4];
        for (slot, item) in t.iter_mut().zip(a) {
            *slot = super::util::as_f64(item).map_or(-1, |n| n as i32);
        }
        t
    });
    Ok(Tile {
        id: uint(v, "id"),
        class: string_any(v, &["class", "type"]).to_string(),
        probability: f32_or(v, "probability", 1.0),
        image: image(v, dir),
        animation: list(v, "animation")
            .iter()
            .map(|f| Frame {
                tile_id: uint(f, "tileid"),
                duration: f32_or(f, "duration", 0.0) / 1000.0,
            })
            .collect(),
        collision: v.get("objectgroup").map(objects).unwrap_or_default(),
        terrain,
        properties: properties(v),
    })
}

fn wang_set(v: &Value) -> Result<WangSet> {
    let name = string(v, "name");
    let mut tiles = Vec::new();
    for t in list(v, "wangtiles") {
        let ids = list(t, "wangid");
        if ids.len() > 8 {
            return Err(Error::Invalid(format!(
                "wangset {name:?} tile {}: wangid has {} values",
                uint(t, "tileid"),
                ids.len()
            )));
        }
        let mut wang_id = [0u32; 8];
        for (slot, item) in wang_id.iter_mut().zip(ids) {
            *slot = super::util::as_f64(item)
                .filter(|n| *n >= 0.0)
                .ok_or_else(|| {
                    Error::Invalid(format!("wangset {name:?}: bad wangid value {item}"))
                })? as u32;
        }
        tiles.push(WangTile {
            tile_id: uint(t, "tileid"),
            wang_id,
        });
    }
    Ok(WangSet {
        name: name.to_string(),
        class: string(v, "class").to_string(),
        kind: match string(v, "type") {
            "corner" => WangKind::Corner,
            "edge" => WangKind::Edge,
            _ => WangKind::Mixed,
        },
        tile: int_or(v, "tile", -1),
        colors: list(v, "colors")
            .iter()
            .map(|c| WangColor {
                name: string(c, "name").to_string(),
                class: string(c, "class").to_string(),
                color: color(c, "color").unwrap_or(Color::WHITE),
                tile: int_or(c, "tile", -1),
                probability: num(c, "probability").map_or(1.0, |p| p as f32),
                properties: properties(c),
            })
            .collect(),
        tiles,
        properties: properties(v),
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn tiles_in_both_shapes_and_rebase() {
        let modern = json!({"name": "t", "image": "t.png", "imagewidth": 32, "tilewidth": 16, "tileheight": 16,
            "tilecount": 4, "columns": 2, "tiles": [{"id": 1, "animation": [{"tileid": 1, "duration": 250}],
            "terrain": [0, 0, 1], "probability": 0.5}]});
        let legacy =
            json!({"tiles": {"7": {"properties": [{"name": "k", "type": "int", "value": 1}]}}});
        let ts = tileset(&modern, "sets").unwrap();
        assert_eq!(ts.image.as_ref().unwrap().source, "sets/t.png");
        let t = &ts.tiles[&1];
        assert_eq!(
            t.animation,
            [Frame {
                tile_id: 1,
                duration: 0.25
            }]
        );
        assert_eq!(t.terrain, Some([0, 0, 1, -1]));
        assert_eq!(t.probability, 0.5);
        let ts = tileset(&legacy, "").unwrap();
        assert_eq!(ts.tiles[&7].properties.int("k"), Some(1));
        assert_eq!(rebase("a/b", "../c.png"), "a/c.png");
        assert_eq!(rebase("../ts", "img/c.png"), "../ts/img/c.png");
        assert_eq!(rebase("a", "../../c.png"), "../c.png");
        assert_eq!(rebase("", "c.png"), "c.png");
    }

    #[test]
    fn wang_sets_and_errors() {
        let v = json!({"wangsets": [{"name": "w", "type": "corner", "tile": 2,
            "colors": [{"name": "grass", "color": "#00ff00", "tile": -1, "probability": 2}],
            "wangtiles": [{"tileid": 5, "wangid": [0, 1, 0, 1, 0, 1, 0, 1]}]}]});
        let ts = tileset(&v, "").unwrap();
        let w = &ts.wang_sets[0];
        assert_eq!(
            (w.kind, w.tile, w.colors[0].probability),
            (WangKind::Corner, 2, 2.0)
        );
        assert_eq!(w.tiles[0].corners(), [1, 1, 1, 1]);
        let long = json!({"wangsets": [{"name": "w", "wangtiles": [{"tileid": 1, "wangid": [0,0,0,0,0,0,0,0,0]}]}]});
        assert!(tileset(&long, "").is_err());
        let neg =
            json!({"wangsets": [{"name": "w", "wangtiles": [{"tileid": 1, "wangid": [-1]}]}]});
        assert!(tileset(&neg, "").is_err());
    }
}
