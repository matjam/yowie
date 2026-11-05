//! The system clipboard, or a private one for headless runs.

/// Clipboard access. The system clipboard is opened on first use, so a
/// game that never touches it never connects to it.
pub(crate) enum Clipboard {
    System(Option<arboard::Clipboard>),
    Memory(String),
}

impl std::fmt::Debug for Clipboard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Clipboard::System(c) => write!(f, "Clipboard::System(open: {})", c.is_some()),
            Clipboard::Memory(_) => f.write_str("Clipboard::Memory"),
        }
    }
}

impl Clipboard {
    pub(crate) fn system() -> Self {
        Clipboard::System(None)
    }

    pub(crate) fn memory() -> Self {
        Clipboard::Memory(String::new())
    }

    fn open(slot: &mut Option<arboard::Clipboard>) -> Result<&mut arboard::Clipboard, String> {
        if slot.is_none() {
            *slot = Some(arboard::Clipboard::new().map_err(|e| e.to_string())?);
        }
        slot.as_mut()
            .ok_or_else(|| "clipboard unavailable".to_owned())
    }

    pub(crate) fn get(&mut self) -> Option<String> {
        match self {
            Clipboard::Memory(s) => (!s.is_empty()).then(|| s.clone()),
            Clipboard::System(slot) => match Self::open(slot) {
                Ok(c) => c.get_text().ok(),
                Err(e) => {
                    log::warn!("yowie: clipboard: {e}");
                    None
                }
            },
        }
    }

    pub(crate) fn set(&mut self, text: &str) -> Result<(), String> {
        match self {
            Clipboard::Memory(s) => {
                text.clone_into(s);
                Ok(())
            }
            Clipboard::System(slot) => Self::open(slot)?
                .set_text(text.to_owned())
                .map_err(|e| e.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_clipboard_round_trips() {
        let mut c = Clipboard::memory();
        assert_eq!(c.get(), None);
        assert!(c.set("hello").is_ok());
        assert_eq!(c.get().as_deref(), Some("hello"));
    }
}
