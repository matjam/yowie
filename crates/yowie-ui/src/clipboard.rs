//! Clipboard access for text fields.

/// What text fields cut, copy and paste through. The engine installs one
/// backed by the system clipboard.
pub trait Clipboard {
    /// The clipboard's text, if it holds any.
    fn get(&mut self) -> Option<String>;
    /// Replaces the clipboard's text.
    fn set(&mut self, text: &str);
}

/// A clipboard that keeps its text in memory, the default for a new
/// [`Ui`](crate::Ui) and for headless runs.
#[derive(Clone, Debug, Default)]
pub struct MemoryClipboard {
    text: String,
}

impl Clipboard for MemoryClipboard {
    fn get(&mut self) -> Option<String> {
        (!self.text.is_empty()).then(|| self.text.clone())
    }

    fn set(&mut self, text: &str) {
        self.text = text.to_owned();
    }
}
