//! A drawable level built from a Tiled map.

use yowie_asset::Vfs;
use yowie_core::{Color, UVec2, Vec2, uvec2, vec2};
use yowie_tiled::{
    ImageLayer, LayerKind, Map, Object, Orientation, ResolvedLayer, Shape, StaggerAxis,
    StaggerIndex, Tileset,
};

use crate::error::{GfxError, Result};
use crate::sheet::{SheetGrid, SpriteSheet};
use crate::tilemap::{GridLayout, Stagger, Tile, TileAnimation, TileFlip, Tilemap};
use crate::{Graphics, Sprite, Texture, TextureOptions};

/// What a [`LevelLayer`] holds.
#[derive(Clone, Debug)]
pub enum LevelLayerKind {
    /// A tile layer: one [`Tilemap`] for each tileset the layer uses, so
    /// each is one draw call.
    Tiles(Vec<Tilemap>),
    /// An object layer, drawn as outlines by [`TiledLevel::draw_objects`].
    Objects(Vec<Object>),
    /// An image layer.
    Image {
        /// The image, if it loaded.
        texture: Option<Texture>,
        /// The layer's settings.
        layer: ImageLayer,
    },
}

/// One layer of a [`TiledLevel`] with its resolved group state.
#[derive(Clone, Debug)]
pub struct LevelLayer {
    /// The layer's name in Tiled.
    pub name: String,
    /// Whether the layer and all its groups are visible.
    pub visible: bool,
    /// Opacity multiplied through the groups.
    pub opacity: f32,
    /// Offset in pixels, summed through the groups.
    pub offset: Vec2,
    /// Parallax factor: 1 moves with the world, 0 stays on screen.
    pub parallax: Vec2,
    /// What the layer holds.
    pub kind: LevelLayerKind,
}

/// A Tiled map built into drawable layers with its tilesets uploaded.
///
/// [`TiledLevel::load`] reads a map and its tileset images through a
/// [`Vfs`]. Tile layers become [`Tilemap`]s that cull to the camera;
/// animated tiles play when [`advance`](Self::advance) runs; object layers
/// draw as outlines. Orthogonal, isometric and hexagonal maps are
/// supported. Tiled's staggered isometric orientation is an error.
///
/// ```ignore
/// let mut level = TiledLevel::load(&gfx, &vfs, "maps/level.tmx")?;
/// level.advance(dt);
/// level.draw(&mut gfx);
/// level.draw_objects(&mut gfx, 2.0, Color::YELLOW);
/// ```
#[derive(Clone, Debug)]
pub struct TiledLevel {
    map: Map,
    layers: Vec<LevelLayer>,
}

impl TiledLevel {
    /// Loads the map at `path` in `vfs` and uploads its tilesets.
    pub fn load(gfx: &Graphics, vfs: &Vfs, path: &str) -> Result<Self> {
        let map = Map::load(vfs, path).map_err(|e| GfxError::TiledLevel(format!("{path}: {e}")))?;
        let dir =
            yowie_asset::dir_of(&yowie_asset::normalize(path).unwrap_or_default()).to_string();
        Self::build(gfx, vfs, map, &dir)
    }

    /// Builds a level from a parsed map. Tileset and image paths are
    /// relative to `dir`, the directory of the map file in `vfs`.
    pub fn build(gfx: &Graphics, vfs: &Vfs, map: Map, dir: &str) -> Result<Self> {
        let layout = grid_layout(&map)?;
        let mut sheets: Vec<Option<SpriteSheet>> = Vec::with_capacity(map.tilesets.len());
        for ts in &map.tilesets {
            sheets.push(load_sheet(gfx, vfs, dir, ts)?);
        }
        let mut layers = Vec::new();
        for r in map.walk_layers() {
            layers.push(build_layer(gfx, vfs, dir, &map, &layout, &sheets, &r)?);
        }
        Ok(Self { map, layers })
    }

    /// The parsed map.
    pub fn map(&self) -> &Map {
        &self.map
    }

    /// The layers in drawing order.
    pub fn layers(&self) -> &[LevelLayer] {
        &self.layers
    }

    /// The first layer called `name`.
    pub fn layer(&self, name: &str) -> Option<&LevelLayer> {
        self.layers.iter().find(|l| l.name == name)
    }

    /// The map's size in cells.
    pub fn size_in_cells(&self) -> UVec2 {
        uvec2(self.map.width, self.map.height)
    }

    /// The area the map covers in pixels, for clamping a camera.
    pub fn bounds(&self) -> yowie_core::Rect {
        let bounds = grid_layout(&self.map)
            .map(|l| l.map_bounds(self.size_in_cells()))
            .unwrap_or_default();
        match self.map.orientation {
            Orientation::Isometric => bounds.translate(iso_offset(&self.map)),
            _ => bounds,
        }
    }

