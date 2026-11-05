//! Window icons and custom cursor images, and their conversion to winit's
//! types.

use crate::{Error, Result};

/// An RGBA image used as a window icon.
///
/// Pixels are 8-bit RGBA with straight alpha, rows from the top. Platform
/// support varies: Windows and X11 show the icon in the title bar and task
/// bar; macOS takes its icon from the application bundle, so the icon is
/// ignored there (set the Dock icon in the bundle, see `yowie-bundle`);
/// Wayland has no protocol for a client-set icon (the compositor looks the
/// icon up from the desktop entry that matches the window's app id), so it
/// is ignored there too.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Icon {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

impl Icon {
    /// Builds an icon from `width * height` RGBA8 pixels. Fails when the
    /// buffer length does not match or the size is zero.
    pub fn from_rgba(width: u32, height: u32, rgba: Vec<u8>) -> Result<Self> {
        check_rgba(width, height, rgba.len())?;
        Ok(Self {
            width,
            height,
            rgba,
        })
    }

    /// Decodes a PNG or JPEG file's bytes into an icon.
    pub fn from_image_bytes(bytes: &[u8]) -> Result<Self> {
        let image = image::load_from_memory(bytes)
            .map_err(|e| Error::Window(format!("cannot decode icon: {e}")))?
            .to_rgba8();
        Self::from_rgba(image.width(), image.height(), image.into_raw())
    }

    /// Width in pixels.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Height in pixels.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// The RGBA8 pixels.
    pub fn rgba(&self) -> &[u8] {
        &self.rgba
    }

    pub(crate) fn to_winit(&self) -> Result<winit::window::Icon> {
        winit::window::Icon::from_rgba(self.rgba.clone(), self.width, self.height)
            .map_err(|e| Error::Window(format!("bad icon: {e}")))
    }
}

fn check_rgba(width: u32, height: u32, len: usize) -> Result<()> {
    let expected = width as usize * height as usize * 4;
    if width == 0 || height == 0 {
        return Err(Error::Window("an image needs a size".into()));
    }
    if len != expected {
        return Err(Error::Window(format!(
            "{len} bytes of pixels, expected {expected} for {width}x{height}"
        )));
    }
    Ok(())
}

/// A custom mouse cursor: an RGBA image and the pixel that sits on the
/// pointer's position.
///
/// Pixels are 8-bit RGBA with straight alpha. Windows, macOS, X11 and
/// Wayland all accept images; the largest size they accept varies, and
/// 64 by 64 pixels works everywhere.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CursorImage {
    width: u16,
    height: u16,
    hotspot: (u16, u16),
    rgba: Vec<u8>,
}

impl CursorImage {
    /// Builds a cursor from `width * height` RGBA8 pixels whose hotspot is
    /// pixel `(hotspot_x, hotspot_y)`. Fails for a size of zero, a size
    /// above 65535, a wrong buffer length or a hotspot outside the image.
    pub fn from_rgba(width: u32, height: u32, hotspot: (u32, u32), rgba: Vec<u8>) -> Result<Self> {
        check_rgba(width, height, rgba.len())?;
        let narrow = |v: u32| {
            u16::try_from(v).map_err(|_| Error::Window(format!("{v} is too large for a cursor")))
        };
        let (w, h) = (narrow(width)?, narrow(height)?);
        if hotspot.0 >= width || hotspot.1 >= height {
            return Err(Error::Window(format!(
                "hotspot {hotspot:?} lies outside the {width}x{height} cursor"
            )));
        }
        Ok(Self {
            width: w,
            height: h,
            hotspot: (narrow(hotspot.0)?, narrow(hotspot.1)?),
            rgba,
        })
    }

    /// Decodes a PNG or JPEG file's bytes into a cursor.
    pub fn from_image_bytes(bytes: &[u8], hotspot: (u32, u32)) -> Result<Self> {
        let image = image::load_from_memory(bytes)
            .map_err(|e| Error::Window(format!("cannot decode cursor: {e}")))?
            .to_rgba8();
        Self::from_rgba(image.width(), image.height(), hotspot, image.into_raw())
    }

    /// Width in pixels.
    pub fn width(&self) -> u32 {
        u32::from(self.width)
    }

    /// Height in pixels.
    pub fn height(&self) -> u32 {
        u32::from(self.height)
    }

    /// The hotspot in pixels from the top-left.
    pub fn hotspot(&self) -> (u32, u32) {
        (u32::from(self.hotspot.0), u32::from(self.hotspot.1))
    }

    pub(crate) fn to_source(&self) -> Result<winit::window::CustomCursorSource> {
        winit::window::CustomCursor::from_rgba(
            self.rgba.clone(),
            self.width,
            self.height,
            self.hotspot.0,
            self.hotspot.1,
        )
        .map_err(|e| Error::Window(format!("bad cursor image: {e}")))
    }
}

