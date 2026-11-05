//! Colours, measures and skins.

use yowie_core::Color;
use yowie_gfx::NineSlice;

/// Nine-slice textures that replace the flat rectangles widgets draw.
/// Every slice is optional; a widget without one draws flat colours.
/// Slices are tinted by the widget's state, so draw them in light colours.
#[derive(Clone, Debug, Default)]
pub struct Skin {
    /// Panels and windows.
    pub panel: Option<NineSlice>,
    /// Window title bars.
    pub title: Option<NineSlice>,
    /// Idle buttons.
    pub button: Option<NineSlice>,
    /// Hovered buttons; falls back to `button` brightened.
    pub button_hover: Option<NineSlice>,
    /// Pressed buttons; falls back to `button` darkened.
    pub button_active: Option<NineSlice>,
    /// Text fields and list backgrounds.
    pub field: Option<NineSlice>,
    /// Slider, progress and scrollbar tracks.
    pub track: Option<NineSlice>,
    /// The filled part of sliders and progress bars, and scrollbar thumbs.
    pub fill: Option<NineSlice>,
}

/// The few colours a [`Theme`] derives from.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    /// Panel background.
    pub background: Color,
    /// Buttons and tracks sit on this.
    pub surface: Color,
    /// Outlines and separators.
    pub border: Color,
    /// Ordinary text.
    pub text: Color,
    /// Secondary text and placeholders.
    pub text_dim: Color,
    /// Selections, focus rings and filled controls.
    pub accent: Color,
    /// Window titles.
    pub title: Color,
}

const fn pal(
    bg: u32,
    surface: u32,
    border: u32,
    text: u32,
    dim: u32,
    accent: u32,
    title: u32,
) -> Palette {
    Palette {
        background: Color::hex(bg),
        surface: Color::hex(surface),
        border: Color::hex(border),
        text: Color::hex(text),
        text_dim: Color::hex(dim),
        accent: Color::hex(accent),
        title: Color::hex(title),
    }
}

/// The built-in palettes by name, in menu order.
pub const PALETTES: &[(&str, Palette)] = &[
    (
        "dark",
        pal(
            0x1E2028, 0x3A3E4E, 0x464A5A, 0xE6E6EB, 0x9696A5, 0x78BEFF, 0xFFD278,
        ),
    ),
    (
        "light",
        pal(
            0xF5F5F8, 0xDCDEE6, 0xBEBEC8, 0x1E1E23, 0x6E6E78, 0x286EC8, 0x784600,
        ),
    ),
    (
        "nord",
        pal(
            0x2E3440, 0x3B4252, 0x4C566A, 0xECEFF4, 0xD8DEE9, 0x88C0D0, 0xEBCB8B,
        ),
    ),
    (
        "gruvbox",
        pal(
            0x282828, 0x3C3836, 0x504945, 0xEBDBB2, 0xA89984, 0x83A598, 0xFABD2F,
        ),
    ),
    (
        "solarized-dark",
        pal(
            0x002B36, 0x073642, 0x586E75, 0xEEE8D5, 0x93A1A1, 0x268BD2, 0xB58900,
        ),
    ),
    (
        "solarized-light",
        pal(
            0xFDF6E3, 0xEEE8D5, 0x93A1A1, 0x073642, 0x586E75, 0x268BD2, 0xB58900,
        ),
    ),
    (
        "sepia",
        pal(
            0xF4ECD8, 0xEADBC0, 0xB9A582, 0x4B3B2A, 0x7A6650, 0xB05A2A, 0x8B4513,
        ),
    ),
    (
        "high-contrast",
        pal(
            0x000000, 0x000000, 0xFFFFFF, 0xFFFFFF, 0xC0C0C0, 0xFFFF00, 0x00FFFF,
        ),
    ),
];