    /// Advances animated tiles by `dt` seconds.
    pub fn advance(&mut self, dt: f32) {
        for l in &mut self.layers {
            if let LevelLayerKind::Tiles(maps) = &mut l.kind {
                for m in maps {
                    m.advance(dt);
                }
            }
        }
    }

    /// Queues every visible tile and image layer in order, applying each
    /// layer's offset, opacity and parallax. Call it inside the camera
    /// scope; parallax reads the current camera.
    pub fn draw(&self, gfx: &mut Graphics) {
        let cam = gfx.camera().map_or(Vec2::ZERO, |c| c.position);
        for l in self.layers.iter().filter(|l| l.visible) {
            let shift = (cam - self.map.parallax_origin) * (Vec2::ONE - l.parallax);
            match &l.kind {
                LevelLayerKind::Tiles(maps) => {
                    for m in maps {
                        m.draw_shifted(gfx, shift);
                    }
                }
                LevelLayerKind::Image {
                    texture: Some(t), ..
                } => {
                    let tint = Color::WHITE.with_alpha(l.opacity);
                    gfx.draw_sprite(t, &Sprite::at(l.offset + shift).tint(tint));
                }
                _ => {}
            }
        }
    }

    /// Queues the outlines of every visible object: rectangles, ellipses,
    /// polygons, polylines, tile objects as their box, and points as
    /// crosses.
    pub fn draw_objects(&self, gfx: &mut Graphics, thickness: f32, color: Color) {
        for l in self.layers.iter().filter(|l| l.visible) {
            if let LevelLayerKind::Objects(objects) = &l.kind {
                for o in objects.iter().filter(|o| o.visible) {
                    draw_object(gfx, o, l.offset, thickness, color);
                }
            }
        }
    }
}

fn stagger(index: Option<StaggerIndex>) -> Stagger {
    match index {
        Some(StaggerIndex::Even) => Stagger::Even,
        _ => Stagger::Odd,
    }
}

fn grid_layout(map: &Map) -> Result<GridLayout> {
    let cell = vec2(map.tile_width as f32, map.tile_height as f32);
    Ok(match map.orientation {
        Orientation::Orthogonal => GridLayout::square(cell),
        Orientation::Isometric => GridLayout::isometric(cell),
        Orientation::Hexagonal => {
            let side = map.hex_side_length as f32;
            match map.stagger_axis {
                Some(StaggerAxis::X) => GridLayout::hex_flat(cell, stagger(map.stagger_index)),
                _ => GridLayout::hex_pointy(cell, stagger(map.stagger_index)),
            }
            .with_hex_side(side)
        }
        Orientation::Staggered => {
            return Err(GfxError::TiledLevel(
                "staggered isometric maps are not supported".into(),
            ));
        }
    })
}

/// Tiled places an isometric map so no cell has a negative x.
fn iso_offset(map: &Map) -> Vec2 {
    vec2(map.height as f32 * map.tile_width as f32 * 0.5, 0.0)
}

fn load_sheet(gfx: &Graphics, vfs: &Vfs, dir: &str, ts: &Tileset) -> Result<Option<SpriteSheet>> {
    let Some(image) = &ts.image else {
        log::warn!("tileset {:?} has no single image and is skipped", ts.name);
        return Ok(None);
    };
    let path = yowie_asset::join(dir, &image.source)
        .map_err(|e| GfxError::TiledLevel(format!("tileset {:?}: {e}", ts.name)))?;
    let bytes = vfs
        .read(&path)
        .map_err(|e| GfxError::TiledLevel(format!("tileset {:?}: {path}: {e}", ts.name)))?;
    let texture = Texture::from_bytes(gfx, &bytes, TextureOptions::default())?;
    let frame = uvec2(ts.tile_width, ts.tile_height);
    let mut grid = SheetGrid::fit_spaced(texture.size(), frame, ts.margin, ts.spacing);
    if ts.columns > 0 {
        grid.columns = ts.columns;
        grid.rows = ts.tile_count.div_ceil(ts.columns).max(1);
    }
    Ok(Some(SpriteSheet::from_grid(texture, grid)))
}