/// The pointer's appearance over the window.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Cursor {
    /// The platform's arrow.
    #[default]
    Arrow,
    /// A pointing hand, for links and buttons.
    Pointer,
    /// An I-beam, for editable text.
    Text,
    /// A cross, for precise picking.
    Crosshair,
    /// Four arrows: something can be moved.
    Move,
    /// An open hand: something can be grabbed.
    Grab,
    /// A closed hand: something is being dragged.
    Grabbing,
    /// A forbidden sign: the action is not allowed here.
    NotAllowed,
    /// The platform's busy indicator; input is blocked.
    Wait,
    /// The arrow with a busy indicator; input still works.
    Progress,
    /// An arrow with a question mark.
    Help,
    /// Arrows left and right: a column or a width can be resized.
    ResizeHorizontal,
    /// Arrows up and down: a row or a height can be resized.
    ResizeVertical,
    /// A magnifier with a plus.
    ZoomIn,
    /// A magnifier with a minus.
    ZoomOut,
    /// An image supplied by the game.
    Custom(CursorImage),
}

impl Cursor {
    /// The winit icon for a standard shape, or `None` for a custom image.
    pub(crate) fn icon(&self) -> Option<winit::window::CursorIcon> {
        use winit::window::CursorIcon as C;
        Some(match self {
            Cursor::Arrow => C::Default,
            Cursor::Pointer => C::Pointer,
            Cursor::Text => C::Text,
            Cursor::Crosshair => C::Crosshair,
            Cursor::Move => C::Move,
            Cursor::Grab => C::Grab,
            Cursor::Grabbing => C::Grabbing,
            Cursor::NotAllowed => C::NotAllowed,
            Cursor::Wait => C::Wait,
            Cursor::Progress => C::Progress,
            Cursor::Help => C::Help,
            Cursor::ResizeHorizontal => C::EwResize,
            Cursor::ResizeVertical => C::NsResize,
            Cursor::ZoomIn => C::ZoomIn,
            Cursor::ZoomOut => C::ZoomOut,
            Cursor::Custom(_) => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn px(w: u32, h: u32) -> Vec<u8> {
        vec![255; (w * h * 4) as usize]
    }

    #[test]
    fn icon_validation() {
        let cases: [(u32, u32, usize, bool); 5] = [
            (4, 4, 64, true),
            (4, 4, 60, false),
            (0, 4, 0, false),
            (2, 1, 8, true),
            (2, 2, 8, false),
        ];
        for (w, h, len, ok) in cases {
            let icon = Icon::from_rgba(w, h, vec![0; len]);
            assert_eq!(icon.is_ok(), ok, "{w}x{h} with {len} bytes");
            if let Ok(icon) = icon {
                assert!(icon.to_winit().is_ok());
            }
        }
    }

    #[test]
    fn icon_decodes_png() {
        let mut png = Vec::new();
        image::RgbaImage::from_pixel(8, 4, image::Rgba([9, 8, 7, 255]))
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap_or_else(|e| panic!("encode: {e}"));
        let icon = Icon::from_image_bytes(&png).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!((icon.width(), icon.height()), (8, 4));
        assert_eq!(&icon.rgba()[..4], &[9, 8, 7, 255]);
        assert!(Icon::from_image_bytes(b"nope").is_err());
    }

    #[test]
    fn cursor_image_validation() {
        let cases: [(u32, u32, (u32, u32), bool); 5] = [
            (8, 8, (0, 0), true),
            (8, 8, (7, 7), true),
            (8, 8, (8, 0), false),
            (8, 8, (0, 9), false),
            (70_000, 1, (0, 0), false),
        ];
        for (w, h, hot, ok) in cases {
            let rgba = if w > 4096 { vec![0; 4] } else { px(w, h) };
            let cursor = CursorImage::from_rgba(w, h, hot, rgba);
            assert_eq!(cursor.is_ok(), ok, "{w}x{h} hotspot {hot:?}");
            if let Ok(c) = cursor {
                assert_eq!(c.hotspot(), hot);
                assert!(c.to_source().is_ok());
            }
        }
    }

    #[test]
    fn standard_shapes_map_and_custom_does_not() {
        use winit::window::CursorIcon as C;
        let cases = [
            (Cursor::Arrow, Some(C::Default)),
            (Cursor::Text, Some(C::Text)),
            (Cursor::Grabbing, Some(C::Grabbing)),
            (Cursor::ResizeHorizontal, Some(C::EwResize)),
        ];
        for (cursor, want) in cases {
            assert_eq!(cursor.icon(), want, "{cursor:?}");
        }
        let custom =
            CursorImage::from_rgba(2, 2, (0, 0), px(2, 2)).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(Cursor::Custom(custom).icon(), None);
    }
}
