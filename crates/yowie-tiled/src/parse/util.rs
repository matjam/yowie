//! Lenient accessors over `serde_json::Value`. Numbers and booleans may be
//! JSON values or text (as XML attributes are), so one parser serves both
//! formats.

use serde_json::Value;
use yowie_core::{Color, Vec2};

use crate::property::parse_color;

/// Reads a number from a JSON number or numeric text.
pub(super) fn as_f64(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// Reads a boolean from a JSON bool or the text `true`, `false`, `1`, `0`.
pub(super) fn as_bool(v: &Value) -> Option<bool> {
    match v {
        Value::Bool(b) => Some(*b),
        Value::String(s) => match s.trim() {
            "true" | "1" => Some(true),
            "false" | "0" => Some(false),
            _ => None,
        },
        Value::Number(n) => n.as_i64().map(|i| i != 0),
        _ => None,
    }
}

/// A number field.
pub(super) fn num(v: &Value, key: &str) -> Option<f64> {
    v.get(key).and_then(as_f64)
}

/// A number field as `f32`, or `default` when missing.
pub(super) fn f32_or(v: &Value, key: &str, default: f32) -> f32 {
    num(v, key).map_or(default, |n| n as f32)
}

/// A non-negative integer field, or 0 when missing or invalid.
pub(super) fn uint(v: &Value, key: &str) -> u32 {
    num(v, key)
        .filter(|n| *n >= 0.0 && *n <= f64::from(u32::MAX))
        .map_or(0, |n| n as u32)
}

/// A signed integer field, or `default` when missing.
pub(super) fn int_or(v: &Value, key: &str, default: i32) -> i32 {
    num(v, key).map_or(default, |n| n as i32)
}

/// A boolean field, or `default` when missing.
pub(super) fn flag(v: &Value, key: &str, default: bool) -> bool {
    v.get(key).and_then(as_bool).unwrap_or(default)
}

/// A string field, or `""` when missing.
pub(super) fn string<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(Value::as_str).unwrap_or("")
}

/// The first non-empty string among `keys`, or `""`.
pub(super) fn string_any<'a>(v: &'a Value, keys: &[&str]) -> &'a str {
    keys.iter()
        .map(|k| string(v, k))
        .find(|s| !s.is_empty())
        .unwrap_or("")
}

/// An array field, or an empty slice.
pub(super) fn list<'a>(v: &'a Value, key: &str) -> &'a [Value] {
    v.get(key)
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice)
}

/// A colour field in Tiled's text form.
pub(super) fn color(v: &Value, key: &str) -> Option<Color> {
    parse_color(string(v, key))
}

/// A pair of fields as a vector, with a default for each axis.
pub(super) fn vec2(v: &Value, kx: &str, ky: &str, default: f32) -> Vec2 {
    Vec2::new(f32_or(v, kx, default), f32_or(v, ky, default))
}
