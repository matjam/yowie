use std::fmt;
use std::str::FromStr;

use crate::{Rng, RngError};

const MAX_DICE: u32 = 1000;
const MAX_SIDES: u32 = 1_000_000;

/// A dice expression such as `3d6+2`: `count` dice of `sides` sides, plus
/// a fixed `modifier`.
///
/// Parse one with `str::parse`. The count may be omitted (`d20` is one
/// die). Whitespace is ignored and `D` is accepted for `d`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Dice {
    /// Number of dice, 1 to 1000.
    pub count: u32,
    /// Sides per die, 1 to 1,000,000.
    pub sides: u32,
    /// Amount added to the total; may be negative.
    pub modifier: i32,
}

impl Dice {
    /// Rolls the expression and returns the total.
    pub fn roll(&self, rng: &mut Rng) -> i64 {
        rng.roll(self.count, self.sides) as i64 + i64::from(self.modifier)
    }

    /// The lowest possible total.
    pub fn min(&self) -> i64 {
        i64::from(self.count) + i64::from(self.modifier)
    }

    /// The highest possible total.
    pub fn max(&self) -> i64 {
        i64::from(self.count) * i64::from(self.sides) + i64::from(self.modifier)
    }
}

impl FromStr for Dice {
    type Err = RngError;

    fn from_str(s: &str) -> Result<Self, RngError> {
        let bad = || RngError::InvalidDice(s.to_string());
        let text: String = s
            .chars()
            .filter(|c| !c.is_whitespace())
            .map(|c| c.to_ascii_lowercase())
            .collect();
        let (count_text, rest) = text.split_once('d').ok_or_else(bad)?;
        let count = if count_text.is_empty() {
            1
        } else {
            parse_digits(count_text).ok_or_else(bad)?
        };
        let split = rest.find(['+', '-']).unwrap_or(rest.len());
        let (sides_text, mod_text) = rest.split_at(split);
        let sides = parse_digits(sides_text).ok_or_else(bad)?;
        let modifier = match mod_text.split_at_checked(1) {
            None | Some(("", _)) => 0,
            Some((sign, digits)) => {
                let v = i32::try_from(parse_digits(digits).ok_or_else(bad)?).map_err(|_| bad())?;
                if sign == "-" { -v } else { v }
            }
        };
        if !(1..=MAX_DICE).contains(&count) || !(1..=MAX_SIDES).contains(&sides) {
            return Err(RngError::DiceOutOfRange(s.to_string()));
        }
        Ok(Dice {
            count,
            sides,
            modifier,
        })
    }
}

fn parse_digits(s: &str) -> Option<u32> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

impl fmt::Display for Dice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}d{}", self.count, self.sides)?;
        match self.modifier {
            0 => Ok(()),
            m => write!(f, "{m:+}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dice(count: u32, sides: u32, modifier: i32) -> Dice {
        Dice {
            count,
            sides,
            modifier,
        }
    }

    #[test]
    fn parses_valid_notation() {
        let cases = [
            ("3d6+2", dice(3, 6, 2)),
            ("d20", dice(1, 20, 0)),
            ("2D8-1", dice(2, 8, -1)),
            (" 1 d 4 + 10 ", dice(1, 4, 10)),
            ("10d10", dice(10, 10, 0)),
        ];
        for (text, want) in cases {
            assert_eq!(text.parse::<Dice>(), Ok(want), "{text}");
        }
    }

    #[test]
    fn rejects_invalid_notation() {
        let invalid = ["", "d", "3", "3d", "d6+", "3d6++2", "x3d6", "3d6+a", "-3d6"];
        for text in invalid {
            assert!(
                matches!(text.parse::<Dice>(), Err(RngError::InvalidDice(_))),
                "{text}"
            );
        }
        for text in ["0d6", "3d0", "1001d6", "d9999999"] {
            assert!(
                matches!(text.parse::<Dice>(), Err(RngError::DiceOutOfRange(_))),
                "{text}"
            );
        }
    }

    #[test]
    fn rolls_within_bounds() {
        let mut rng = Rng::new(1);
        let d: Dice = "3d6+2".parse().unwrap();
        assert_eq!((d.min(), d.max()), (5, 20));
        for _ in 0..500 {
            assert!((5..=20).contains(&d.roll(&mut rng)));
        }
    }

    #[test]
    fn display_round_trips() {
        for text in ["3d6+2", "1d20", "2d8-1"] {
            let d: Dice = text.parse().unwrap();
            assert_eq!(d.to_string(), text);
        }
    }
}
