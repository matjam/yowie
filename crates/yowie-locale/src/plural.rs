/// A CLDR plural category. Which categories a language uses depends on
/// the language; every language uses [`Category::Other`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Category {
    /// Zero, used by Arabic and Latvian.
    Zero,
    /// Singular.
    One,
    /// Dual.
    Two,
    /// A small number, such as 2 to 4 in Russian.
    Few,
    /// A large number, such as 5 to 20 in Russian.
    Many,
    /// Everything else.
    Other,
}

impl Category {
    /// The JSON key for this category.
    pub fn name(self) -> &'static str {
        match self {
            Category::Zero => "zero",
            Category::One => "one",
            Category::Two => "two",
            Category::Few => "few",
            Category::Many => "many",
            Category::Other => "other",
        }
    }

    /// Parses a JSON key such as `"few"`.
    pub fn from_name(name: &str) -> Option<Self> {
        [
            Category::Zero,
            Category::One,
            Category::Two,
            Category::Few,
            Category::Many,
            Category::Other,
        ]
        .into_iter()
        .find(|c| c.name() == name)
    }
}

/// Returns the plural category of `n` in language `lang`.
///
/// `lang` is a tag such as `"en"`, `"pt-BR"` or `"ru_RU"`; only the
/// language and, for Portuguese, the region matter. Rules follow CLDR for
/// en, fr, de, es, it, pt, ru, pl, uk, cs, ar, he, ja, zh, ko and tr, and
/// several related languages. Languages not listed use the English rule:
/// exactly 1 is `One`, everything else `Other`.
pub fn plural(lang: &str, n: f64) -> Category {
    let lower = lang.to_ascii_lowercase();
    let (base, region) = match lower.split_once(['-', '_']) {
        Some((b, r)) => (b, r),
        None => (lower.as_str(), ""),
    };
    // CLDR rules apply to the absolute value.
    let n = n.abs();
    let whole = n.fract() == 0.0 && n.is_finite();
    let i = n.trunc() as i64;
    let m10 = i.rem_euclid(10);
    let m100 = i.rem_euclid(100);
    let slavic_few = (2..=4).contains(&m10) && !(12..=14).contains(&m100);
    match base {
        "ja" | "zh" | "ko" | "vi" | "th" | "id" | "ms" => Category::Other,
        // Turkish has no distinct plural after a number.
        "tr" => {
            if n == 1.0 {
                Category::One
            } else {
                Category::Other
            }
        }
        "fr" | "hy" | "kab" => zero_to_two_is_one(n),
        "pt" if region.is_empty() || region == "br" => zero_to_two_is_one(n),
        "ru" | "uk" | "be" => {
            if !whole {
                Category::Other
            } else if m10 == 1 && m100 != 11 {
                Category::One
            } else if slavic_few {
                Category::Few
            } else {
                Category::Many
            }
        }
        "pl" => {
            if !whole {
                Category::Other
            } else if i == 1 {
                Category::One
            } else if slavic_few {
                Category::Few
            } else {
                Category::Many
            }
        }
        "cs" | "sk" => {
            if !whole {
                Category::Many
            } else if i == 1 {
                Category::One
            } else if (2..=4).contains(&i) {
                Category::Few
            } else {
                Category::Other
            }
        }
        "ar" => {
            if !whole {
                Category::Other
            } else if i == 0 {
                Category::Zero
            } else if i == 1 {
                Category::One
            } else if i == 2 {
                Category::Two
            } else if (3..=10).contains(&m100) {
                Category::Few
            } else if (11..=99).contains(&m100) {
                Category::Many
            } else {
                Category::Other
            }
        }
        "he" | "iw" => match (whole, i) {
            (true, 1) => Category::One,
            (true, 2) => Category::Two,
            _ => Category::Other,
        },
        "lv" => {
            if !whole {
                Category::Other
            } else if m10 == 0 || (11..=19).contains(&m100) {
                Category::Zero
            } else if m10 == 1 && m100 != 11 {
                Category::One
            } else {
                Category::Other
            }
        }
        "ga" => {
            if !whole {
                Category::Other
            } else {
                match i {
                    1 => Category::One,
                    2 => Category::Two,
                    3..=6 => Category::Few,
                    7..=10 => Category::Many,
                    _ => Category::Other,
                }
            }
        }
        _ => {
            if whole && i == 1 {
                Category::One
            } else {
                Category::Other
            }
        }
    }
}

fn zero_to_two_is_one(n: f64) -> Category {
    if (0.0..2.0).contains(&n) {
        Category::One
    } else {
        Category::Other
    }
}

#[cfg(test)]
mod tests {
    use super::Category::*;
    use super::*;

    #[test]
    fn rules_by_language() {
        let cases: &[(&str, &[(f64, Category)])] = &[
            (
                "en",
                &[(0.0, Other), (1.0, One), (2.0, Other), (1.5, Other)],
            ),
            ("de", &[(1.0, One), (0.0, Other), (21.0, Other)]),
            ("es", &[(1.0, One), (2.0, Other)]),
            ("it", &[(1.0, One), (2.0, Other)]),
            ("fr", &[(0.0, One), (1.0, One), (1.5, One), (2.0, Other)]),
            ("pt", &[(0.0, One), (1.0, One), (2.0, Other)]),
            ("pt-BR", &[(0.0, One), (2.0, Other)]),
            ("pt-PT", &[(0.0, Other), (1.0, One), (2.0, Other)]),
            (
                "ru",
                &[
                    (1.0, One),
                    (2.0, Few),
                    (5.0, Many),
                    (11.0, Many),
                    (21.0, One),
                    (22.0, Few),
                    (112.0, Many),
                    (1.5, Other),
                ],
            ),
            ("uk", &[(1.0, One), (3.0, Few), (11.0, Many), (101.0, One)]),
            (
                "pl",
                &[
                    (1.0, One),
                    (2.0, Few),
                    (5.0, Many),
                    (22.0, Few),
                    (12.0, Many),
                    (21.0, Many),
                ],
            ),
            ("cs", &[(1.0, One), (3.0, Few), (5.0, Other), (1.5, Many)]),
            (
                "ar",
                &[
                    (0.0, Zero),
                    (1.0, One),
                    (2.0, Two),
                    (3.0, Few),
                    (10.0, Few),
                    (11.0, Many),
                    (99.0, Many),
                    (100.0, Other),
                    (103.0, Few),
                ],
            ),
            ("he", &[(1.0, One), (2.0, Two), (3.0, Other)]),
            ("ja", &[(1.0, Other), (0.0, Other)]),
            ("zh", &[(1.0, Other)]),
            ("ko", &[(1.0, Other)]),
            ("tr", &[(1.0, One), (2.0, Other), (0.0, Other)]),
            ("xx", &[(1.0, One), (2.0, Other)]),
            ("ru_RU", &[(2.0, Few)]),
            ("RU", &[(2.0, Few)]),
        ];
        for (lang, rows) in cases {
            for &(n, want) in *rows {
                assert_eq!(plural(lang, n), want, "{lang} {n}");
            }
        }
    }

    #[test]
    fn negative_numbers_use_the_absolute_value() {
        assert_eq!(plural("ru", -1.0), One);
        assert_eq!(plural("ru", -3.0), Few);
    }

    #[test]
    fn category_names_round_trip() {
        for c in [Zero, One, Two, Few, Many, Other] {
            assert_eq!(Category::from_name(c.name()), Some(c));
        }
        assert_eq!(Category::from_name("lots"), None);
    }
}