/// Every colour and measure widgets use. Measures are view units.
#[derive(Clone, Debug)]
pub struct Theme {
    /// Ordinary text and widget values.
    pub text: Color,
    /// Secondary text, placeholders and disabled content.
    pub text_dim: Color,
    /// Panel and window background.
    pub panel: Color,
    /// Outlines and separators.
    pub border: Color,
    /// Window titles.
    pub title: Color,
    /// Idle button fill.
    pub button: Color,
    /// Hovered button fill.
    pub button_hover: Color,
    /// Pressed button fill.
    pub button_active: Color,
    /// Selections, focus rings and filled controls.
    pub accent: Color,
    /// Text on an accent fill.
    pub on_accent: Color,
    /// Text field and list background.
    pub field: Color,
    /// Slider, progress and scrollbar background.
    pub track: Color,
    /// Dimming drawn behind modal dialogs.
    pub backdrop: Color,
    /// Space inside panels and around button text.
    pub padding: f32,
    /// Space between stacked widgets.
    pub spacing: f32,
    /// Outline thickness.
    pub border_width: f32,
    /// Minimum widget height.
    pub row_height: f32,
    /// Keyboard focus ring thickness.
    pub focus_width: f32,
    /// Interface text size.
    pub text_size: f32,
    /// Monospaced text size.
    pub mono_size: f32,
    /// Textures for widget backgrounds.
    pub skin: Option<Skin>,
}

fn shade(c: Color, f: f32) -> Color {
    Color::rgba(
        (c.r * f).min(1.0),
        (c.g * f).min(1.0),
        (c.b * f).min(1.0),
        c.a,
    )
}

fn luminance(c: Color) -> f32 {
    0.2126 * c.r + 0.7152 * c.g + 0.0722 * c.b
}

impl Theme {
    /// Derives a theme: buttons brighten (or darken, on a light
    /// background) as they are hovered, fields sit below the background.
    pub fn from_palette(p: Palette) -> Theme {
        let dark = luminance(p.background) < 0.5;
        let lift = |c: Color, amount: f32| shade(c, if dark { 1.0 + amount } else { 1.0 - amount });
        let mut bg = p.background;
        if bg.a >= 1.0 {
            bg.a = 0.96;
        }
        Theme {
            text: p.text,
            text_dim: p.text_dim,
            panel: bg,
            border: p.border,
            title: p.title,
            button: p.surface,
            button_hover: lift(p.surface, 0.18),
            button_active: p.accent.with_alpha(0.75),
            accent: p.accent,
            on_accent: if luminance(p.accent) > 0.5 {
                Color::BLACK
            } else {
                Color::WHITE
            },
            field: lift(p.background, -0.15),
            track: lift(p.surface, -0.1),
            backdrop: Color::rgba(0.0, 0.0, 0.0, 0.5),
            padding: 8.0,
            spacing: 6.0,
            border_width: 1.0,
            row_height: 26.0,
            focus_width: 2.0,
            text_size: 15.0,
            mono_size: 13.0,
            skin: None,
        }
    }

    /// The built-in theme `name` (see [`theme_names`]).
    pub fn named(name: &str) -> Option<Theme> {
        let (_, p) = PALETTES.iter().find(|(n, _)| *n == name)?;
        let mut t = Theme::from_palette(*p);
        if name == "high-contrast" {
            t.border_width = 2.0;
            t.focus_width = 4.0;
            t.panel.a = 1.0;
        }
        Some(t)
    }

    /// The dark default theme.
    pub fn dark() -> Theme {
        Theme::from_palette(PALETTES[0].1)
    }

    /// The same theme drawing with `skin`.
    pub fn with_skin(mut self, skin: Skin) -> Theme {
        self.skin = Some(skin);
        self
    }
}

impl Default for Theme {
    fn default() -> Self {
        Theme::dark()
    }
}

/// The built-in theme names in menu order.
pub fn theme_names() -> impl Iterator<Item = &'static str> {
    PALETTES.iter().map(|(n, _)| *n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_builtin_theme_builds() {
        for name in theme_names() {
            let t = Theme::named(name).expect(name);
            assert!(t.row_height > 0.0);
        }
        assert!(Theme::named("nord").is_some());
        assert!(Theme::named("plaid").is_none());
    }

    #[test]
    fn nord_uses_its_published_colours() {
        let t = Theme::named("nord").expect("nord exists");
        assert_eq!(t.accent, Color::hex(0x88C0D0));
        assert_eq!(t.text, Color::hex(0xECEFF4));
    }
}
