//! Object templates (TX and TJ files), applied while the map is read.
//!
//! An object that names a template takes the template object's fields; the
//! fields the object itself states win, and properties merge by name. A
//! tile object's gid in the template counts in the template's tileset, so
//! it is moved to where the map keeps that tileset (the tileset is added to
//! the map when the map does not list it).

use std::collections::HashMap;

use serde_json::{Map, Value};

use super::util::{list, string, uint};
use super::{Resolver, document, external, tileset};
use crate::error::{Error, Result, invalid};
use crate::tileset::Tileset;

/// Flag bits in the top of a gid: flips, diagonal flip, hexagonal rotation.
const FLAGS: u32 = 0xF000_0000;

/// A template file after reading.
struct Template {
    object: Value,
    /// Where the template's tile ids start in the template's tileset, and
    /// where the map keeps that tileset. `None` for templates without a
    /// tile.
    gids: Option<(u32, u32)>,
}

struct Context<'a, 'r> {
    resolve: Option<Resolver<'r>>,
    tilesets: &'a mut Vec<Tileset>,
    cache: HashMap<String, Template>,
}

/// Replaces every template instance in the map's object layers with the
/// merged object. `tilesets` may gain the tilesets the templates use.
pub(super) fn apply(
    map: &mut Value,
    tilesets: &mut Vec<Tileset>,
    resolve: Option<Resolver<'_>>,
) -> Result<()> {
    let mut ctx = Context {
        resolve,
        tilesets,
        cache: HashMap::new(),
    };
    walk(map, &mut ctx)
}

/// Visits the layers of `v` (a map or a group), depth first.
fn walk(v: &mut Value, ctx: &mut Context<'_, '_>) -> Result<()> {
    let Some(layers) = v.get_mut("layers").and_then(Value::as_array_mut) else {
        return Ok(());
    };
    for layer in layers {
        match string(layer, "type") {
            "group" => walk(layer, ctx)?,
            "objectgroup" => {
                let Some(objects) = layer.get_mut("objects").and_then(Value::as_array_mut) else {
                    continue;
                };
                for object in objects {
                    let path = string(object, "template").to_string();
                    if !path.is_empty() {
                        *object = instance(object, &path, ctx)?;
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn instance(object: &Value, path: &str, ctx: &mut Context<'_, '_>) -> Result<Value> {
    if !ctx.cache.contains_key(path) {
        let template = load(path, ctx)?;
        ctx.cache.insert(path.to_string(), template);
    }
    let template = &ctx.cache[path];
    Ok(merge(&template.object, object, template.gids))
}

/// Reads a template and registers its tileset with the map.
fn load(path: &str, ctx: &mut Context<'_, '_>) -> Result<Template> {
    let resolve = ctx
        .resolve
        .ok_or_else(|| Error::NoResolver(path.to_string()))?;
    let data = resolve(path).map_err(|source| Error::Resolve {
        path: path.to_string(),
        source,
    })?;
    let doc = document(&data)?;
    let object = match doc.get("object") {
        Some(o) if o.is_object() => o.clone(),
        _ => return invalid(format!("template {path:?} has no object")),
    };
    let dir = yowie_asset::dir_of(&path.replace('\\', "/")).to_string();
    let gids = match doc.get("tileset") {
        Some(ts) if object.get("gid").is_some() => Some(register_tileset(ts, &dir, ctx)?),
        _ => None,
    };
    Ok(Template { object, gids })
}

/// Finds or adds the template's tileset in the map and returns its first
/// gid in the template and in the map.
fn register_tileset(ts: &Value, dir: &str, ctx: &mut Context<'_, '_>) -> Result<(u32, u32)> {
    let template_first = uint(ts, "firstgid");
    let source = string(ts, "source");
    let wanted = if source.is_empty() {
        None
    } else {
        Some(yowie_asset::join(dir, source)?)
    };
    if let Some(wanted) = &wanted {
        let found = ctx.tilesets.iter().find(|t| {
            t.source
                .as_deref()
                .and_then(|s| yowie_asset::normalize(s).ok())
                .as_deref()
                == Some(wanted)
        });
        if let Some(t) = found {
            return Ok((template_first, t.first_gid));
        }
    }
    let next = ctx
        .tilesets
        .iter()
        .map(|t| t.first_gid + t.tile_count.max(1))
        .max()
        .unwrap_or(1);
    let added = match &wanted {
        Some(path) => external(path, next, ctx.resolve)?,
        None => {
            let mut t = tileset::tileset(ts, dir)?;
            t.first_gid = next;
            t
        }
    };
    ctx.tilesets.push(added);
    Ok((template_first, next))
}

/// The template's fields under the instance's, with the gid moved into the
/// map's numbering.
fn merge(template: &Value, instance: &Value, gids: Option<(u32, u32)>) -> Value {
    let mut out: Map<String, Value> = template.as_object().cloned().unwrap_or_default();
    if instance.get("gid").is_none()
        && let (Some((from, to)), Some(gid)) = (gids, out.get("gid"))
    {
        let raw = super::util::as_f64(gid).map_or(0, |g| g as u32);
        let id = raw & !FLAGS;
        if id >= from {
            out.insert("gid".into(), Value::from((raw & FLAGS) | (id - from + to)));
        }
    }
    let mut properties = list(template, "properties").to_vec();
    if let Some(fields) = instance.as_object() {
        for (key, value) in fields {
            if key != "properties" {
                out.insert(key.clone(), value.clone());
            }
        }
    }
    for p in list(instance, "properties") {
        let name = string(p, "name");
        match properties.iter_mut().find(|q| string(q, "name") == name) {
            Some(slot) => *slot = p.clone(),
            None => properties.push(p.clone()),
        }
    }
    if !properties.is_empty() {
        out.insert("properties".into(), Value::Array(properties));
    }
    Value::Object(out)
}
