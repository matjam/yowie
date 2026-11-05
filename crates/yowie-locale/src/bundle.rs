use std::collections::BTreeMap;

use crate::args::expand;
use crate::{Args, LocaleError, Table};

/// The active system locale as a tag such as `"en-US"`, if the platform
/// reports one.
pub fn system_locale() -> Option<String> {
    sys_locale::get_locale()
}

fn normalise(lang: &str) -> String {
    lang.trim().replace('_', "-").to_ascii_lowercase()
}

/// The tables for every language plus the fallback languages.
///
/// Build the bundle at load time, then read it from any thread. Fallbacks
/// are explicit: nothing is installed for you.
#[derive(Clone, Debug, Default)]
pub struct Bundle {
    tables: BTreeMap<String, Table>,
    fallbacks: Vec<String>,
}

impl Bundle {
    /// Creates a bundle. `fallbacks` are tried in order after the requested
    /// language and its base language, for keys a translation lacks.
    pub fn new<S: AsRef<str>>(fallbacks: impl IntoIterator<Item = S>) -> Self {
        Self {
            tables: BTreeMap::new(),
            fallbacks: fallbacks
                .into_iter()
                .map(|s| normalise(s.as_ref()))
                .collect(),
        }
    }

    /// Adds a table, replacing any earlier one for the same language.
    pub fn add(&mut self, table: Table) {
        self.tables.insert(normalise(table.lang()), table);
    }

    /// Parses a JSON table for `lang` and adds it.
    pub fn load_json(&mut self, lang: &str, json: &str) -> Result<(), LocaleError> {
        self.add(Table::from_json(lang, json)?);
        Ok(())
    }

    /// The table for exactly `lang`, ignoring case.
    pub fn table(&self, lang: &str) -> Option<&Table> {
        self.tables.get(&normalise(lang))
    }

    /// The tags of all loaded tables, sorted.
    pub fn languages(&self) -> Vec<&str> {
        self.tables.values().map(Table::lang).collect()
    }

    /// Keys present in the `source` table but absent from `lang`: the work
    /// a translator still has to do. Empty when `source` is not loaded.
    pub fn missing(&self, lang: &str, source: &str) -> Vec<String> {
        let Some(src) = self.table(source) else {
            return Vec::new();
        };
        let dst = self.table(lang);
        src.keys()
            .into_iter()
            .filter(|k| !dst.is_some_and(|d| d.has(k)))
            .map(str::to_string)
            .collect()
    }

    /// Returns a translator for `lang`. Lookups try `lang`, then its base
    /// language (`pt` for `pt-BR`), then the fallbacks.
    pub fn translator(&self, lang: &str) -> Translator<'_> {
        let l = normalise(lang);
        let mut chain = vec![l.clone()];
        if let Some((base, _)) = l.split_once('-') {
            chain.push(base.to_string());
        }
        chain.extend(self.fallbacks.iter().cloned());
        Translator {
            bundle: self,
            chain,
            lang: lang.to_string(),
        }
    }

    /// Returns a translator for the system locale, or one that uses only
    /// the fallbacks when the platform reports none.
    pub fn system_translator(&self) -> Translator<'_> {
        match system_locale() {
            Some(lang) => self.translator(&lang),
            None => self.translator(""),
        }
    }
}

/// Looks up messages for one language, falling back as the [`Bundle`]
/// directs.
#[derive(Clone, Debug)]
pub struct Translator<'a> {
    bundle: &'a Bundle,
    chain: Vec<String>,
    lang: String,
}

impl Translator<'_> {
    /// The language this translator was created for.
    pub fn lang(&self) -> &str {
        &self.lang
    }

    /// Returns the message for `key`, or `[key]` when no table has it.
    pub fn get(&self, key: &str) -> String {
        self.get_with(key, &Args::new())
    }

    /// Returns the message for `key` with placeholders filled from `args`.
    /// A plural message uses [`Args::count`].
    pub fn get_with(&self, key: &str, args: &Args) -> String {
        for lang in &self.chain {
            let Some(table) = self.bundle.tables.get(lang) else {
                continue;
            };
            if let Some(msg) = table.lookup(key, args.count()) {
                return expand(msg, args);
            }
        }
        format!("[{key}]")
    }

    /// Returns the plural message for `key` for the count `n`, with `{n}`
    /// filled in.
    pub fn count(&self, key: &str, n: i64) -> String {
        self.get_with(key, &Args::new().num("n", n))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bundle() -> Bundle {
        let mut b = Bundle::new(["en"]);
        b.load_json(
            "en",
            r#"{"play": "Play", "greet": "Hello, {who}!", "only_en": "English",
                "arrows": {"one": "{n} arrow", "other": "{n} arrows"}}"#,
        )
        .unwrap();
        b.load_json(
            "ru",
            r#"{"play": "Играть",
                "arrows": {"one": "{n} стрела", "few": "{n} стрелы", "many": "{n} стрел", "other": "{n} стрелы"}}"#,
        )
        .unwrap();
        b.load_json("pt", r#"{"play": "Jogar"}"#).unwrap();
        b
    }

    #[test]
    fn looks_up_with_fallbacks() {
        let b = bundle();
        let ru = b.translator("ru");
        assert_eq!(ru.get("play"), "Играть");
        assert_eq!(ru.get("only_en"), "English");
        assert_eq!(ru.get("nothing"), "[nothing]");
        assert_eq!(b.translator("pt_BR").get("play"), "Jogar");
        assert_eq!(b.translator("ru-RU").get("play"), "Играть");
        assert_eq!(b.translator("EN").get("play"), "Play");
        assert_eq!(b.translator("xx").get("play"), "Play");
        assert_eq!(ru.lang(), "ru");
    }

    #[test]
    fn fills_placeholders_and_plurals() {
        let b = bundle();
        let en = b.translator("en");
        let args = Args::new().set("who", "Ada");
        assert_eq!(en.get_with("greet", &args), "Hello, Ada!");
        assert_eq!(en.get("greet"), "Hello, {who}!");
        let cases = [(1, "1 arrow"), (0, "0 arrows"), (3, "3 arrows")];
        for (n, want) in cases {
            assert_eq!(en.count("arrows", n), want);
        }
        let ru = b.translator("ru");
        let cases = [
            (1, "1 стрела"),
            (3, "3 стрелы"),
            (5, "5 стрел"),
            (21, "21 стрела"),
        ];
        for (n, want) in cases {
            assert_eq!(ru.count("arrows", n), want);
        }
    }

    #[test]
    fn reports_missing_keys() {
        let b = bundle();
        assert_eq!(b.missing("ru", "en"), ["greet", "only_en"]);
        assert_eq!(b.missing("de", "en").len(), 4);
        assert!(b.missing("en", "zz").is_empty());
        assert_eq!(b.languages(), ["en", "pt", "ru"]);
        assert!(b.table("RU").is_some());
    }

    #[test]
    fn system_translator_always_resolves() {
        let b = bundle();
        // Whatever the host locale, the English fallback answers.
        assert_eq!(b.system_translator().get("only_en"), "English");
    }
}
