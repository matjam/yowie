//! Layers from JSON.

use serde_json::Value;

use super::object::objects;
use super::props::properties;
use super::util::{color, f32_or, flag, int_or, list, string, uint, vec2};
use crate::data::{decode_base64, decode_csv};
use crate::error::{Error, Result, invalid};
use crate::gid::Gid;
use crate::layer::{Chunk, DrawOrder, ImageLayer, Layer, LayerKind, ObjectLayer, TileLayer};

/// Largest chunk side, in tiles.
const MAX_CHUNK_SIDE: u32 = 1 << 12;
/// Farthest a chunk may sit from the origin, in tiles.
const MAX_COORD: i32 = 1 << 24;

/// Reads a `layers` array.
pub(super) fn layers(v: &Value) -> Result<Vec<Layer>> {
    list(v, "layers").iter().map(layer).collect()
}

fn layer(v: &Value) -> Result<Layer> {
    let name = string(v, "name");
    let ctx = |e: Error| match e {
        Error::Invalid(m) => Error::Invalid(format!("layer {name:?}: {m}")),
        Error::Data(m) => Error::Data(format!("layer {name:?}: {m}")),
        other => other,
    };
    let kind = match string(v, "type") {
        "tilelayer" => LayerKind::Tiles(tile_layer(v).map_err(ctx)?),
        "objectgroup" => LayerKind::Objects(ObjectLayer {
            objects: objects(v),
            draw_order: if string(v, "draworder") == "index" {
                DrawOrder::Index
            } else {
                DrawOrder::TopDown
            },
            color: color(v, "color"),
        }),
        "imagelayer" => LayerKind::Image(ImageLayer {
            image: string(v, "image").to_string(),
            transparent_color: color(v, "transparentcolor"),
            repeat_x: flag(v, "repeatx", false),
            repeat_y: flag(v, "repeaty", false),
        }),
        "group" => LayerKind::Group(layers(v).map_err(ctx)?),
        other => {
            return Err(Error::Unsupported(format!(
                "layer {name:?} has type {other:?}"
            )));
        }
    };
    Ok(Layer {
        id: uint(v, "id"),
        name: name.to_string(),
        class: string(v, "class").to_string(),
        visible: flag(v, "visible", true),
        opacity: f32_or(v, "opacity", 1.0),
        offset: vec2(v, "offsetx", "offsety", 0.0),
        parallax: vec2(v, "parallaxx", "parallaxy", 1.0),
        tint: color(v, "tintcolor"),
        properties: properties(v),
        kind,
    })
}

fn tile_layer(v: &Value) -> Result<TileLayer> {
    let encoding = string(v, "encoding");
    let compression = string(v, "compression");
    let chunks = list(v, "chunks");
    if chunks.is_empty() {
        let (width, height) = (uint(v, "width"), uint(v, "height"));
        let data = cells(v.get("data"), encoding, compression)?;
        if data.len() as u64 != u64::from(width) * u64::from(height) {
            return invalid(format!("{} cells for a {width}x{height} layer", data.len()));
        }
        return Ok(TileLayer {
            x: 0,
            y: 0,
            width,
            height,
            data,
            chunks: Vec::new(),
        });
    }
    let mut out = Vec::with_capacity(chunks.len());
    for c in chunks {
        let (x, y) = (int_or(c, "x", 0), int_or(c, "y", 0));
        let (width, height) = (uint(c, "width"), uint(c, "height"));
        if width == 0
            || height == 0
            || width > MAX_CHUNK_SIDE
            || height > MAX_CHUNK_SIDE
            || x.abs() > MAX_COORD
            || y.abs() > MAX_COORD
        {
            return invalid(format!(
                "chunk ({x},{y}) of {width}x{height} is out of range"
            ));
        }
        let data = cells(c.get("data"), encoding, compression)
            .map_err(|e| Error::Data(format!("chunk ({x},{y}): {e}")))?;
        if data.len() != width as usize * height as usize {
            return invalid(format!(
                "chunk ({x},{y}): {} cells for {width}x{height}",
                data.len()
            ));
        }
        out.push(Chunk {
            x,
            y,
            width,
            height,
            data,
        });
    }
    let x0 = out.iter().map(|c| c.x).min().unwrap_or(0);
    let y0 = out.iter().map(|c| c.y).min().unwrap_or(0);
    let x1 = out.iter().map(|c| c.x + c.width as i32).max().unwrap_or(0);
    let y1 = out.iter().map(|c| c.y + c.height as i32).max().unwrap_or(0);
    Ok(TileLayer {
        x: x0,
        y: y0,
        width: (x1 - x0) as u32,
        height: (y1 - y0) as u32,
        data: Vec::new(),
        chunks: out,
    })
}

