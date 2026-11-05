//! TMX and TSX support: XML is rewritten into the JSON shape of TMJ and TSJ
//! so one parser reads both.

mod convert;
mod convert_layer;
mod dom;

use serde_json::Value;

use crate::error::{Error, Result};

/// Parses an XML map or tileset document into TMJ/TSJ-shaped JSON.
pub(crate) fn to_json(data: &[u8]) -> Result<Value> {
    let root = dom::parse(data)?;
    match root.name.as_str() {
        "map" => convert::map_json(&root),
        "tileset" => convert::tileset_json(&root),
        "template" => convert::template_json(&root),
        other => Err(Error::Invalid(format!("unexpected root element <{other}>"))),
    }
}
