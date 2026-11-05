//! Typed custom properties.

use std::collections::BTreeMap;

use yowie_core::Color;

/// One custom property value.
#[derive(Clone, Debug, PartialEq)]
pub enum PropertyValue {
    /// Text.
    String(String),
    /// A whole number.
    Int(i64),
    /// A floating point number.
    Float(f64),
    /// A boolean.
    Bool(bool),
    /// A colour (sRGB, straight alpha).
    Color(Color),
    /// A file path, relative to the file that names it.
    File(String),
    /// A reference to an object by id; 0 is none.
    Object(u32),
    /// A custom class instance: its class name and member values.
    Class {
        /// The class name (empty when the file does not give one).
        class: String,
        /// The members.
        members: Properties,
    },
}

/// A set of custom properties by name.
///
/// The typed accessors return `None` when the property is missing or has a
/// different type, except [`float`](Properties::float), which also accepts
/// an integer.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Properties(pub BTreeMap<String, PropertyValue>);

impl Properties {
    /// Returns the property called `name`.
    pub fn get(&self, name: &str) -> Option<&PropertyValue> {
        self.0.get(name)
    }

    /// Reports whether a property is set.
    pub fn contains(&self, name: &str) -> bool {
        self.0.contains_key(name)
    }

    /// Returns a string property.
    pub fn string(&self, name: &str) -> Option<&str> {
        match self.get(name)? {
            PropertyValue::String(s) => Some(s),
            _ => None,
        }
    }

    /// Returns an integer property.
    pub fn int(&self, name: &str) -> Option<i64> {
        match self.get(name)? {
            PropertyValue::Int(v) => Some(*v),
            _ => None,
        }
    }

    /// Returns a float property; an integer property converts.
    pub fn float(&self, name: &str) -> Option<f64> {
        match self.get(name)? {
            PropertyValue::Float(v) => Some(*v),
            PropertyValue::Int(v) => Some(*v as f64),
            _ => None,
        }
    }

    /// Returns a boolean property.
    pub fn bool(&self, name: &str) -> Option<bool> {
        match self.get(name)? {
            PropertyValue::Bool(v) => Some(*v),
            _ => None,
        }
    }

    /// Returns a colour property.
    pub fn color(&self, name: &str) -> Option<Color> {
        match self.get(name)? {
            PropertyValue::Color(c) => Some(*c),
            _ => None,
        }
    }

    /// Returns a file property.
    pub fn file(&self, name: &str) -> Option<&str> {
        match self.get(name)? {
            PropertyValue::File(s) => Some(s),
            _ => None,
        }
    }

    /// Returns the members of a class property.
    pub fn class(&self, name: &str) -> Option<&Properties> {
        match self.get(name)? {
            PropertyValue::Class { members, .. } => Some(members),
            _ => None,
        }
    }

    /// Iterates over the properties in name order.
    pub fn iter(&self) -> impl Iterator<Item = (&String, &PropertyValue)> {
        self.0.iter()
    }

    /// The number of properties.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Reports whether there are no properties.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// Parses Tiled's `#AARRGGBB` or `#RRGGBB` colour text (the `#` is optional).
///
/// Note the alpha byte comes first, unlike CSS. Returns `None` for any other
/// length or a non-hex digit.
pub fn parse_color(text: &str) -> Option<Color> {
    let s = text.trim().trim_start_matches('#');
    let byte = |i: usize| u8::from_str_radix(s.get(i..i + 2)?, 16).ok();
    match s.len() {
        6 => Some(Color::rgb8(byte(0)?, byte(2)?, byte(4)?)),
        8 => Some(Color::rgba8(byte(2)?, byte(4)?, byte(6)?, byte(0)?)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_color_table() {
        let cases = [
            ("#336699", Some(Color::rgb8(0x33, 0x66, 0x99))),
            ("#80336699", Some(Color::rgba8(0x33, 0x66, 0x99, 0x80))),
            ("ff0000", Some(Color::rgb8(255, 0, 0))),
            ("#fff", None),
            ("#gg0000", None),
            ("", None),
        ];
        for (text, want) in cases {
            assert_eq!(parse_color(text), want, "{text:?}");
        }
    }

    #[test]
    fn accessors_check_types() {
        let mut p = Properties::default();
        p.0.insert("n".into(), PropertyValue::Int(3));
        p.0.insert("s".into(), PropertyValue::String("x".into()));
        assert_eq!(p.int("n"), Some(3));
        assert_eq!(p.float("n"), Some(3.0));
        assert_eq!(p.int("s"), None);
        assert_eq!(p.string("s"), Some("x"));
        assert_eq!(p.bool("missing"), None);
        assert_eq!(p.len(), 2);
    }
}
