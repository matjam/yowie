//! Entry points: parsing from memory, loading through a resolver or the
//! asset file system, and the asset loader.

use yowie_asset::{BoxError, LoadContext, Loader, Vfs};

use crate::error::{Error, Result};
use crate::map::Map;
use crate::parse::{parse_map, parse_tileset};
use crate::tileset::Tileset;

impl Map {
    /// Parses a map from memory. The format (TMX XML or TMJ JSON) is
    /// detected from the first non-blank byte.
    ///
    /// `resolve` fetches an external tileset by the path the map names,
    /// which is relative to the map's directory; the tileset may be in
    /// either format whatever the map's. Paths in the result (tileset
    /// images, image layers) stay relative to the map's directory.
    ///
    /// ```
    /// # use yowie_tiled::Map;
    /// let json = br#"{"width":1,"height":1,"tilewidth":8,"tileheight":8,
    ///   "layers":[{"type":"tilelayer","name":"g","width":1,"height":1,"data":[0]}]}"#;
    /// let map = Map::parse(json, |_path| Err("no files".into()))?;
    /// assert_eq!(map.layers.len(), 1);
    /// # Ok::<(), yowie_tiled::Error>(())
    /// ```
    pub fn parse<F>(data: &[u8], resolve: F) -> Result<Map>
    where
        F: Fn(&str) -> std::result::Result<Vec<u8>, BoxError>,
    {
        parse_map(data, Some(&resolve))
    }

    /// Parses a map whose tilesets are all embedded. An external tileset is
    /// an [`Error::NoResolver`].
    pub fn parse_embedded(data: &[u8]) -> Result<Map> {
        parse_map(data, None)
    }

    /// Loads the map at `path` in `vfs`, resolving external tilesets
    /// relative to the map's directory.
    pub fn load(vfs: &Vfs, path: &str) -> Result<Map> {
        let path = yowie_asset::normalize(path)?;
        let data = vfs.read(&path)?;
        let dir = yowie_asset::dir_of(&path).to_string();
        parse_map(
            &data,
            Some(&|p: &str| Ok(vfs.read(&yowie_asset::join(&dir, p)?)?)),
        )
    }
}

impl Tileset {
    /// Parses a standalone tileset (TSX or TSJ). `first_gid` is 0 and image
    /// paths stay relative to the tileset's own directory.
    pub fn parse(data: &[u8]) -> Result<Tileset> {
        parse_tileset(data)
    }

    /// Loads the standalone tileset at `path` in `vfs`.
    pub fn load(vfs: &Vfs, path: &str) -> Result<Tileset> {
        parse_tileset(&vfs.read(path)?)
    }
}

/// Loads [`Map`]s through [`yowie_asset::Assets`].
///
/// External tilesets are read through the loader context, so editing a
/// tileset file reloads every map that uses it when hot reload is enabled.
///
/// ```
/// # use yowie_asset::{Assets, MemorySource, Vfs};
/// # use yowie_tiled::{Map, MapLoader};
/// let json = br#"{"width":1,"height":1,"tilewidth":8,"tileheight":8,"layers":[]}"#;
/// let mut vfs = Vfs::new();
/// vfs.mount_memory(MemorySource::new().with("maps/a.tmj", &json[..])?);
/// let assets = Assets::new(vfs, 1)?;
/// assets.register(MapLoader);
/// let map = assets.load_now::<Map>("maps/a.tmj")?;
/// assert_eq!(map.get().map(|m| m.tile_width), Some(8));
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Clone, Copy, Debug, Default)]
pub struct MapLoader;

impl Loader for MapLoader {
    type Asset = Map;

    fn load(
        &self,
        bytes: &[u8],
        _path: &str,
        ctx: &LoadContext<'_>,
    ) -> std::result::Result<Map, BoxError> {
        let resolve =
            |p: &str| -> std::result::Result<Vec<u8>, BoxError> { Ok(ctx.read_relative(p)?) };
        parse_map(bytes, Some(&resolve)).map_err(|e: Error| Box::new(e) as BoxError)
    }
}
