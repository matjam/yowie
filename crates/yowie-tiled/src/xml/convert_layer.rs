//! XML layers and objects in TMJ shape.

use serde_json::{Map, Value, json};

use super::convert::{attrs, image_into, properties_json};
use super::dom::Element;
use crate::data::decode_csv;
use crate::error::{Error, Result};

/// Converts a `<layer>`, `<objectgroup>`, `<imagelayer>` or `<group>`.
pub(super) fn layer_json(el: &Element) -> Result<Value> {
    let mut m = attrs(el);
    let kind = match el.name.as_str() {
        "layer" => "tilelayer",
        "objectgroup" => "objectgroup",
        "imagelayer" => "imagelayer",
        _ => "group",
    };
    m.insert("type".into(), kind.into());
    let mut objects = Vec::new();
    let mut children = Vec::new();
    for c in &el.children {
        match c.name.as_str() {
            "properties" => {
                m.insert("properties".into(), properties_json(c)?);
            }
            "data" => data_into(&mut m, c)?,
            "object" => objects.push(object_json(c)?),
            "image" => image_into(&mut m, c),
            "layer" | "objectgroup" | "imagelayer" | "group" => children.push(layer_json(c)?),
            _ => {}
        }
    }
    match kind {
        "objectgroup" => {
            m.insert("objects".into(), Value::Array(objects));
        }
        "group" => {
            m.insert("layers".into(), Value::Array(children));
        }
        _ => {}
    }
    Ok(Value::Object(m))
}

/// Fills `data` (or `chunks`), `encoding` and `compression` from a `<data>`
/// element.
fn data_into(m: &mut Map<String, Value>, data: &Element) -> Result<()> {
    let encoding = data.attr("encoding").unwrap_or("");
    if let Some(c) = data.attr("compression") {
        m.insert("compression".into(), c.into());
    }
    if encoding == "base64" {
        m.insert("encoding".into(), "base64".into());
    }
    let chunks: Vec<&Element> = data.children_named("chunk").collect();
    if chunks.is_empty() {
        m.insert("data".into(), cells_json(data, encoding)?);
        return Ok(());
    }
    let mut out = Vec::new();
    for c in chunks {
        let mut chunk = attrs(c);
        chunk.insert("data".into(), cells_json(c, encoding)?);
        out.push(Value::Object(chunk));
    }
    m.insert("chunks".into(), Value::Array(out));
    Ok(())
}

/// Cell data of a `<data>` or `<chunk>` element: a JSON array of ids, or the
/// base64 string.
fn cells_json(el: &Element, encoding: &str) -> Result<Value> {
    match encoding {
        "base64" => Ok(Value::String(el.text.clone())),
        "csv" => {
            let cells = decode_csv(&el.text)?;
            Ok(Value::Array(cells.iter().map(|g| json!(g.0)).collect()))
        }
        "" => {
            let mut cells = Vec::new();
            for t in el.children_named("tile") {
                let gid = t.attr("gid").unwrap_or("0");
                let v: u32 = gid
                    .trim()
                    .parse()
                    .map_err(|_| Error::Data(format!("bad tile gid {gid:?}")))?;
                cells.push(json!(v));
            }
            Ok(Value::Array(cells))
        }
        other => Err(Error::Unsupported(format!("encoding {other:?}"))),
    }
}

/// Converts an `<object>` element.
pub(super) fn object_json(el: &Element) -> Result<Value> {
    let mut m = attrs(el);
    for c in &el.children {
        match c.name.as_str() {
            "properties" => {
                m.insert("properties".into(), properties_json(c)?);
            }
            "ellipse" => {
                m.insert("ellipse".into(), true.into());
            }
            "point" => {
                m.insert("point".into(), true.into());
            }
            "polygon" => {
                m.insert(
                    "polygon".into(),
                    points_json(c.attr("points").unwrap_or(""))?,
                );
            }
            "polyline" => {
                m.insert(
                    "polyline".into(),
                    points_json(c.attr("points").unwrap_or(""))?,
                );
            }
            "text" => {
                let mut t = attrs(c);
                t.insert("text".into(), c.text.clone().into());
                m.insert("text".into(), Value::Object(t));
            }
            _ => {}
        }
    }
    Ok(Value::Object(m))
}

/// Parses `"x,y x,y ..."` into `[{x, y}, ...]`.
fn points_json(text: &str) -> Result<Value> {
    let bad = || Error::Invalid(format!("bad point list {text:?}"));
    let mut out = Vec::new();
    for pair in text.split_whitespace() {
        let (x, y) = pair.split_once(',').ok_or_else(bad)?;
        let x: f64 = x.parse().map_err(|_| bad())?;
        let y: f64 = y.parse().map_err(|_| bad())?;
        out.push(json!({ "x": x, "y": y }));
    }
    Ok(Value::Array(out))
}