/// Reads cells from a JSON array of ids or a base64 string.
fn cells(data: Option<&Value>, encoding: &str, compression: &str) -> Result<Vec<Gid>> {
    match data {
        Some(Value::Array(items)) => items
            .iter()
            .map(|n| {
                n.as_u64()
                    .and_then(|u| u32::try_from(u).ok())
                    .map(Gid)
                    .ok_or_else(|| Error::Data(format!("bad cell {n}")))
            })
            .collect(),
        Some(Value::String(text)) => match encoding {
            "base64" => decode_base64(text, compression),
            "csv" => decode_csv(text),
            other => Err(Error::Unsupported(format!("encoding {other:?}"))),
        },
        _ => invalid("missing \"data\""),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use yowie_core::Vec2;

    use super::*;

    #[test]
    fn finite_layer_count_is_checked() {
        let ok = json!({"layers": [{"type": "tilelayer", "name": "a", "width": 2, "height": 1, "data": [1, 2]}]});
        assert_eq!(layers(&ok).unwrap().len(), 1);
        let bad = json!({"layers": [{"type": "tilelayer", "name": "a", "width": 2, "height": 2, "data": [1, 2]}]});
        let err = layers(&bad).unwrap_err().to_string();
        assert!(err.contains("\"a\"") && err.contains("2x2"), "{err}");
    }

    #[test]
    fn chunk_bounds_and_defaults() {
        let v = json!({"layers": [{"type": "tilelayer", "name": "inf", "visible": false, "opacity": 0.5,
            "parallaxx": 0.5, "tintcolor": "#ff8000", "chunks": [
            {"x": -2, "y": -1, "width": 2, "height": 2, "data": [1, 0, 0, 2]},
            {"x": 0, "y": 0, "width": 2, "height": 1, "data": [3, 4]}]}]});
        let l = &layers(&v).unwrap()[0];
        let LayerKind::Tiles(t) = &l.kind else {
            panic!("not tiles")
        };
        assert_eq!((t.x, t.y, t.width, t.height), (-2, -1, 4, 2));
        assert_eq!(t.gid_at(-2, -1), Gid(1));
        assert_eq!(t.gid_at(0, 0), Gid(3));
        assert!(!l.visible);
        assert_eq!((l.opacity, l.parallax), (0.5, Vec2::new(0.5, 1.0)));
        assert!(l.tint.is_some());
    }

    #[test]
    fn bad_chunks_and_types_are_rejected() {
        let cases = [
            json!({"layers": [{"type": "tilelayer", "chunks": [{"x": 0, "y": 0, "width": 0, "height": 1, "data": []}]}]}),
            json!({"layers": [{"type": "tilelayer", "chunks": [{"x": 0, "y": 0, "width": 2, "height": 1, "data": [1]}]}]}),
            json!({"layers": [{"type": "tilelayer", "chunks": [{"x": 99999999, "y": 0, "width": 1, "height": 1, "data": [1]}]}]}),
            json!({"layers": [{"type": "tilelayer", "width": 1, "height": 1}]}),
            json!({"layers": [{"type": "tilelayer", "width": 1, "height": 1, "data": "AQAAAA==", "encoding": "ascii"}]}),
            json!({"layers": [{"type": "hologram"}]}),
        ];
        for (i, c) in cases.iter().enumerate() {
            assert!(layers(c).is_err(), "case {i}");
        }
    }
}
