//! Rewrites XML elements into the JSON shape Tiled's TMJ/TSJ files use, so
//! one parser reads both formats. Attribute values stay strings; the JSON
//! parser accepts numbers and booleans written as text.

use serde_json::{Map, Value, json};

use super::convert_layer::{layer_json, object_json};
use super::dom::Element;
use crate::error::{Error, Result};

/// All attributes of `el` as a JSON object of strings.
pub(super) fn attrs(el: &Element) -> Map<String, Value> {
    el.attrs
        .iter()
        .map(|(k, v)| (k.clone(), Value::String(v.clone())))
        .collect()
}

/// Converts a `<map>` element.
pub(super) fn map_json(el: &Element) -> Result<Value> {
    let mut m = attrs(el);
    let mut tilesets = Vec::new();
    let mut layers = Vec::new();
    for c in &el.children {
        match c.name.as_str() {
            "properties" => {
                m.insert("properties".into(), properties_json(c)?);
            }
            "tileset" => tilesets.push(tileset_json(c)?),
            "layer" | "objectgroup" | "imagelayer" | "group" => layers.push(layer_json(c)?),
            _ => {}
        }
    }
    m.insert("tilesets".into(), Value::Array(tilesets));
    m.insert("layers".into(), Value::Array(layers));
    Ok(Value::Object(m))
}

/// Converts a `<properties>` element into an array of property objects.
pub(super) fn properties_json(el: &Element) -> Result<Value> {
    let mut out = Vec::new();
    for p in el.children_named("property") {
        let kind = p.attr("type").unwrap_or("string");
        let mut obj = Map::new();
        obj.insert("name".into(), p.attr("name").unwrap_or_default().into());
        obj.insert("type".into(), kind.into());
        if let Some(t) = p.attr("propertytype") {
            obj.insert("propertytype".into(), t.into());
        }
        let value = match (kind, p.child("properties")) {
            ("class", members) => class_value(members)?,
            (_, _) => typed_value(kind, p.attr("value").unwrap_or(&p.text)),
        };
        obj.insert("value".into(), value);
        out.push(Value::Object(obj));
    }
    Ok(Value::Array(out))
}

/// A class property's members as a JSON object with typed values.
fn class_value(members: Option<&Element>) -> Result<Value> {
    let mut out = Map::new();
    if let Some(list) = members {
        let Value::Array(items) = properties_json(list)? else {
            return Ok(Value::Object(out));
        };
        for item in items {
            if let (Some(name), Some(value)) = (item["name"].as_str(), item.get("value")) {
                out.insert(name.to_string(), value.clone());
            }
        }
    }
    Ok(Value::Object(out))
}

/// Converts attribute text to the JSON type the property kind uses. Text
/// that does not parse stays a string, which the JSON parser then drops.
fn typed_value(kind: &str, text: &str) -> Value {
    match kind {
        "int" | "object" => text
            .trim()
            .parse::<i64>()
            .map_or_else(|_| text.into(), Value::from),
        "float" => text
            .trim()
            .parse::<f64>()
            .map_or_else(|_| text.into(), Value::from),
        "bool" => match text.trim() {
            "true" => Value::Bool(true),
            "false" => Value::Bool(false),
            _ => text.into(),
        },
        _ => text.into(),
    }
}

/// Converts an `<image>` element's fields into `target` under the keys
/// TMJ uses.
pub(super) fn image_into(target: &mut Map<String, Value>, img: &Element) {
    let copy = |t: &mut Map<String, Value>, from: &str, to: &str| {
        if let Some(v) = img.attr(from) {
            t.insert(to.into(), v.into());
        }
    };
    copy(target, "source", "image");
    copy(target, "width", "imagewidth");
    copy(target, "height", "imageheight");
    copy(target, "trans", "transparentcolor");
}

/// Converts a `<tileset>` element.
pub(super) fn tileset_json(el: &Element) -> Result<Value> {
    let mut m = attrs(el);
    let mut tiles = Vec::new();
    let mut wangsets = Vec::new();
    let mut terrains = Vec::new();
    for c in &el.children {
        match c.name.as_str() {
            "image" => image_into(&mut m, c),
            "properties" => {
                m.insert("properties".into(), properties_json(c)?);
            }
            "tileoffset" => {
                m.insert("tileoffset".into(), Value::Object(attrs(c)));
            }
            "tile" => tiles.push(tile_json(c)?),
            "terraintypes" => {
                for t in c.children_named("terrain") {
                    terrains.push(Value::Object(attrs(t)));
                }
            }
            "wangsets" => {
                for w in c.children_named("wangset") {
                    wangsets.push(wangset_json(w)?);
                }
            }
            _ => {}
        }
    }
    m.insert("tiles".into(), Value::Array(tiles));
    m.insert("wangsets".into(), Value::Array(wangsets));
    m.insert("terrains".into(), Value::Array(terrains));
    Ok(Value::Object(m))
}

fn tile_json(el: &Element) -> Result<Value> {
    let mut m = attrs(el);
    if let Some(t) = el.attr("terrain") {
        let ids: Vec<Value> = t
            .split(',')
            .map(|s| s.trim().parse::<i64>().unwrap_or(-1).into())
            .collect();
        m.insert("terrain".into(), Value::Array(ids));
    }
    for c in &el.children {
        match c.name.as_str() {
            "image" => image_into(&mut m, c),
            "properties" => {
                m.insert("properties".into(), properties_json(c)?);
            }
            "animation" => {
                let frames: Vec<Value> = c
                    .children_named("frame")
                    .map(|f| Value::Object(attrs(f)))
                    .collect();
                m.insert("animation".into(), Value::Array(frames));
            }
            "objectgroup" => {
                let objects = c
                    .children_named("object")
                    .map(object_json)
                    .collect::<Result<Vec<_>>>()?;
                m.insert("objectgroup".into(), json!({ "objects": objects }));
            }
            _ => {}
        }
    }
    Ok(Value::Object(m))
}

fn wangset_json(el: &Element) -> Result<Value> {
    let mut m = attrs(el);
    let mut colors = Vec::new();
    let mut wangtiles = Vec::new();
    for c in &el.children {
        match c.name.as_str() {
            "wangcolor" | "wangcornercolor" | "wangedgecolor" => {
                let mut color = attrs(c);
                if let Some(p) = c.child("properties") {
                    color.insert("properties".into(), properties_json(p)?);
                }
                colors.push(Value::Object(color));
            }
            "wangtile" => {
                let mut t = attrs(c);
                if let Some(id) = c.attr("wangid") {
                    let ids = id
                        .split(',')
                        .map(|s| {
                            s.trim().parse::<i64>().map(Value::from).map_err(|_| {
                                Error::Invalid(format!("wangset: bad wangid value {:?}", s.trim()))
                            })
                        })
                        .collect::<Result<Vec<_>>>()?;
                    t.insert("wangid".into(), Value::Array(ids));
                }
                wangtiles.push(Value::Object(t));
            }
            "properties" => {
                m.insert("properties".into(), properties_json(c)?);
            }
            _ => {}
        }
    }
    m.insert("colors".into(), Value::Array(colors));
    m.insert("wangtiles".into(), Value::Array(wangtiles));
    Ok(Value::Object(m))
}