fn build_layer(
    gfx: &Graphics,
    vfs: &Vfs,
    dir: &str,
    map: &Map,
    layout: &GridLayout,
    sheets: &[Option<SpriteSheet>],
    r: &ResolvedLayer<'_>,
) -> Result<LevelLayer> {
    let base = if map.orientation == Orientation::Isometric {
        iso_offset(map)
    } else {
        Vec2::ZERO
    };
    let kind = match &r.layer.kind {
        LayerKind::Tiles(t) => {
            let mut maps: Vec<Option<Tilemap>> = vec![None; map.tilesets.len()];
            let size = uvec2(t.width, t.height);
            let origin = if map.orientation == Orientation::Orthogonal {
                vec2(
                    t.x as f32 * map.tile_width as f32,
                    t.y as f32 * map.tile_height as f32,
                )
            } else {
                Vec2::ZERO
            };
            let tint = r.tint.unwrap_or(Color::WHITE);
            for (i, gid) in t.dense().into_iter().enumerate() {
                let Some((ts, local)) = map.tileset_for(gid) else {
                    continue;
                };
                let Some(idx) = map
                    .tilesets
                    .iter()
                    .position(|s| s.first_gid == ts.first_gid)
                else {
                    continue;
                };
                let Some(sheet) = sheets[idx].as_ref() else {
                    continue;
                };
                let tm = maps[idx].get_or_insert_with(|| {
                    let mut tm = Tilemap::new(*layout, size, sheet.clone());
                    tm.tile_size = vec2(ts.tile_width as f32, ts.tile_height as f32);
                    tm.offset = base + origin + r.offset + ts.tile_offset;
                    tm.tint = Color::rgba(tint.r, tint.g, tint.b, tint.a * r.opacity);
                    for (id, tile) in &ts.tiles {
                        if tile.animation.len() > 1 {
                            tm.animate(
                                *id,
                                TileAnimation {
                                    frames: tile.animation.iter().map(|f| f.tile_id).collect(),
                                    durations: tile.animation.iter().map(|f| f.duration).collect(),
                                },
                            );
                        }
                    }
                    tm
                });
                let f = gid.flags();
                let w = size.x as usize;
                tm.set(
                    (i % w) as i32,
                    (i / w) as i32,
                    Tile::new(local).flipped(TileFlip {
                        horizontal: f.horizontal,
                        vertical: f.vertical,
                        diagonal: f.diagonal,
                        rotate_120: f.rotate_120,
                    }),
                );
            }
            LevelLayerKind::Tiles(maps.into_iter().flatten().collect())
        }
        LayerKind::Objects(o) => LevelLayerKind::Objects(o.objects.clone()),
        LayerKind::Image(img) => {
            let texture = if img.image.is_empty() {
                None
            } else {
                let path = yowie_asset::join(dir, &img.image)
                    .map_err(|e| GfxError::TiledLevel(e.to_string()))?;
                match vfs.read(&path) {
                    Ok(bytes) => Some(Texture::from_bytes(gfx, &bytes, TextureOptions::default())?),
                    Err(e) => {
                        log::warn!("image layer {:?}: {path}: {e}", r.layer.name);
                        None
                    }
                }
            };
            LevelLayerKind::Image {
                texture,
                layer: img.clone(),
            }
        }
        LayerKind::Group(_) => LevelLayerKind::Objects(Vec::new()),
    };
    Ok(LevelLayer {
        name: r.layer.name.clone(),
        visible: r.visible,
        opacity: r.opacity,
        offset: r.offset + base,
        parallax: r.parallax,
        kind,
    })
}

fn draw_object(gfx: &mut Graphics, o: &Object, layer_offset: Vec2, thickness: f32, color: Color) {
    let origin = vec2(o.x, o.y) + layer_offset;
    let rot = Vec2::from_angle(o.rotation.to_radians());
    let place = |p: Vec2| origin + rot.rotate(p);
    let size = vec2(o.width, o.height);
    match &o.shape {
        Shape::Point => {
            let r = 4.0;
            gfx.line(place(vec2(-r, 0.0)), place(vec2(r, 0.0)), thickness, color);
            gfx.line(place(vec2(0.0, -r)), place(vec2(0.0, r)), thickness, color);
        }
        Shape::Polygon(pts) => {
            let pts: Vec<Vec2> = pts.iter().map(|p| place(*p)).collect();
            gfx.stroke_polygon(&pts, thickness, color);
        }
        Shape::Polyline(pts) => {
            let pts: Vec<Vec2> = pts.iter().map(|p| place(*p)).collect();
            gfx.stroke_polyline(&pts, thickness, color);
        }
        Shape::Ellipse => {
            let pts: Vec<Vec2> = (0..32)
                .map(|i| {
                    let a = i as f32 / 32.0 * std::f32::consts::TAU;
                    place((vec2(a.cos(), a.sin()) * 0.5 + Vec2::splat(0.5)) * size)
                })
                .collect();
            gfx.stroke_polygon(&pts, thickness, color);
        }
        Shape::Tile(_) => {
            // A tile object's origin is its bottom-left corner.
            let pts = [
                vec2(0.0, -size.y),
                vec2(size.x, -size.y),
                vec2(size.x, 0.0),
                vec2(0.0, 0.0),
            ]
            .map(place);
            gfx.stroke_polygon(&pts, thickness, color);
        }
        Shape::Rectangle | Shape::Text(_) => {
            let pts = [vec2(0.0, 0.0), vec2(size.x, 0.0), size, vec2(0.0, size.y)].map(place);
            gfx.stroke_polygon(&pts, thickness, color);
        }
    }
}
