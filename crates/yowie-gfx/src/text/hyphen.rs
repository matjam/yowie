//! Hyphenation: where a word may break at a line end, by Liang's pattern
//! method with the `hyphenation` crate's embedded dictionaries.

use std::collections::HashMap;

use hyphenation::{Hyphenator, Load, Standard};

/// The languages with hyphenation patterns. Set one on
/// [`TextStyle::hyphenate`](crate::TextStyle::hyphenate) and wrapped
/// text breaks long words with a hyphen instead of leaving ragged gaps.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Language {
    /// Danish.
    Danish,
    /// German, reformed spelling.
    German,
    /// American English.
    English,
    /// Spanish.
    Spanish,
    /// Finnish.
    Finnish,
    /// French.
    French,
    /// Italian.
    Italian,
    /// Norwegian Bokmal.
    Norwegian,
    /// Dutch.
    Dutch,
    /// Polish.
    Polish,
    /// Portuguese.
    Portuguese,
    /// Russian.
    Russian,
    /// Swedish.
    Swedish,
}

impl Language {
    /// Every language, in declaration order.
    pub const ALL: [Language; 13] = [
        Language::Danish,
        Language::German,
        Language::English,
        Language::Spanish,
        Language::Finnish,
        Language::French,
        Language::Italian,
        Language::Norwegian,
        Language::Dutch,
        Language::Polish,
        Language::Portuguese,
        Language::Russian,
        Language::Swedish,
    ];

    /// The language for a BCP 47 tag such as `"de"` or `"pt-BR"`: only the
    /// primary subtag is read. `None` when there are no patterns for it.
    pub fn from_tag(tag: &str) -> Option<Language> {
        let primary = tag.split(['-', '_']).next()?.to_ascii_lowercase();
        Some(match primary.as_str() {
            "da" => Language::Danish,
            "de" => Language::German,
            "en" => Language::English,
            "es" => Language::Spanish,
            "fi" => Language::Finnish,
            "fr" => Language::French,
            "it" => Language::Italian,
            "nb" | "no" | "nn" => Language::Norwegian,
            "nl" => Language::Dutch,
            "pl" => Language::Polish,
            "pt" => Language::Portuguese,
            "ru" => Language::Russian,
            "sv" => Language::Swedish,
            _ => return None,
        })
    }

    fn dictionary(self) -> hyphenation::Language {
        use hyphenation::Language as L;
        match self {
            Language::Danish => L::Danish,
            Language::German => L::German1996,
            Language::English => L::EnglishUS,
            Language::Spanish => L::Spanish,
            Language::Finnish => L::Finnish,
            Language::French => L::French,
            Language::Italian => L::Italian,
            Language::Norwegian => L::NorwegianBokmal,
            Language::Dutch => L::Dutch,
            Language::Polish => L::Polish,
            Language::Portuguese => L::Portuguese,
            Language::Russian => L::Russian,
            Language::Swedish => L::Swedish,
        }
    }
}

/// Fewest letters kept before and after a break.
const MIN_LEFT: usize = 2;
const MIN_RIGHT: usize = 3;

/// Dictionaries loaded so far, one per language.
#[derive(Default)]
pub(crate) struct Hyphenators {
    loaded: HashMap<Language, Option<Standard>>,
}

impl std::fmt::Debug for Hyphenators {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Hyphenators")
            .field("loaded", &self.loaded.len())
            .finish()
    }
}

impl Hyphenators {
    /// Byte offsets in `text` where a hyphen may go, ascending.
    pub(crate) fn points(&mut self, language: Language, text: &str) -> Vec<usize> {
        let dict = self.loaded.entry(language).or_insert_with(|| {
            match Standard::from_embedded(language.dictionary()) {
                Ok(d) => Some(d),
                Err(e) => {
                    log::error!("yowie-gfx: hyphenation patterns for {language:?}: {e}");
                    None
                }
            }
        });
        let Some(dict) = dict else {
            return Vec::new();
        };
        let mut out = Vec::new();
        let mut word_start = None;
        let mut chars = text.char_indices().peekable();
        while let Some((i, c)) = chars.next() {
            if c.is_alphabetic() {
                word_start.get_or_insert(i);
            }
            let ends = chars.peek().is_none_or(|(_, n)| !n.is_alphabetic());
            if let (Some(start), true) = (word_start, ends && c.is_alphabetic()) {
                let end = i + c.len_utf8();
                let word = &text[start..end];
                let letters = word.chars().count();
                if letters >= MIN_LEFT + MIN_RIGHT {
                    for b in dict.hyphenate(word).breaks {
                        let left = word[..b].chars().count();
                        if left >= MIN_LEFT && letters - left >= MIN_RIGHT {
                            out.push(start + b);
                        }
                    }
                }
                word_start = None;
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_language_breaks_a_long_word() {
        let words = [
            (Language::Danish, "kommunalbestyrelsesmedlem"),
            (Language::German, "Donaudampfschifffahrtsgesellschaft"),
            (Language::English, "hyphenation"),
            (Language::Spanish, "electrodomestico"),
            (Language::Finnish, "tietokonejarjestelma"),
            (Language::French, "anticonstitutionnellement"),
            (Language::Italian, "precipitevolissimevolmente"),
            (Language::Norwegian, "arbeidsgiverorganisasjon"),
            (Language::Dutch, "wetenschappelijk"),
            (Language::Polish, "bezwzgledniejszy"),
            (Language::Portuguese, "inconstitucionalissimamente"),
            (Language::Russian, "достопримечательность"),
            (Language::Swedish, "sjukhusadministration"),
        ];
        let mut h = Hyphenators::default();
        for (lang, word) in words {
            let points = h.points(lang, word);
            assert!(!points.is_empty(), "{lang:?} found no break in {word}");
            assert!(points.iter().all(|p| word.is_char_boundary(*p)));
        }
        assert_eq!(Language::from_tag("pt-BR"), Some(Language::Portuguese));
        assert_eq!(Language::ALL.len(), 13);
    }
}
