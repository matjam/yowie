//! Bounds-checked reads for the module loaders. A truncated or corrupt file is an
//! error, never a panic.

use crate::error::{Error, Result};

#[derive(Clone, Copy)]
pub(crate) struct Bytes<'a>(pub &'a [u8]);

impl<'a> Bytes<'a> {
    pub(crate) fn len(&self) -> usize {
        self.0.len()
    }

    /// `len` bytes at `off`.
    pub(crate) fn slice(&self, off: usize, len: usize) -> Result<&'a [u8]> {
        off.checked_add(len)
            .and_then(|end| self.0.get(off..end))
            .ok_or_else(|| Error::Module(format!("file truncated at byte {off}")))
    }

    /// The bytes from `off` to the end of the file.
    pub(crate) fn from(&self, off: usize) -> Result<&'a [u8]> {
        self.0
            .get(off..)
            .ok_or_else(|| Error::Module(format!("offset {off} is past the end of the file")))
    }

    pub(crate) fn u8(&self, off: usize) -> Result<u8> {
        self.0
            .get(off)
            .copied()
            .ok_or_else(|| Error::Module(format!("file truncated at byte {off}")))
    }

    pub(crate) fn u16(&self, off: usize) -> Result<u16> {
        let b = self.slice(off, 2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }

    pub(crate) fn u32(&self, off: usize) -> Result<u32> {
        let b = self.slice(off, 4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    pub(crate) fn u16_be(&self, off: usize) -> Result<u16> {
        let b = self.slice(off, 2)?;
        Ok(u16::from_be_bytes([b[0], b[1]]))
    }
}

pub(crate) fn bad(msg: impl Into<String>) -> Error {
    Error::Module(msg.into())
}
