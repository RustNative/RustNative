//! CLDR cardinal plural rules for the shipped locales, for whole numbers
//! (Unicode CLDR 45, `plurals.xml`; the rules are data, transcribed here as
//! the functions they describe — see `VENDORED.md`).
//!
//! | Language | Categories |
//! |---|---|
//! | `en`, `de` | one (1), other |
//! | `fr` | one (0, 1), many (a multiple of a million), other |
//! | `ar` | zero, one, two, few (3–10 mod 100), many (11–99 mod 100), other |
//! | `he` | one (1), two (2), other |
//! | `pl` | one (1), few (2–4 mod 10, not 12–14), many, other |
//! | `ru` | one (1 mod 10, not 11), few (2–4 mod 10, not 12–14), many, other |
//! | `ja`, `zh`, `ko` | other |
//!
//! A language not listed is given `en`'s rules.

/// A plural category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PluralCategory {
    /// `zero`.
    Zero,
    /// `one`.
    One,
    /// `two`.
    Two,
    /// `few`.
    Few,
    /// `many`.
    Many,
    /// `other`.
    Other,
}

impl PluralCategory {
    /// Its name in a catalogue (`one`).
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Zero => "zero",
            Self::One => "one",
            Self::Two => "two",
            Self::Few => "few",
            Self::Many => "many",
            Self::Other => "other",
        }
    }

    /// The category named `name`.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "zero" => Self::Zero,
            "one" => Self::One,
            "two" => Self::Two,
            "few" => Self::Few,
            "many" => Self::Many,
            "other" => Self::Other,
            _ => return None,
        })
    }
}

/// The category of whole number `n` in `locale`.
#[must_use]
pub fn plural_category(locale: &str, n: i64) -> PluralCategory {
    use PluralCategory::{Few, Many, One, Other, Two, Zero};
    let language = locale.split(['-', '_']).next().unwrap_or(locale).to_ascii_lowercase();
    let i = n.unsigned_abs();
    let (mod10, mod100) = (i % 10, i % 100);
    match language.as_str() {
        "ja" | "zh" | "ko" => Other,
        "fr" => match i {
            0 | 1 => One,
            _ if i % 1_000_000 == 0 => Many,
            _ => Other,
        },
        "ar" => match i {
            0 => Zero,
            1 => One,
            2 => Two,
            _ if (3..=10).contains(&mod100) => Few,
            _ if (11..=99).contains(&mod100) => Many,
            _ => Other,
        },
        "he" => match i {
            1 => One,
            2 => Two,
            _ => Other,
        },
        "pl" => match i {
            1 => One,
            _ if (2..=4).contains(&mod10) && !(12..=14).contains(&mod100) => Few,
            _ => Many,
        },
        "ru" => {
            if mod10 == 1 && mod100 != 11 {
                One
            } else if (2..=4).contains(&mod10) && !(12..=14).contains(&mod100) {
                Few
            } else {
                Many
            }
        }
        _ => {
            if i == 1 {
                One
            } else {
                Other
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use PluralCategory::{Few, Many, One, Other, Two, Zero};

    #[test]
    fn the_rules_match_cldr_for_their_sample_numbers() {
        // CLDR's own samples for each category.
        let cases: &[(&str, &[(i64, PluralCategory)])] = &[
            ("en", &[(0, Other), (1, One), (2, Other), (11, Other)]),
            ("de", &[(1, One), (5, Other)]),
            ("fr", &[(0, One), (1, One), (2, Other), (1_000_000, Many)]),
            (
                "ar",
                &[
                    (0, Zero),
                    (1, One),
                    (2, Two),
                    (3, Few),
                    (10, Few),
                    (103, Few),
                    (11, Many),
                    (99, Many),
                    (100, Other),
                    (102, Other),
                ],
            ),
            ("he", &[(1, One), (2, Two), (3, Other), (20, Other)]),
            (
                "pl",
                &[
                    (1, One),
                    (2, Few),
                    (4, Few),
                    (22, Few),
                    (5, Many),
                    (12, Many),
                    (14, Many),
                    (21, Many),
                    (0, Many),
                ],
            ),
            (
                "ru",
                &[
                    (1, One),
                    (21, One),
                    (11, Many),
                    (2, Few),
                    (24, Few),
                    (12, Many),
                    (5, Many),
                    (0, Many),
                ],
            ),
            ("ja", &[(1, Other), (2, Other)]),
            ("xx", &[(1, One), (2, Other)]),
        ];
        for (locale, samples) in cases {
            for (n, expected) in *samples {
                assert_eq!(plural_category(locale, *n), *expected, "{locale} {n}");
            }
        }
        assert_eq!(plural_category("pl-PL", 3), Few, "by language");
        for category in [Zero, One, Two, Few, Many, Other] {
            assert_eq!(PluralCategory::parse(category.name()), Some(category));
        }
    }
}
