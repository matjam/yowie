//! KTX2 container writer and reader for single 2D block-compressed textures.

use crate::error::{Error, Result};
use crate::format::{BlockFormat, mip_count, mip_size};
use ktx2::dfd::{Basic, Block};
use ktx2::{Header, Index, LevelIndex, Reader};

/// Every level starts at a multiple of this many bytes; 16 is a multiple of
/// every block size written here, as the specification asks.
const LEVEL_ALIGN: usize = 16;

/// One 2D texture as stored in a KTX2 file: a Vulkan format, a size and the
/// bytes of each mip level.
///
/// `levels[0]` is the full-size image and each following level halves both
/// dimensions down to 1x1. Level bytes are the format's blocks in row order,
/// ready to upload to a GPU texture of the same format without conversion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ktx2File {
    /// The Vulkan format of the blocks.
    pub format: ktx2::Format,
    /// Width of level 0 in texels.
    pub width: u32,
    /// Height of level 0 in texels.
    pub height: u32,
    /// True when the colour channels are already multiplied by alpha.
    pub premultiplied: bool,
    /// The bytes of each mip level, level 0 first.
    pub levels: Vec<Vec<u8>>,
}

impl Ktx2File {
    /// The block format and sRGB flag of [`format`](Self::format), or `None`
    /// for a format outside BC1, BC3, BC4, BC5 and BC7.
    pub fn block_format(&self) -> Option<(BlockFormat, bool)> {
        BlockFormat::from_vk(self.format)
    }

    /// Size in texels of mip `level`.
    pub fn level_size(&self, level: u32) -> (u32, u32) {
        mip_size(self.width, self.height, level)
    }

    /// Serializes the file. Levels are written smallest first, as the
    /// specification recommends, with the level index in level order.
    ///
    /// Returns an error when the level count or any level length does not
    /// match the format and size.
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let (block, _) = self
            .block_format()
            .ok_or_else(|| Error::Ktx2(format!("format {:?} cannot be written", self.format)))?;
        if self.width == 0 || self.height == 0 || self.levels.is_empty() {
            return Err(Error::Ktx2(
                "an image needs a size and at least one level".into(),
            ));
        }
        if self.levels.len() as u32 > mip_count(self.width, self.height) {
            return Err(Error::Ktx2(format!(
                "{} levels exceed the chain for {}x{}",
                self.levels.len(),
                self.width,
                self.height
            )));
        }
        for (i, data) in self.levels.iter().enumerate() {
            let (w, h) = self.level_size(i as u32);
            let want = block.level_bytes(w, h);
            if data.len() != want {
                return Err(Error::Ktx2(format!(
                    "level {i} is {} bytes, want {want} for {w}x{h}",
                    data.len()
                )));
            }
        }
        let (basic, type_size) =
            Basic::from_format_with(self.format, self.premultiplied, None, None, None)
                .map_err(|e| Error::Ktx2(format!("{e:?}")))?;
        let block_bytes = Block::Basic(basic).to_vec();
        let mut dfd = ((block_bytes.len() + 4) as u32).to_le_bytes().to_vec();
        dfd.extend_from_slice(&block_bytes);
        let kvd = key_values();

