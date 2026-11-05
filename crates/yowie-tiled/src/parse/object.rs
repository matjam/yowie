//! Objects from JSON.

use serde_json::Value;
use yowie_core::{Color, Vec2};

use super::props::properties;
use super::util::{color, f32_or, flag, list, num, string, string_any, uint};
use crate::gid::Gid;
use crate::object::{HAlign, Object, Shape, Text, VAlign};

/// Reads an `objects` array.
pub(super) fn objects(v: &Value) -> Vec<Object> {
    list(v, "objects").iter().map(object).collect()
}

fn object(v: &Value) -> Object {
    Object {
        id: uint(v, "id"),
        name: string(v, "name").to_string(),
        // `type` is the field's name before Tiled 1.9.
        class: string_any(v, &["class", "type"]).to_string(),
        x: f32_or(v, "x", 0.0),
        y: f32_or(v, "y", 0.0),
        width: f32_or(v, "width", 0.0),
        height: f32_or(v, "height", 0.0),
        rotation: f32_or(v, "rotation", 0.0),
        visible: flag(v, "visible", true),
        shape: shape(v),
        properties: properties(v),
    }
}

fn shape(v: &Value) -> Shape {
    if flag(v, "point", false) {
        Shape::Point
    } else if flag(v, "ellipse", false) {
        Shape::Ellipse
    } else if v.get("polygon").is_some() {
        Shape::Polygon(points(v, "polygon"))
    } else if v.get("polyline").is_some() {
        Shape::Polyline(points(v, "polyline"))
    } else if let Some(t) = v.get("text").filter(|t| t.is_object()) {
        Shape::Text(text(t))
    } else if num(v, "gid").is_some_and(|g| g > 0.0) {
        Shape::Tile(Gid(num(v, "gid").unwrap_or(0.0) as u32))
    } else {
        Shape::Rectangle
    }
}

fn points(v: &Value, key: &str) -> Vec<Vec2> {
    list(v, key)
        .iter()
        .map(|p| Vec2::new(f32_or(p, "x", 0.0), f32_or(p, "y", 0.0)))
        .collect()
}

fn text(t: &Value) -> Text {
    let d = Text::default();
    Text {
        text: string(t, "text").to_string(),
        font_family: match string(t, "fontfamily") {
            "" => d.font_family,
            f => f.to_string(),
        },
        pixel_size: f32_or(t, "pixelsize", d.pixel_size),
        wrap: flag(t, "wrap", false),
        color: color(t, "color").unwrap_or(Color::BLACK),
        bold: flag(t, "bold", false),
        italic: flag(t, "italic", false),
        h_align: match string(t, "halign") {
            "center" => HAlign::Center,
            "right" => HAlign::Right,
            "justify" => HAlign::Justify,
            _ => HAlign::Left,
        },
        v_align: match string(t, "valign") {
            "center" => VAlign::Center,
            "bottom" => VAlign::Bottom,
            _ => VAlign::Top,
        },
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn shapes_table() {
        let v = json!({"objects": [
            {"id": 1, "x": 1, "y": 2, "width": 3, "height": 4},
            {"id": 2, "ellipse": true, "width": 8, "height": 4},
            {"id": 3, "point": true},
            {"id": 4, "polygon": [{"x": 0, "y": 0}, {"x": 5, "y": 0}, {"x": 0, "y": 5}]},
            {"id": 5, "polyline": [{"x": 0, "y": 0}, {"x": 1, "y": 1}]},
            {"id": 6, "gid": 2147483650u32, "rotation": 45, "type": "crate"},
            {"id": 7, "visible": false, "class": "sign",
             "text": {"text": "hello", "wrap": true, "halign": "center", "pixelsize": 12, "color": "#ff0000"}}
        ]});
        let objs = objects(&v);
        assert_eq!(objs.len(), 7);
        assert_eq!(objs[0].shape, Shape::Rectangle);
        assert_eq!(objs[0].rect().w, 3.0);
        assert_eq!(objs[1].shape, Shape::Ellipse);
        assert_eq!(objs[2].shape, Shape::Point);
        assert_eq!(
            objs[3].shape,
            Shape::Polygon(vec![Vec2::ZERO, Vec2::new(5.0, 0.0), Vec2::new(0.0, 5.0)])
        );
        assert!(matches!(&objs[4].shape, Shape::Polyline(p) if p.len() == 2));
        assert_eq!(objs[5].shape, Shape::Tile(Gid(0x8000_0002)));
        assert_eq!((objs[5].class.as_str(), objs[5].rotation), ("crate", 45.0));
        let Shape::Text(t) = &objs[6].shape else {
            panic!("not text")
        };
        assert_eq!(
            (t.text.as_str(), t.wrap, t.h_align, t.pixel_size),
            ("hello", true, HAlign::Center, 12.0)
        );
        assert_eq!(t.color, Color::rgb8(255, 0, 0));
        assert!(!objs[6].visible);
    }
}
