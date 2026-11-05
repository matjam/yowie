//! Splitting a command line into words.

/// A malformed command line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParseError {
    /// A double quote was opened and never closed.
    UnterminatedQuote,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::UnterminatedQuote => f.write_str("unterminated quote"),
        }
    }
}

impl std::error::Error for ParseError {}

/// Splits `line` into words separated by spaces. A word in double quotes
/// keeps its spaces; inside quotes `\"` and `\\` stand for a quote and a
/// backslash. Blank lines and lines starting with `#` or `//` give no
/// words.
pub fn split_words(line: &str) -> Result<Vec<String>, ParseError> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') || line.starts_with("//") {
        return Ok(Vec::new());
    }
    let mut words = Vec::new();
    let mut cur = String::new();
    let mut started = false;
    let mut quoted = false;
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                quoted = !quoted;
                started = true;
            }
            '\\' if quoted => match chars.next() {
                Some(n @ ('"' | '\\')) => cur.push(n),
                Some(n) => {
                    cur.push('\\');
                    cur.push(n);
                }
                None => cur.push('\\'),
            },
            c if c.is_whitespace() && !quoted => {
                if started {
                    words.push(std::mem::take(&mut cur));
                    started = false;
                }
            }
            c => {
                cur.push(c);
                started = true;
            }
        }
    }
    if quoted {
        return Err(ParseError::UnterminatedQuote);
    }
    if started {
        words.push(cur);
    }
    Ok(words)
}

/// The word being typed at the end of `line` and where it starts, for
/// completion. A trailing space starts an empty word.
pub fn last_word(line: &str) -> (usize, &str) {
    let start = line
        .rfind(|c: char| c.is_whitespace())
        .map_or(0, |i| i + line[i..].chars().next().map_or(1, char::len_utf8));
    (start, &line[start..])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_words_and_quotes() {
        let cases: &[(&str, &[&str])] = &[
            ("echo hello world", &["echo", "hello", "world"]),
            ("  set   speed  3 ", &["set", "speed", "3"]),
            ("bind Space \"echo jump\"", &["bind", "Space", "echo jump"]),
            ("say \"a \\\"quoted\\\" word\"", &["say", "a \"quoted\" word"]),
            ("empty \"\" arg", &["empty", "", "arg"]),
            ("path \"C:\\\\dir\"", &["path", "C:\\dir"]),
            ("", &[]),
            ("# comment", &[]),
            ("// comment", &[]),
            ("x\\y", &["x\\y"]),
        ];
        for (line, want) in cases {
            let got = split_words(line).expect(line);
            assert_eq!(&got, want, "{line:?}");
        }
    }

    #[test]
    fn unterminated_quote_is_an_error() {
        assert_eq!(split_words("echo \"oops"), Err(ParseError::UnterminatedQuote));
    }

    #[test]
    fn last_word_finds_the_completion_target() {
        assert_eq!(last_word("set spe"), (4, "spe"));
        assert_eq!(last_word("set "), (4, ""));
        assert_eq!(last_word("he"), (0, "he"));
        assert_eq!(last_word(""), (0, ""));
    }
}
