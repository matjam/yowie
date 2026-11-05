//! Parsing TMJ/TSJ JSON (and XML rewritten into the same shape) into the
//! data model.

mod layer;
mod object;
mod props;
mod tileset;
mod util;

use serde_json::Value;
use yowie_core::Vec2;

use self::props::properties;
use self::util::{color, f32_or, flag, string, uint};
use crate::error::{Error, Result};
use crate::map::{Map, Orientation, RenderOrder, StaggerAxis, StaggerIndex};
use crate::tileset::Tileset;
use crate::xml;

/// Fetches the bytes behind a path an external tileset names, relative to
/// the map's directory.
pub(crate) type Resolver<'a> =
    &'a dyn Fn(&str) -> std::result::Result<Vec<u8>, yowie_asset::BoxError>;

/// Reports whether a document is XML (starts with `<` after whitespace and
/// an optional byte order mark).
fn is_xml(data: &[u8]) -> bool {
    let data = data.strip_prefix(b"\xef\xbb\xbf").unwrap_or(data);
    data.iter().find(|b| !b.is_ascii_whitespace()) == Some(&b'<')
}

/// Decodes a document in either format into TMJ/TSJ-shaped JSON.
fn document(data: &[u8]) -> Result<Value> {
    if is_xml(data) {
        xml::to_json(data)
    } else {
        Ok(serde_json::from_slice(data)?)
    }
}

/// Parses a map. `resolve` fetches external tilesets, and may be `None`
/// when every tileset is embedded.
pub(crate) fn parse_map(data: &[u8], resolve: Option<Resolver<'_>>) -> Result<Map> {
    let v = document(data)?;
    let orientation = match string(&v, "orientation") {
        "" | "orthogonal" => Orientation::Orthogonal,
        "isometric" => Orientation::Isometric,
        "staggered" => Orientation::Staggered,
        "hexagonal" => Orientation::Hexagonal,
        other => return Err(Error::Unsupported(format!("orientation {other:?}"))),
    };
    let render_order = match string(&v, "renderorder") {
        "right-up" => RenderOrder::RightUp,
        "left-down" => RenderOrder::LeftDown,
        "left-up" => RenderOrder::LeftUp,
        _ => RenderOrder::RightDown,
    };
    let mut tilesets = Vec::new();
    for (i, t) in util::list(&v, "tilesets").iter().enumerate() {
        let ts = match string(t, "source") {
            "" => tileset::tileset(t, ""),
            src => external(src, uint(t, "firstgid"), resolve),
        };
        tilesets.push(ts.map_err(|e| match e {
            Error::Invalid(m) => Error::Invalid(format!("tileset {i}: {m}")),
            other => other,
        })?);
    }
    tilesets.sort_by_key(|t| t.first_gid);
    Ok(Map {
        orientation,
        render_order,
        width: uint(&v, "width"),
        height: uint(&v, "height"),
        tile_width: uint(&v, "tilewidth"),
        tile_height: uint(&v, "tileheight"),
        infinite: flag(&v, "infinite", false),
        stagger_axis: match string(&v, "staggeraxis") {
            "x" => Some(StaggerAxis::X),
            "y" => Some(StaggerAxis::Y),
            _ => None,
        },
        stagger_index: match string(&v, "staggerindex") {
            "odd" => Some(StaggerIndex::Odd),
            "even" => Some(StaggerIndex::Even),
            _ => None,
        },
        hex_side_length: uint(&v, "hexsidelength"),
        background: color(&v, "backgroundcolor"),
        parallax_origin: Vec2::new(
            f32_or(&v, "parallaxoriginx", 0.0),
            f32_or(&v, "parallaxoriginy", 0.0),
        ),
        class: string(&v, "class").to_string(),
        layers: layer::layers(&v)?,
        tilesets,
        properties: properties(&v),
    })
}

/// Parses a standalone tileset document. `first_gid` is 0 and image paths
/// stay relative to the tileset's own directory.
pub(crate) fn parse_tileset(data: &[u8]) -> Result<Tileset> {
    tileset::tileset(&document(data)?, "")
}

/// Reads an external tileset through the resolver. Its image paths are
/// rebased from the tileset's directory to the map's.
fn external(src: &str, first_gid: u32, resolve: Option<Resolver<'_>>) -> Result<Tileset> {
    let resolve = resolve.ok_or_else(|| Error::NoResolver(src.to_string()))?;
    let data = resolve(src).map_err(|source| Error::Resolve {
        path: src.to_string(),
        source,
    })?;
    let slashed = src.replace('\\', "/");
    let dir = slashed.rsplit_once('/').map_or("", |(d, _)| d);
    let mut ts = tileset::tileset(&document(&data)?, dir)?;
    ts.first_gid = first_gid;
    ts.source = Some(src.to_string());
    Ok(ts)
}
