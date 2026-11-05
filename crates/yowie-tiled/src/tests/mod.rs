//! Tests that parse whole files: the fixture maps, every encoding, every
//! orientation, infinite maps, terrain, loading through the VFS, and errors.

mod encodings;
mod errors;
mod fixtures;
mod loading;
mod orientations;
mod terrain;

use yowie_asset::BoxError;

use crate::Map;

const LEVEL_TMX: &[u8] = include_bytes!("../../testdata/level.tmx");
const LEVEL_TMJ: &[u8] = include_bytes!("../../testdata/level.tmj");
const PROPS_TSX: &[u8] = include_bytes!("../../testdata/sets/props.tsx");
const PROPS_TSJ: &[u8] = include_bytes!("../../testdata/sets/props.tsj");

/// A resolver serving the fixture tilesets.
fn fixture_resolver(path: &str) -> Result<Vec<u8>, BoxError> {
    match path {
        "sets/props.tsx" => Ok(PROPS_TSX.to_vec()),
        "sets/props.tsj" => Ok(PROPS_TSJ.to_vec()),
        other => Err(format!("no fixture {other}").into()),
    }
}

fn parse(data: &[u8]) -> Map {
    Map::parse(data, fixture_resolver).expect("fixture parses")
}
