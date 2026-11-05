use std::collections::BTreeMap;

use serde_json::Value;

use crate::{Category, LocaleError, plural};

/// The messages of one language.
#[derive(Clone, Debug, Default)]
pub struct Table {
    lang: String,
    plain: BTreeMap<String, String>,
    forms: BTreeMap<String, BTreeMap<Category, String>>,
}

impl Table {
    /// Creates an empty table for `lang`, a tag such as `"en"` or
    /// `"pt-BR"`. The tag selects the plural rules.
    pub fn new(lang: &str) -> Self {
        Self {
            lang: lang.to_string(),
            ..Self::default()
        }
    }

    /// Parses a JSON table. See the crate documentation for the format.
    pub fn from_json(lang: &str, json: &str) -> Result<Self, LocaleError> {
        let value: Value = serde_json::from_str(json).map_err(|source| LocaleError::Json {
            lang: lang.to_string(),
            source,
        })?;
        let mut table = Self::new(lang);
        match value {
            Value::Object(map) => table.add_object("", &map)?,
            _ => {
                return Err(LocaleError::InvalidEntry {
                    lang: lang.to_string(),
                    key: String::new(),
                });
            }
        }
        Ok(table)
    }

    fn add_object(
        &mut self,
        prefix: &str,
        map: &serde_json::Map<String, Value>,
    ) -> Result<(), LocaleError> {
        for (key, value) in map {
            let full = if prefix.is_empty() {
                key.clone()
            } else {
                format!("{prefix}.{key}")
            };
            let invalid = || LocaleError::InvalidEntry {
                lang: self.lang.clone(),
                key: full.clone(),
            };
            match value {
                Value::String(s) => {
                    self.plain.insert(full, s.clone());
                }
                Value::Object(obj) => {
                    let is_plural =
                        !obj.is_empty() && obj.keys().all(|k| Category::from_name(k).is_some());
                    if !is_plural {
                        self.add_object(&full, obj)?;
                        continue;
                    }
                    let mut forms = BTreeMap::new();
                    for (k, v) in obj {
                        let (Some(cat), Value::String(s)) = (Category::from_name(k), v) else {
                            return Err(invalid());
                        };
                        forms.insert(cat, s.clone());
                    }
                    self.forms.insert(full, forms);
                }
                _ => return Err(invalid()),
            }
        }
        Ok(())
    }

    /// The language tag this table was created with.
    pub fn lang(&self) -> &str {
        &self.lang
    }

    /// Sets a plain message.
    pub fn set(&mut self, key: &str, message: &str) {
        self.plain.insert(key.to_string(), message.to_string());
    }

    /// Sets a plural message from its forms. Include `Category::Other`.
    pub fn set_plural(&mut self, key: &str, forms: impl IntoIterator<Item = (Category, String)>) {
        self.forms
            .insert(key.to_string(), forms.into_iter().collect());
    }

    /// All keys, sorted.
    pub fn keys(&self) -> Vec<&str> {
        let mut keys: Vec<&str> = self
            .plain
            .keys()
            .chain(self.forms.keys())
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        keys
    }

    /// Returns true when the table has a message for `key`.
    pub fn has(&self, key: &str) -> bool {
        self.plain.contains_key(key) || self.forms.contains_key(key)
    }

    /// Finds the message for `key`. A plural message uses the form for
    /// `count` (1 when `None`), then `Other`, then any form.
    pub fn lookup(&self, key: &str, count: Option<f64>) -> Option<&str> {
        if let Some(s) = self.plain.get(key) {
            return Some(s);
        }
        let forms = self.forms.get(key)?;
        let cat = plural(&self.lang, count.unwrap_or(1.0));
        forms
            .get(&cat)
            .or_else(|| forms.get(&Category::Other))
            .or_else(|| forms.values().next())
            .map(String::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const JSON: &str = r#"{
        "title": "Game",
        "menu": {"play": "Play", "quit": "Quit"},
        "arrows": {"one": "{n} arrow", "other": "{n} arrows"},
        "deep": {"a": {"b": "x"}}
    }"#;

    #[test]
    fn flattens_nested_keys_and_detects_plurals() {
        let t = Table::from_json("en", JSON).unwrap();
        assert_eq!(
            t.keys(),
            ["arrows", "deep.a.b", "menu.play", "menu.quit", "title"]
        );
        assert!(t.has("arrows") && !t.has("menu"));
        assert_eq!(t.lookup("menu.play", None), Some("Play"));
        assert_eq!(t.lookup("arrows", Some(1.0)), Some("{n} arrow"));
        assert_eq!(t.lookup("arrows", Some(5.0)), Some("{n} arrows"));
        assert_eq!(t.lookup("arrows", None), Some("{n} arrow"));
        assert_eq!(t.lookup("nope", None), None);
    }

    #[test]
    fn plural_falls_back_to_other_then_any() {
        let mut t = Table::new("ru");
        t.set_plural(
            "a",
            [
                (Category::One, "one".into()),
                (Category::Other, "other".into()),
            ],
        );
        t.set_plural("b", [(Category::One, "only".into())]);
        assert_eq!(t.lookup("a", Some(5.0)), Some("other"));
        assert_eq!(t.lookup("b", Some(5.0)), Some("only"));
    }

    #[test]
    fn rejects_bad_json() {
        let bad = [
            "{",
            "[]",
            r#"{"a": 1}"#,
            r#"{"a": {"one": 1}}"#,
            r#"{"a": [1]}"#,
        ];
        for text in bad {
            assert!(Table::from_json("en", text).is_err(), "{text}");
        }
        let err = Table::from_json("en", r#"{"m": {"x": 5}}"#).unwrap_err();
        assert!(err.to_string().contains("m.x"));
    }
}
