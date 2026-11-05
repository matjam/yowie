//! Orientation fields and the remaining layer kinds, in both formats.

use yowie_core::Color;

use super::*;
use crate::{DrawOrder, Error, LayerKind, Orientation, RenderOrder, StaggerAxis, StaggerIndex};

#[test]
fn orientations_parse_alike_in_both_formats() {
    let cases = [
        ("orthogonal", "", "", 0, Orientation::Orthogonal, None, None),
        ("isometric", "", "", 0, Orientation::Isometric, None, None),
        (
            "staggered",
            "y",
            "odd",
            0,
            Orientation::Staggered,
            Some(StaggerAxis::Y),
            Some(StaggerIndex::Odd),
        ),
        (
            "hexagonal",
            "x",
            "even",
            12,
            Orientation::Hexagonal,
            Some(StaggerAxis::X),
            Some(StaggerIndex::Even),
        ),
    ];
    for (name, axis, index, side, orientation, want_axis, want_index) in cases {
        let extra_x =
            format!(r#" staggeraxis="{axis}" staggerindex="{index}" hexsidelength="{side}""#);
        let extra_j =
            format!(r#","staggeraxis":"{axis}","staggerindex":"{index}","hexsidelength":{side}"#);
        let (extra_x, extra_j) = if axis.is_empty() {
            (String::new(), String::new())
        } else {
            (extra_x, extra_j)
        };
        let xml = format!(
            r#"<map orientation="{name}" renderorder="left-up" width="3" height="3" tilewidth="32" tileheight="16"{extra_x}/>"#
        );
        let json = format!(
            r#"{{"orientation":"{name}","renderorder":"left-up","width":3,"height":3,"tilewidth":32,"tileheight":16{extra_j}}}"#
        );
        let (x, j) = (
            Map::parse_embedded(xml.as_bytes()).unwrap(),
            Map::parse_embedded(json.as_bytes()).unwrap(),
        );
        assert_eq!(x, j, "{name}");
        assert_eq!(x.orientation, orientation, "{name}");
        assert_eq!(
            (x.stagger_axis, x.stagger_index, x.hex_side_length),
            (want_axis, want_index, side),
            "{name}"
        );
        assert_eq!(x.render_order, RenderOrder::LeftUp);
        assert_eq!((x.tile_width, x.tile_height), (32, 16));
    }
}

#[test]
fn unknown_orientation_is_unsupported() {
    let err = Map::parse_embedded(br#"{"orientation":"cubic"}"#).unwrap_err();
    assert!(matches!(err, Error::Unsupported(_)), "{err}");
}

#[test]
fn image_and_object_group_attributes() {
    let xml = r##"<map width="1" height="1" tilewidth="8" tileheight="8">
        <imagelayer id="1" name="sky" repeatx="1" offsetx="3" parallaxx="0.25" parallaxy="0.5">
          <image source="img/sky.png" width="64" height="32" trans="#ff00ff"/>
        </imagelayer>
        <objectgroup id="2" name="o" draworder="index" color="#a0b0c0" opacity="0.5" locked="1"/>
      </map>"##;
    let json = r##"{"width":1,"height":1,"tilewidth":8,"tileheight":8,"layers":[
        {"type":"imagelayer","id":1,"name":"sky","image":"img/sky.png","transparentcolor":"#ff00ff",
         "repeatx":true,"offsetx":3,"parallaxx":0.25,"parallaxy":0.5},
        {"type":"objectgroup","id":2,"name":"o","draworder":"index","color":"#a0b0c0","opacity":0.5,"objects":[]}]}"##;
    let (x, j) = (
        Map::parse_embedded(xml.as_bytes()).unwrap(),
        Map::parse_embedded(json.as_bytes()).unwrap(),
    );
    assert_eq!(x, j);
    let LayerKind::Image(img) = &x.layers[0].kind else {
        panic!("not an image layer")
    };
    assert_eq!(img.image, "img/sky.png");
    assert_eq!(img.transparent_color, Some(Color::rgb8(255, 0, 255)));
    assert!(img.repeat_x && !img.repeat_y);
    assert_eq!(x.layers[0].offset.x, 3.0);
    assert_eq!(
        (x.layers[0].parallax.x, x.layers[0].parallax.y),
        (0.25, 0.5)
    );
    let LayerKind::Objects(o) = &x.layers[1].kind else {
        panic!("not objects")
    };
    assert_eq!(o.draw_order, DrawOrder::Index);
    assert_eq!(o.color, Some(Color::rgb8(0xa0, 0xb0, 0xc0)));
    assert_eq!(x.layers[1].opacity, 0.5);
}

#[test]
fn text_objects_resolve_entities_and_styles() {
    let xml = r##"<map width="1" height="1" tilewidth="8" tileheight="8"><objectgroup id="1" name="t">
        <object id="1" x="0" y="0" width="40" height="20">
          <text fontfamily="Mono" pixelsize="12" color="#ff0000" bold="1" halign="right" valign="bottom">a &lt; b &amp; c</text>
        </object></objectgroup></map>"##;
    let m = Map::parse_embedded(xml.as_bytes()).unwrap();
    let LayerKind::Objects(o) = &m.layers[0].kind else {
        panic!("not objects")
    };
    let crate::Shape::Text(t) = &o.objects[0].shape else {
        panic!("not text")
    };
    assert_eq!(t.text, "a < b & c");
    assert_eq!(
        (t.font_family.as_str(), t.pixel_size, t.bold),
        ("Mono", 12.0, true)
    );
    assert_eq!(
        (t.h_align, t.v_align),
        (crate::HAlign::Right, crate::VAlign::Bottom)
    );
    assert_eq!(t.color, Color::rgb8(255, 0, 0));
}