        let count = self.levels.len();
        let dfd_offset = Header::LENGTH + count * LevelIndex::LENGTH;
        let kvd_offset = dfd_offset + dfd.len();
        let header = Header {
            format: Some(self.format),
            type_size,
            pixel_width: self.width,
            pixel_height: self.height,
            pixel_depth: 0,
            layer_count: 0,
            face_count: 1,
            level_count: count as u32,
            supercompression_scheme: None,
            index: Index {
                dfd_byte_offset: dfd_offset as u32,
                dfd_byte_length: dfd.len() as u32,
                kvd_byte_offset: kvd_offset as u32,
                kvd_byte_length: kvd.len() as u32,
                sgd_byte_offset: 0,
                sgd_byte_length: 0,
            },
        };
        let mut out = header.as_bytes().to_vec();
        out.resize(dfd_offset, 0);
        out.extend_from_slice(&dfd);
        out.extend_from_slice(&kvd);
        for i in (0..count).rev() {
            out.resize(out.len().next_multiple_of(LEVEL_ALIGN), 0);
            let index = LevelIndex {
                byte_offset: out.len() as u64,
                byte_length: self.levels[i].len() as u64,
                uncompressed_byte_length: self.levels[i].len() as u64,
            };
            let at = Header::LENGTH + i * LevelIndex::LENGTH;
            out[at..at + LevelIndex::LENGTH].copy_from_slice(&index.as_bytes());
            out.extend_from_slice(&self.levels[i]);
        }
        Ok(out)
    }

    /// Parses a KTX2 file holding one 2D image with no supercompression.
    ///
    /// Returns an error for malformed input, for arrays, cube maps and 3D
    /// textures, for supercompressed levels, and when a level's length does
    /// not match its format and size. Any Vulkan format is accepted, so a
    /// caller reading an uncompressed file checks [`block_format`](Self::block_format)
    /// before assuming BC blocks.
    pub fn parse(data: &[u8]) -> Result<Self> {
        let reader = Reader::new(data).map_err(|e| Error::Ktx2(e.to_string()))?;
        let h = reader.header();
        let format = h
            .format
            .ok_or_else(|| Error::Ktx2("format is undefined (a Basis Universal file?)".into()))?;
        if let Some(s) = h.supercompression_scheme {
            return Err(Error::Ktx2(format!("supercompression {s:?} is not read")));
        }
        if h.pixel_depth > 1 || h.layer_count > 1 || h.face_count != 1 {
            return Err(Error::Ktx2(format!(
                "only a single 2D image is read, not depth {}, {} layers, {} faces",
                h.pixel_depth, h.layer_count, h.face_count
            )));
        }
        let width = h.pixel_width;
        let height = h.pixel_height.max(1);
        let count = h.level_count.max(1);
        if count > mip_count(width, height) {
            return Err(Error::Ktx2(format!(
                "{count} levels exceed the chain for {width}x{height}"
            )));
        }
        let mut file = Ktx2File {
            format,
            width,
            height,
            premultiplied: reader.is_alpha_premultiplied().unwrap_or(false),
            levels: Vec::new(),
        };
        for (i, level) in reader.levels().enumerate() {
            if let Some((block, _)) = file.block_format() {
                let (w, h) = file.level_size(i as u32);
                let want = block.level_bytes(w, h);
                if level.data.len() != want {
                    return Err(Error::Ktx2(format!(
                        "level {i} is {} bytes, want {want} for {w}x{h}",
                        level.data.len()
                    )));
                }
            }
            file.levels.push(level.data.to_vec());
        }
        Ok(file)
    }

    /// Reads and parses a KTX2 file from disk.
    pub fn open(path: impl AsRef<std::path::Path>) -> Result<Self> {
        Self::parse(&std::fs::read(path)?)
    }
}

/// Key/value data naming the writer and the row order, sorted by key and
/// padded to four bytes as the specification requires.
fn key_values() -> Vec<u8> {
    let entries = [("KTXorientation", "rd"), ("KTXwriter", "yowie-tools")];
    let mut out = Vec::new();
    for (key, value) in entries {
        let mut body = key.as_bytes().to_vec();
        body.push(0);
        body.extend_from_slice(value.as_bytes());
        body.push(0);
        out.extend_from_slice(&(body.len() as u32).to_le_bytes());
        out.extend_from_slice(&body);
        out.resize(out.len().next_multiple_of(4), 0);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(block: BlockFormat, srgb: bool, w: u32, h: u32) -> Ktx2File {
        let levels = (0..mip_count(w, h))
            .map(|i| {
                let (lw, lh) = mip_size(w, h, i);
                (0..block.level_bytes(lw, lh))
                    .map(|b| (b + i as usize) as u8)
                    .collect()
            })
            .collect();
        Ktx2File {
            format: block.vk_format(srgb),
            width: w,
            height: h,
            premultiplied: false,
            levels,
        }
    }

    #[test]
    fn round_trip() {
        for (block, srgb, w, h) in [
            (BlockFormat::Bc7, true, 64, 32),
            (BlockFormat::Bc1, false, 5, 3),
            (BlockFormat::Bc4, false, 1, 1),
            (BlockFormat::Bc5, false, 16, 16),
            (BlockFormat::Bc3, true, 7, 40),
        ] {
            let mut file = sample(block, srgb, w, h);
            file.premultiplied = block == BlockFormat::Bc3;
            let bytes = file.to_bytes().unwrap();
            assert_eq!(Ktx2File::parse(&bytes).unwrap(), file, "{block:?} {w}x{h}");
        }
    }

    #[test]
    fn header_fields() {
        let bytes = sample(BlockFormat::Bc7, true, 8, 8).to_bytes().unwrap();
        assert_eq!(&bytes[..12], &ktx2::MAGIC);
        assert_eq!(u32::from_le_bytes(bytes[12..16].try_into().unwrap()), 146);
        assert_eq!(u32::from_le_bytes(bytes[20..24].try_into().unwrap()), 8);
        assert_eq!(u32::from_le_bytes(bytes[40..44].try_into().unwrap()), 4);
        let reader = Reader::new(&bytes[..]).unwrap();
        assert_eq!(
            reader.transfer_function(),
            Some(ktx2::TransferFunction::SRGB)
        );
        assert_eq!(reader.color_model(), Some(ktx2::ColorModel::BC7));
    }

    #[test]
    fn rejects_bad_levels() {
        let mut file = sample(BlockFormat::Bc7, false, 8, 8);
        file.levels[1].pop();
        assert!(file.to_bytes().is_err());
        let mut file = sample(BlockFormat::Bc7, false, 8, 8);
        file.levels.push(vec![0; 16]);
        file.levels.push(vec![0; 16]);
        assert!(file.to_bytes().is_err());
        assert!(Ktx2File::parse(b"not a ktx2 file at all, nope").is_err());
    }
}
