//! Completing the word at the end of a command line.

use crate::parse::last_word;

/// The names completion chooses from.
#[derive(Clone, Copy, Debug, Default)]
pub struct Names<'a> {
    /// Command names.
    pub commands: &'a [String],
    /// Variable names.
    pub vars: &'a [String],
    /// Key names, for `bind` and `unbind`.
    pub keys: &'a [&'a str],
    /// Panel names, for `panels`.
    pub panels: &'a [String],
}

/// What completion found for the word being typed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Completion {
    /// Where the completed word starts in the line.
    pub start: usize,
    /// Every name that starts with the typed prefix, sorted.
    pub candidates: Vec<String>,
    /// The longest prefix all candidates share, which the line is
    /// extended to.
    pub common: String,
}

impl Completion {
    /// The line with the word replaced by the common prefix, plus a space
    /// when the match is unique.
    pub fn apply(&self, line: &str) -> String {
        if self.candidates.is_empty() {
            return line.to_owned();
        }
        let mut out = line[..self.start].to_owned();
        out.push_str(&self.common);
        if self.candidates.len() == 1 {
            out.push(' ');
        }
        out
    }
}

fn common_prefix(items: &[String]) -> String {
    let Some(first) = items.first() else {
        return String::new();
    };
    let mut len = first.len();
    for s in &items[1..] {
        len = first
            .bytes()
            .zip(s.bytes())
            .take(len)
            .take_while(|(a, b)| a.eq_ignore_ascii_case(b))
            .count();
    }
    while !first.is_char_boundary(len) {
        len -= 1;
    }
    first[..len].to_owned()
}

/// Completes the last word of `line` from `names`. The first word is
/// a command; later words depend on the command: `set` and `get` take
/// variables, `help` a command, `bind` a key then a command, `log` a
/// level, `panels` a panel. `custom` supplies candidates for commands that
/// register a completer, given the command name and the words after it.
pub fn complete(
    line: &str,
    names: &Names<'_>,
    custom: &dyn Fn(&str, &[String]) -> Option<Vec<String>>,
) -> Completion {
    let (start, partial) = last_word(line);
    let before: Vec<&str> = line[..start].split_whitespace().collect();
    let pool: Vec<String> = match before.as_slice() {
        [] => names.commands.iter().chain(names.vars).cloned().collect(),
        [cmd, rest @ ..] => {
            let args: Vec<String> = rest
                .iter()
                .map(|s| (*s).to_owned())
                .chain(std::iter::once(partial.to_owned()))
                .collect();
            if let Some(c) = custom(cmd, &args) {
                c
            } else {
                match (*cmd, rest.len()) {
                    ("set" | "get", 0) | ("vars", 0) => names.vars.to_vec(),
                    ("help", 0) => names.commands.to_vec(),
                    ("bind", 0) | ("unbind", 0) => names.keys.iter().map(|k| (*k).to_owned()).collect(),
                    ("bind", 1) => names.commands.to_vec(),
                    ("log", 0) => ["debug", "info", "warn", "error"].map(String::from).to_vec(),
                    ("panels", 0) => names.panels.to_vec(),
                    _ => Vec::new(),
                }
            }
        }
    };
    let lower = partial.to_ascii_lowercase();
    let mut candidates: Vec<String> = pool
        .into_iter()
        .filter(|n| n.to_ascii_lowercase().starts_with(&lower))
        .collect();
    candidates.sort();
    candidates.dedup();
    let common = common_prefix(&candidates);
    Completion {
        start,
        candidates,
        common,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(line: &str) -> Completion {
        let commands: Vec<String> = ["help", "set", "get", "spawn", "screenshot", "bind", "log", "panels"]
            .map(String::from)
            .to_vec();
        let vars: Vec<String> = ["player.speed", "player.name", "world.seed"].map(String::from).to_vec();
        let panels: Vec<String> = ["Engine", "Physics"].map(String::from).to_vec();
        let keys = ["Space", "F5", "Grave"];
        let names = Names {
            commands: &commands,
            vars: &vars,
            keys: &keys,
            panels: &panels,
        };
        complete(line, &names, &|cmd, args| {
            (cmd == "spawn" && args.len() == 1).then(|| vec!["goblin".to_owned(), "gnome".to_owned()])
        })
    }

    #[test]
    fn completes_command_names_with_a_common_prefix() {
        let c = run("s");
        assert_eq!(c.candidates, ["screenshot", "set", "spawn"]);
        assert_eq!(c.common, "s");
        let c = run("scr");
        assert_eq!(c.candidates, ["screenshot"]);
        assert_eq!(c.apply("scr"), "screenshot ");
    }

    #[test]
    fn variable_names_follow_set_and_get() {
        let c = run("set pl");
        assert_eq!(c.candidates, ["player.name", "player.speed"]);
        assert_eq!(c.common, "player.");
        assert_eq!(c.apply("set pl"), "set player.");
        assert_eq!(run("get w").candidates, ["world.seed"]);
        assert_eq!(run("set player.speed ").candidates, Vec::<String>::new());
    }

    #[test]
    fn arguments_depend_on_the_command() {
        assert_eq!(run("bind F").candidates, ["F5"]);
        assert_eq!(run("bind F5 sp").candidates, ["spawn"]);
        assert_eq!(run("help s").candidates, ["screenshot", "set", "spawn"]);
        assert_eq!(run("log w").candidates, ["warn"]);
        assert_eq!(run("panels ph").candidates, ["Physics"]);
        assert_eq!(run("spawn g").candidates, ["gnome", "goblin"]);
        assert_eq!(run("spawn g").common, "g");
    }

    #[test]
    fn matching_ignores_case_and_nothing_matches_cleanly() {
        assert_eq!(run("BIN").candidates, ["bind"]);
        let none = run("zzz");
        assert!(none.candidates.is_empty() && none.common.is_empty());
        assert_eq!(none.apply("zzz"), "zzz");
    }

    #[test]
    fn variable_names_also_complete_at_the_start() {
        assert_eq!(run("world").candidates, ["world.seed"]);
    }
}
