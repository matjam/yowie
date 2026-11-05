//! Command history with Up and Down navigation.

const LIMIT: usize = 200;

/// The commands run so far, newest last, and the position of a walk back
/// through them.
#[derive(Clone, Debug, Default)]
pub struct History {
    entries: Vec<String>,
    /// Index into `entries` while walking; `None` when at the live line.
    cursor: Option<usize>,
    /// What was typed before the walk began, restored by walking forward.
    draft: String,
}

impl History {
    /// Records a command. Blank lines and a repeat of the previous
    /// command are not kept. Ends any walk.
    pub fn push(&mut self, line: &str) {
        self.reset();
        let line = line.trim();
        if line.is_empty() || self.entries.last().is_some_and(|l| l == line) {
            return;
        }
        self.entries.push(line.to_owned());
        if self.entries.len() > LIMIT {
            self.entries.remove(0);
        }
    }

    /// Ends a walk without changing the entries.
    pub fn reset(&mut self) {
        self.cursor = None;
        self.draft.clear();
    }

    /// Steps back to the previous command. `current` is the line being
    /// typed, kept so [`forward`](Self::forward) can restore it.
    pub fn back(&mut self, current: &str) -> Option<&str> {
        let next = match self.cursor {
            None if self.entries.is_empty() => return None,
            None => {
                self.draft = current.to_owned();
                self.entries.len() - 1
            }
            Some(0) => 0,
            Some(i) => i - 1,
        };
        self.cursor = Some(next);
        self.entries.get(next).map(String::as_str)
    }

    /// Steps forward to the next command, or to the line that was being
    /// typed when the walk began.
    pub fn forward(&mut self) -> Option<&str> {
        let i = self.cursor?;
        if i + 1 < self.entries.len() {
            self.cursor = Some(i + 1);
            self.entries.get(i + 1).map(String::as_str)
        } else {
            self.cursor = None;
            Some(self.draft.as_str())
        }
    }

    /// Every command, oldest first.
    pub fn entries(&self) -> &[String] {
        &self.entries
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn walks_back_and_forward_with_the_draft() {
        let mut h = History::default();
        for l in ["one", "two", "three"] {
            h.push(l);
        }
        assert_eq!(h.back("dra"), Some("three"));
        assert_eq!(h.back("ignored"), Some("two"));
        assert_eq!(h.back(""), Some("one"));
        assert_eq!(h.back(""), Some("one"), "stops at the oldest");
        assert_eq!(h.forward(), Some("two"));
        assert_eq!(h.forward(), Some("three"));
        assert_eq!(h.forward(), Some("dra"), "the draft returns");
        assert_eq!(h.forward(), None);
    }

    #[test]
    fn skips_blank_and_repeated_lines() {
        let mut h = History::default();
        for l in ["a", "a", "  ", "b", "a"] {
            h.push(l);
        }
        assert_eq!(h.entries(), ["a", "b", "a"]);
    }

    #[test]
    fn pushing_ends_the_walk_and_the_list_is_capped() {
        let mut h = History::default();
        h.push("x");
        h.back("");
        h.push("y");
        assert_eq!(h.back("z"), Some("y"));
        for i in 0..300 {
            h.push(&format!("cmd {i}"));
        }
        assert_eq!(h.entries().len(), LIMIT);
        assert_eq!(h.entries().last().map(String::as_str), Some("cmd 299"));
        assert_eq!(History::default().back(""), None);
    }
}
