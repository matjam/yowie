//! Custom properties from JSON.

use serde_json::Value;

use super::util::{as_bool, as_f64, list, string};
use crate::property::{Properties, PropertyValue, parse_color};

/// Reads the `properties` array of `v`. Properties of an unknown type or
/// with a malformed value are dropped rather than failing the map.
pub(super) fn properties(v: &Value) -> Properties {
    let mut out = Properties::default();
    for p in list(v, "properties") {
        let name = string(p, "name");
        let Some(value) = p.get("value") else {
            continue;
        };
        let kind = match string(p, "type") {
            "" => "string",
            k => k,
        };
        if let Some(pv) = typed(kind, value, string(p, "propertytype")) {
            out.0.insert(name.to_string(), pv);
        }
    }
    out
}

fn typed(kind: &str, v: &Value, class: &str) -> Option<PropertyValue> {
    Some(match kind {
        "int" => PropertyValue::Int(as_f64(v)? as i64),
        "object" => PropertyValue::Object(as_f64(v)?.max(0.0) as u32),
        "float" => PropertyValue::Float(as_f64(v)?),
        "bool" => PropertyValue::Bool(as_bool(v)?),
        "color" => PropertyValue::Color(parse_color(v.as_str()?)?),
        "file" => PropertyValue::File(v.as_str()?.to_string()),
        "class" => PropertyValue::Class {
            class: class.to_string(),
            members: members(v.as_object()?),
        },
        _ => PropertyValue::String(v.as_str()?.to_string()),
    })
}

/// A class value's members carry no type names: whole numbers become
/// integers, other numbers floats.
fn members(obj: &serde_json::Map<String, Value>) -> Properties {
    let mut out = Properties::default();
    for (k, v) in obj {
        let value = match v {
            Value::Number(n) => match n.as_f64() {
                Some(f) if f.fract() == 0.0 && f.abs() < 9e15 => PropertyValue::Int(f as i64),
                Some(f) => PropertyValue::Float(f),
                None => continue,
            },
            Value::Bool(b) => PropertyValue::Bool(*b),
            Value::String(s) => PropertyValue::String(s.clone()),
            Value::Object(o) => PropertyValue::Class {
                class: String::new(),
                members: members(o),
            },
            _ => continue,
        };
        out.0.insert(k.clone(), value);
    }
    out
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use yowie_core::Color;

    use super::*;

    #[test]
    fn every_property_type() {
        let v = json!({"properties": [
            {"name": "s", "type": "string", "value": "hi"},
            {"name": "untyped", "value": "plain"},
            {"name": "i", "type": "int", "value": 3},
            {"name": "f", "type": "float", "value": 9.5},
            {"name": "b", "type": "bool", "value": true},
            {"name": "c", "type": "color", "value": "#80ff0000"},
            {"name": "file", "type": "file", "value": "a/b.png"},
            {"name": "o", "type": "object", "value": 7},
            {"name": "cls", "type": "class", "propertytype": "Spawn",
             "value": {"hp": 10, "rate": 0.5, "name": "orc", "inner": {"x": 1}}},
            {"name": "broken", "type": "int", "value": "nope"},
            {"name": "badcolor", "type": "color", "value": "zzz"}
        ]});
        let p = properties(&v);
        assert_eq!(p.string("s"), Some("hi"));
        assert_eq!(p.string("untyped"), Some("plain"));
        assert_eq!(p.int("i"), Some(3));
        assert_eq!(p.float("f"), Some(9.5));
        assert_eq!(p.bool("b"), Some(true));
        assert_eq!(p.color("c"), Some(Color::rgba8(255, 0, 0, 0x80)));
        assert_eq!(p.file("file"), Some("a/b.png"));
        assert_eq!(p.get("o"), Some(&PropertyValue::Object(7)));
        let cls = p.class("cls").unwrap();
        assert_eq!(
            (cls.int("hp"), cls.float("rate"), cls.string("name")),
            (Some(10), Some(0.5), Some("orc"))
        );
        assert_eq!(cls.class("inner").unwrap().int("x"), Some(1));
        assert!(
            matches!(p.get("cls"), Some(PropertyValue::Class { class, .. }) if class == "Spawn")
        );
        assert!(!p.contains("broken") && !p.contains("badcolor"));
    }
}
