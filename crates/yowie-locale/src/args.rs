use std::fmt::Display;

/// A number that can select a plural form and fill a placeholder.
pub trait Number: Display {
    /// The value as used for plural rules.
    fn as_f64(&self) -> f64;
}

macro_rules! number {
    ($($ty:ty),*) => {$(
        impl Number for $ty {
            fn as_f64(&self) -> f64 {
                *self as f64
            }
        }
    )*};
}
number!(i8, i16, i32, i64, isize, u8, u16, u32, u64, usize, f32, f64);

/// Named values for the placeholders of one message.
///
/// The number set with [`Args::num`] under the name `n` (or, when no `n`
/// exists, the first number set) selects the plural form.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Args {
    values: Vec<(String, String)>,
    count: Option<f64>,
    count_is_n: bool,
}

impl Args {
    /// Creates an empty set of arguments.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets a placeholder to any displayable value. A later call with the
    /// same name replaces the earlier value.
    pub fn set(mut self, name: &str, value: impl Display) -> Self {
        self.values.retain(|(k, _)| k != name);
        self.values.push((name.to_string(), value.to_string()));
        self
    }

    /// Sets a placeholder to a number that also selects the plural form.
    pub fn num(mut self, name: &str, value: impl Number) -> Self {
        if name == "n" || (self.count.is_none() && !self.count_is_n) {
            self.count = Some(value.as_f64());
            self.count_is_n = name == "n";
        }
        self.set(name, value)
    }

    /// The number that selects the plural form, if one was set.
    pub fn count(&self) -> Option<f64> {
        self.count
    }

    /// The text for a placeholder name.
    pub fn value(&self, name: &str) -> Option<&str> {
        self.values
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }
}

/// Replaces `{name}` placeholders in `msg` with values from `args`.
/// `{{` and `}}` produce literal braces; unknown placeholders and an
/// unterminated `{` stay as written.
pub(crate) fn expand(msg: &str, args: &Args) -> String {
    if !msg.contains(['{', '}']) {
        return msg.to_string();
    }
    let mut out = String::with_capacity(msg.len() + 8);
    let mut rest = msg;
    while let Some(pos) = rest.find(['{', '}']) {
        out.push_str(&rest[..pos]);
        let tail = &rest[pos..];
        if tail.starts_with("{{") || tail.starts_with("}}") {
            out.push_str(&tail[..1]);
            rest = &tail[2..];
        } else if let Some(after) = tail.strip_prefix('}') {
            out.push('}');
            rest = after;
        } else if let Some(end) = tail.find('}') {
            match args.value(&tail[1..end]) {
                Some(v) => out.push_str(v),
                None => out.push_str(&tail[..=end]),
            }
            rest = &tail[end + 1..];
        } else {
            out.push_str(tail);
            return out;
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_placeholders() {
        let args = Args::new().set("who", "Ada").num("n", 3);
        let cases = [
            ("plain", "plain"),
            ("Hi {who}", "Hi Ada"),
            ("{n} of {who}", "3 of Ada"),
            ("{missing} stays", "{missing} stays"),
            ("{{literal}} {who}", "{literal} Ada"),
            ("open {who", "open {who"),
            ("stray } brace", "stray } brace"),
            ("{}", "{}"),
            ("", ""),
            ("é{who}ü", "éAdaü"),
        ];
        for (msg, want) in cases {
            assert_eq!(expand(msg, &args), want, "{msg}");
        }
    }

    #[test]
    fn count_prefers_n() {
        assert_eq!(Args::new().num("a", 2).num("b", 5).count(), Some(2.0));
        assert_eq!(Args::new().num("a", 2).num("n", 5).count(), Some(5.0));
        assert_eq!(Args::new().set("a", 2).count(), None);
        assert_eq!(Args::new().set("a", 1).set("a", 2).value("a"), Some("2"));
    }
}
