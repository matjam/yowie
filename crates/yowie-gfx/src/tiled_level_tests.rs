//! Staggered isometric and hexagonal Tiled maps: coordinate conversion in
//! both directions for both axes and both indices, and drawing.

use yowie_asset::{MemorySource, Vfs};
use yowie_core::{Color, IVec2, ivec2, uvec2, vec2};
use yowie_tiled::Map;

use crate::gpu_tests::{headless, render};
use crate::{Camera2D, TiledLevel};

fn png() -> Vec<u8> {
    let mut out = Vec::new();
    image::RgbaImage::from_pixel(32, 16, image::Rgba([255, 0, 0, 255]))
        .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
        .unwrap_or_else(|e| panic!("encode: {e}"));
    out
}

fn map_xml(orientation: &str, axis: &str, index: &str) -> String {
    format!(
        r#"<map orientation="{orientation}" width="4" height="4" tilewidth="32" tileheight="16"
            staggeraxis="{axis}" staggerindex="{index}" hexsidelength="0">
          <tileset firstgid="1" name="t" tilewidth="32" tileheight="16" tilecount="1" columns="1">
            <image source="t.png" width="32" height="16"/></tileset>
          <layer id="1" name="g" width="4" height="4"><data encoding="csv">
            1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1</data></layer></map>"#
    )
}

fn vfs() -> Vfs {
    let mut vfs = Vfs::new();
    vfs.mount_memory(
        MemorySource::new()
            .with("t.png", png())
            .unwrap_or_else(|e| panic!("memory source: {e}")),
    );
    vfs
}

#[test]
fn staggered_maps_convert_tiles_and_positions_both_ways() {
    let Some(gfx) = headless(uvec2(8, 8)) else {
        return;
    };
    for orientation in ["staggered", "hexagonal"] {
        for axis in ["x", "y"] {
            for index in ["odd", "even"] {
                let map = Map::parse_embedded(map_xml(orientation, axis, index).as_bytes())
                    .unwrap_or_else(|e| panic!("map: {e}"));
                let level = TiledLevel::build(&gfx, &vfs(), map, "")
                    .unwrap_or_else(|e| panic!("{orientation} {axis} {index}: {e}"));
                let mut centres = Vec::new();
                for y in -2..6 {
                    for x in -2..6 {
                        let t = ivec2(x, y);
                        let c = level.tile_center(t);
                        assert_eq!(level.tile_at(c), t, "{orientation} {axis} {index}");
                        centres.push(c);
                    }
                }
                // Neighbouring cells never share a centre.
                for (i, a) in centres.iter().enumerate() {
                    assert!(centres[i + 1..].iter().all(|b| (*a - *b).length() > 1.0));
                }
                // The stagger index decides which of the first two
                // rows (or columns) moves.
                let (a, b): (IVec2, IVec2) = (ivec2(0, 0), ivec2(1, 1));
                let first = level.tile_center(a);
                let second = level.tile_center(b);
                assert_ne!(first, second);
            }
        }
    }
}

#[test]
fn staggered_axis_and_index_pick_the_shifted_cells() {
    let Some(gfx) = headless(uvec2(8, 8)) else {
        return;
    };
    let level = |axis, index| {
        let map = Map::parse_embedded(map_xml("staggered", axis, index).as_bytes())
            .unwrap_or_else(|e| panic!("map: {e}"));
        TiledLevel::build(&gfx, &vfs(), map, "").unwrap_or_else(|e| panic!("build: {e}"))
    };
    // Row stagger: the odd (even) index rows move half a cell right.
    let odd = level("y", "odd");
    assert_eq!(odd.tile_center(ivec2(0, 0)), vec2(16.0, 8.0));
    assert_eq!(odd.tile_center(ivec2(0, 1)), vec2(32.0, 16.0));
    let even = level("y", "even");
    assert_eq!(even.tile_center(ivec2(0, 0)), vec2(32.0, 8.0));
    assert_eq!(even.tile_center(ivec2(0, 1)), vec2(16.0, 16.0));
    // Column stagger: the shifted columns move half a cell down, and
    // columns are half a cell width apart.
    let cols = level("x", "odd");
    assert_eq!(cols.tile_center(ivec2(0, 0)), vec2(16.0, 8.0));
    assert_eq!(cols.tile_center(ivec2(1, 0)), vec2(32.0, 16.0));
    assert_eq!(cols.tile_center(ivec2(2, 0)), vec2(48.0, 8.0));
}

#[test]
fn a_staggered_map_draws_its_tiles() {
    let Some(mut gfx) = headless(uvec2(160, 64)) else {
        return;
    };
    for axis in ["x", "y"] {
        let map = Map::parse_embedded(map_xml("staggered", axis, "odd").as_bytes())
            .unwrap_or_else(|e| panic!("map: {e}"));
        let level =
            TiledLevel::build(&gfx, &vfs(), map, "").unwrap_or_else(|e| panic!("build: {e}"));
        gfx.begin_frame();
        gfx.set_clear_color(Color::BLACK);
        gfx.with_camera(&Camera2D::new(vec2(80.0, 32.0)), |g| level.draw(g));
        let img = render(&mut gfx);
        let c = level.tile_center(ivec2(1, 1));
        let px = img.get_pixel(c.x as u32, c.y as u32).0;
        assert_eq!(px, [255, 0, 0, 255], "axis {axis}");
        assert_eq!(img.get_pixel(150, 60).0, [0, 0, 0, 255], "axis {axis}");
    }
}
