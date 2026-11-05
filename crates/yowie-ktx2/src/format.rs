//! The block formats `yowie-tex` writes and the size arithmetic of mip chains.

use crate::error::{Error, Result};
use ktx2::Format;

/// A block-compressed texture format that `yowie-tex` can write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockFormat {
    /// 8 bytes per 4x4 block, opaque colour.
    Bc1,
    /// 16 bytes per block, colour with 8-bit alpha.
    Bc3,
    /// 8 bytes per block, one channel (the red channel of the source).
    Bc4,
    /// 16 bytes per block, two channels (red and green), for normal maps.
    Bc5,
    /// 16 bytes per block, high-quality colour with optional alpha.
    Bc7,
}

impl BlockFormat {
    /// Parses `bc1`, `bc3`, `bc4`, `bc5` or `bc7`, ignoring case.
    pub fn parse(name: &str) -> Result<Self> {
        match name.to_ascii_lowercase().as_str() {
            "bc1" => Ok(Self::Bc1),
            "bc3" => Ok(Self::Bc3),
            "bc4" => Ok(Self::Bc4),
            "bc5" => Ok(Self::Bc5),
            "bc7" => Ok(Self::Bc7),
            _ => Err(Error::Invalid(format!(
                "no format {name:?}; use bc1, bc3, bc4, bc5 or bc7"
            ))),
        }
    }

    /// The lower-case name, such as `bc7`.
    pub fn name(self) -> &'static str {
        match self {
            Self::Bc1 => "bc1",
            Self::Bc3 => "bc3",
            Self::Bc4 => "bc4",
            Self::Bc5 => "bc5",
            Self::Bc7 => "bc7",
        }
    }

    /// Bytes in one 4x4 block.
    pub fn block_bytes(self) -> usize {
        match self {
            Self::Bc1 | Self::Bc4 => 8,
            _ => 16,
        }
    }

    /// Whether the format carries an alpha channel.
    pub fn has_alpha(self) -> bool {
        matches!(self, Self::Bc3 | Self::Bc7)
    }

    /// Whether the format holds colour, which can be sRGB. BC4 and BC5 hold
    /// data and are always linear.
    pub fn is_color(self) -> bool {
        matches!(self, Self::Bc1 | Self::Bc3 | Self::Bc7)
    }

    /// Size in bytes of a mip level of `width` by `height` texels.
    pub fn level_bytes(self, width: u32, height: u32) -> usize {
        let bx = width.div_ceil(4) as usize;
        let by = height.div_ceil(4) as usize;
        bx * by * self.block_bytes()
    }

    /// The Vulkan format for this block format. `srgb` is ignored for BC4
    /// and BC5, which have no sRGB variant.
    pub fn vk_format(self, srgb: bool) -> Format {
        match (self, srgb) {
            (Self::Bc1, true) => Format::BC1_RGB_SRGB_BLOCK,
            (Self::Bc1, false) => Format::BC1_RGB_UNORM_BLOCK,
            (Self::Bc3, true) => Format::BC3_SRGB_BLOCK,
            (Self::Bc3, false) => Format::BC3_UNORM_BLOCK,
            (Self::Bc4, _) => Format::BC4_UNORM_BLOCK,
            (Self::Bc5, _) => Format::BC5_UNORM_BLOCK,
            (Self::Bc7, true) => Format::BC7_SRGB_BLOCK,
            (Self::Bc7, false) => Format::BC7_UNORM_BLOCK,
        }
    }

    /// The block format and sRGB flag behind a Vulkan format, or `None` when
    /// the format is not one of those this crate writes (BC1 with alpha is
    /// read as BC1).
    pub fn from_vk(format: Format) -> Option<(Self, bool)> {
        Some(match format {
            Format::BC1_RGB_SRGB_BLOCK | Format::BC1_RGBA_SRGB_BLOCK => (Self::Bc1, true),
            Format::BC1_RGB_UNORM_BLOCK | Format::BC1_RGBA_UNORM_BLOCK => (Self::Bc1, false),
            Format::BC3_SRGB_BLOCK => (Self::Bc3, true),
            Format::BC3_UNORM_BLOCK => (Self::Bc3, false),
            Format::BC4_UNORM_BLOCK => (Self::Bc4, false),
            Format::BC5_UNORM_BLOCK => (Self::Bc5, false),
            Format::BC7_SRGB_BLOCK => (Self::Bc7, true),
            Format::BC7_UNORM_BLOCK => (Self::Bc7, false),
            _ => return None,
        })
    }
}

/// Number of mip levels in a full chain for the given size, down to 1x1.
pub fn mip_count(width: u32, height: u32) -> u32 {
    32 - width.max(height).max(1).leading_zeros()
}

/// Size of mip `level` for a base of `width` by `height`; never below 1.
pub fn mip_size(width: u32, height: u32, level: u32) -> (u32, u32) {
    ((width >> level).max(1), (height >> level).max(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mip_chain_lengths() {
        for (w, h, n) in [(1, 1, 1), (2, 2, 2), (256, 256, 9), (256, 64, 9), (5, 3, 3)] {
            assert_eq!(mip_count(w, h), n, "{w}x{h}");
        }
        assert_eq!(mip_size(256, 64, 7), (2, 1));
    }

    #[test]
    fn vk_format_round_trip() {
        for name in ["bc1", "bc3", "bc4", "bc5", "bc7"] {
            let f = BlockFormat::parse(name).unwrap();
            for srgb in [false, true] {
                let (back, s) = BlockFormat::from_vk(f.vk_format(srgb)).unwrap();
                assert_eq!(back, f);
                assert_eq!(s, srgb && f.is_color());
            }
        }
        assert!(BlockFormat::parse("bc6").is_err());
    }

    #[test]
    fn level_sizes() {
        assert_eq!(BlockFormat::Bc7.level_bytes(1, 1), 16);
        assert_eq!(BlockFormat::Bc1.level_bytes(5, 4), 16);
    }
}
