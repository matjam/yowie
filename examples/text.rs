//! Shaped text: kerning and ligatures, Arabic joining, right-to-left and
//! mixed lines, a fallback chain, Unicode wrapping, hyphenation in thirteen
//! languages, rich markup, colour emoji, vertical text, distance-field text,
//! wrapping and alignment, and a caret placed by hit testing.
//!
//! `cargo run --example text -- --font file.ttf` sets the body text in that
//! font instead of Go Regular. Escape quits.

use yowie::gfx::{Font, Language, Orientation, TextAlign, TextStyle};
use yowie::input::Key;
use yowie::{Color, Config, Context, Game, GameResult, Rect, Vec2, vec2};

const ARABIC: &[u8] = include_bytes!("assets/fonts/NotoSansArabic-Regular.ttf");
const CJK: &[u8] = include_bytes!("assets/fonts/NotoSansCJKsc-subset.otf");
const EMOJI: &[u8] = include_bytes!("assets/fonts/NotoColorEmoji-subset.ttf");

const STORY: &str = "The yowie keeps to the ridges above the tree line, where the \
mist settles each evening. Walkers who stay on the track report footprints the size \
of a dinner plate.";

/// One long-word sentence per hyphenation language.
const HYPHEN: [(Language, &str, &str); 13] = [
    (
        Language::Danish,
        "da",
        "Kommunalbestyrelsesmedlemmernes forhandlingsgrundlag",
    ),
    (
        Language::German,
        "de",
        "Donaudampfschifffahrtsgesellschaft Rechtsschutzversicherung",
    ),
    (
        Language::English,
        "en",
        "Internationalization of uncharacteristically long words",
    ),
    (
        Language::Spanish,
        "es",
        "Electroencefalografista anticonstitucionalmente",
    ),
    (
        Language::Finnish,
        "fi",
        "Tietokoneohjelmistosuunnittelija epäjärjestelmällisyys",
    ),
    (
        Language::French,
        "fr",
        "Anticonstitutionnellement incompréhensibilité",
    ),
    (
        Language::Italian,
        "it",
        "Precipitevolissimevolmente straordinariamente",
    ),
    (
        Language::Norwegian,
        "nb",
        "Arbeidsgiverorganisasjonene selvfølgeligheten",
    ),
    (
        Language::Dutch,
        "nl",
        "Wetenschappelijk onderzoeksfinanciering verantwoordelijkheid",
    ),
    (
        Language::Polish,
        "pl",
        "Bezwzględniejszy nieprzejednanie przedsiębiorczość",
    ),
    (
        Language::Portuguese,
        "pt",
        "Inconstitucionalissimamente desenvolvimento extraordinário",
    ),
    (
        Language::Russian,
        "ru",
        "Достопримечательность неудовлетворительный",
    ),
    (
        Language::Swedish,
        "sv",
        "Sjukhusadministration oregelbundenhet förhandlingsresultat",
    ),
];

struct Text {
    body: Font,
    body_name: String,
    mono: Font,
    arabic: Font,
    emoji: Font,
    /// The body font with Arabic and CJK behind it.
    chain: Font,
}

fn user_font_path() -> Option<String> {
    let mut args = std::env::args();
    while let Some(a) = args.next() {
        if a == "--font" {
            return args.next();
        }
        if let Some(path) = a.strip_prefix("--font=") {
            return Some(path.to_owned());
        }
    }
    None
}

impl Game for Text {
    fn init(ctx: &mut Context) -> GameResult<Self> {
        let g = &mut ctx.gfx;
        let arabic = Font::from_bytes(g, ARABIC.to_vec())?;
        let cjk = Font::from_bytes(g, CJK.to_vec())?;
        let emoji = Font::from_bytes(g, EMOJI.to_vec())?;
        let (body, body_name) = match user_font_path() {
            Some(path) => {
                let f = Font::from_file(g, &path)?;
                let name = f.family().to_owned();
                (f, name)
            }
            None => (g.default_font(), "Go Regular".to_owned()),
        };
        let chain = body.with_fallback(&[arabic.clone(), cjk]);
        Ok(Text {
            mono: g.default_mono_font(),
            body,
            body_name,
            arabic,
            emoji,
            chain,
        })
    }

    fn update(&mut self, ctx: &mut Context) -> GameResult {
        if ctx.input.key_pressed(Key::Escape) {
            ctx.quit();
        }
        Ok(())
    }

    fn draw(&mut self, ctx: &mut Context) -> GameResult {
        let g = &mut ctx.gfx;
        self.left_column(g);
        self.middle_column(g);
        self.right_column(g);
        Ok(())
    }
}

const INK: Color = Color::rgb8(235, 232, 220);
const AMBER: Color = Color::rgb8(250, 200, 90);

impl Text {
    /// A framed box with a title; returns the top-left of its content.
    fn panel(&self, g: &mut yowie::gfx::Graphics, rect: Rect, title: &str) -> Vec2 {
        g.fill_rect(rect, Color::rgb8(30, 34, 48));
        g.stroke_rect(rect, 1.0, Color::rgb8(80, 90, 130));
        let label = TextStyle::new(11.0).color(AMBER);
        g.draw_text(&self.mono, title, rect.min() + vec2(8.0, 4.0), &label);
        rect.min() + vec2(10.0, 24.0)
    }

    fn left_column(&self, g: &mut yowie::gfx::Graphics) {
        let at = self.panel(
            g,
            Rect::new(20.0, 14.0, 400.0, 100.0),
            "Kerning and ligatures",
        );
        let big = TextStyle::new(30.0).color(INK);
        g.draw_text(&self.body, "AVATAR Toy Wa Ty", at, &big);
        let small = TextStyle::new(22.0).color(INK);
        g.draw_text(
            &self.body,
            "office affluent fi fl ffi",
            at + vec2(0.0, 38.0),
            &small,
        );

        let at = self.panel(g, Rect::new(20.0, 124.0, 400.0, 150.0), "Rich markup");
        let style = TextStyle::new(17.0).color(INK).width(380.0);
        g.draw_markup(
            &self.body,
            "[b]Bold[/b], [i]italic[/i], [#ff7050]colour[/#], [size=26]big[/size] and \
             [size=12]small[/size], [font=Go Mono]mono[/font], [u]underlined[/u], \
             [s]struck[/s], and a [link=home]link[/link]. [b][#60d0ff]Nested \
             [i]spans[/i][/#][/b] work too.",
            at,
            &style,
        );

        let top = 284.0;
        let at = self.panel(
            g,
            Rect::new(20.0, top, 400.0, 288.0),
            "Hyphenation by language",
        );
        for (i, (language, tag, text)) in HYPHEN.iter().enumerate() {
            let (col, row) = (i % 2, i / 2);
            let pos = at + vec2(col as f32 * 194.0, row as f32 * 38.0);
            let style = TextStyle::new(11.0)
                .color(INK)
                .width(184.0)
                .hyphenate(*language);
            g.draw_text(&self.mono, tag, pos, &TextStyle::new(11.0).color(AMBER));
            g.draw_text(
                &self.body,
                text,
                pos + vec2(18.0, 0.0),
                &TextStyle {
                    width: Some(166.0),
                    ..style
                },
            );
        }

        let top = 582.0;
        let at = self.panel(
            g,
            Rect::new(20.0, top, 400.0, 358.0),
            "Wrapping and alignment",
        );
        let aligns = [
            (TextAlign::Left, "Left"),
            (TextAlign::Center, "Center"),
            (TextAlign::Right, "Right"),
            (TextAlign::Justified, "Justified"),
        ];
        for (i, (align, name)) in aligns.into_iter().enumerate() {
            let pos = at + vec2((i % 2) as f32 * 194.0, (i / 2) as f32 * 160.0);
            let style = TextStyle::new(12.0).color(INK).width(184.0).align(align);
            g.draw_text(&self.mono, name, pos, &TextStyle::new(11.0).color(AMBER));
            g.draw_text(&self.body, STORY, pos + vec2(0.0, 16.0), &style);
        }
    }

    fn middle_column(&self, g: &mut yowie::gfx::Graphics) {
        let ar = TextStyle::new(32.0).color(INK);
        let at = self.panel(g, Rect::new(440.0, 14.0, 400.0, 86.0), "Arabic joining");
        g.draw_text(&self.arabic, "السلام عليكم ورحمة الله", at, &ar);

        let at = self.panel(
            g,
            Rect::new(440.0, 110.0, 400.0, 150.0),
            "Right-to-left and mixed lines",
        );
        let rtl = TextStyle::new(24.0)
            .color(INK)
            .width(380.0)
            .align(TextAlign::Right);
        g.draw_text(&self.chain, "مرحبا بالعالم", at, &rtl);
        let mixed = TextStyle::new(20.0).color(INK).width(380.0);
        g.draw_text(
            &self.chain,
            "The word كتاب means book.",
            at + vec2(0.0, 40.0),
            &mixed,
        );
        g.draw_text(
            &self.chain,
            "abc بيت def 123 جميل",
            at + vec2(0.0, 72.0),
            &mixed,
        );

        let at = self.panel(
            g,
            Rect::new(440.0, 270.0, 400.0, 90.0),
            "Fallback chain: Go, Noto Arabic, Noto CJK",
        );
        let fb = TextStyle::new(24.0).color(INK);
        g.draw_text(&self.chain, "Go ب 你好 world", at, &fb);
        g.draw_text(
            &self.body,
            "Each glyph uses the first font in the chain that has it.",
            at + vec2(0.0, 36.0),
            &TextStyle::new(13.0).color(INK),
        );

        let at = self.panel(g, Rect::new(440.0, 370.0, 400.0, 170.0), "Unicode wrapping");
        let wrap = TextStyle::new(20.0).color(INK).width(380.0);
        g.draw_text(
            &self.chain,
            "雾中的野人在山脊上行走，留下盘子那么大的脚印。你好世界。",
            at,
            &wrap,
        );
        let url = TextStyle::new(14.0).color(INK).width(380.0);
        g.draw_text(
            &self.body,
            "Self-contained, well-known, and e-mail/to-do items break at hyphens, \
             slashes and spaces; antidisestablishmentarianism_without_any_break_point \
             breaks inside the word.",
            at + vec2(0.0, 66.0),
            &url,
        );

        let at = self.panel(
            g,
            Rect::new(440.0, 550.0, 400.0, 100.0),
            "Colour emoji (bitmap font)",
        );
        let em = TextStyle::new(40.0).color(INK);
        g.draw_text(
            &self.emoji,
            "\u{1F600} \u{1F680} \u{1F431} \u{1F525} \u{1F389}",
            at,
            &em,
        );

        let at = self.panel(
            g,
            Rect::new(440.0, 660.0, 400.0, 280.0),
            "Caret and hit testing",
        );
        let style = TextStyle::new(22.0).color(INK).width(380.0);
        let text = "Click anywhere to put the caret there, even inside wrapped, mixed text.";
        let layout = g.layout_text(&self.body, text, &style);
        let (from, to) = (6, 28);
        let (a, b) = (layout.caret(from), layout.caret(to));
        if (a.position.y - b.position.y).abs() < 1.0 {
            let sel = Rect::new(
                at.x + a.position.x,
                at.y + a.position.y,
                b.position.x - a.position.x,
                a.height,
            );
            g.fill_rect(sel, Color::rgb8(50, 90, 160));
        }
        g.draw_layout(&layout, at);
        let click = vec2(150.0, 28.0);
        let offset = layout.hit_test(click);
        let caret = layout.caret(offset);
        g.fill_rect(
            Rect::new(
                at.x + caret.position.x - 1.0,
                at.y + caret.position.y,
                2.0,
                caret.height,
            ),
            Color::rgb8(255, 120, 80),
        );
        g.fill_circle(at + click, 3.0, Color::rgb8(120, 255, 160));
        let note = format!(
            "point (150, 28) is offset {offset}: before \"{}\"",
            &text[offset..(offset + 5).min(text.len())]
        );
        g.draw_text(
            &self.mono,
            &note,
            at + vec2(0.0, 100.0),
            &TextStyle::new(11.0).color(AMBER),
        );
    }

    fn right_column(&self, g: &mut yowie::gfx::Graphics) {
        let at = self.panel(g, Rect::new(860.0, 14.0, 400.0, 330.0), "Vertical text");
        let up = TextStyle::new(26.0)
            .color(INK)
            .orientation(Orientation::Upright);
        g.draw_text(
            &self.chain,
            "雾中的野人在山脊上行走\n留下盘子那么大的脚印",
            at + vec2(110.0, 0.0),
            &up,
        );
        g.draw_text(&self.chain, "STACKED", at + vec2(60.0, 0.0), &up);
        let side = TextStyle::new(22.0)
            .color(AMBER)
            .orientation(Orientation::Sideways);
        g.draw_text(&self.body, "Sideways text", at, &side);
        g.draw_text(
            &self.body,
            "Upright columns read down,\nnew columns step left.",
            at + vec2(200.0, 250.0),
            &TextStyle::new(12.0).color(INK),
        );

        let at = self.panel(
            g,
            Rect::new(860.0, 354.0, 400.0, 360.0),
            "Distance-field text",
        );
        let mut y = 0.0;
        for size in [12.0, 24.0, 48.0, 96.0] {
            let style = TextStyle::new(size).color(INK).sdf(true);
            g.draw_text(&self.body, "Scalable", at + vec2(0.0, y), &style);
            y += size * 1.25;
        }
        g.push_transform();
        g.translate(at + vec2(30.0, 310.0));
        g.rotate(-0.2);
        g.scale(vec2(1.3, 1.3));
        g.draw_text(
            &self.body,
            "Rotated",
            Vec2::ZERO,
            &TextStyle::new(40.0)
                .color(Color::rgb8(120, 220, 255))
                .sdf(true),
        );
        g.pop_transform();

        let at = self.panel(g, Rect::new(860.0, 724.0, 400.0, 216.0), "Styles");
        let both = TextStyle::new(24.0).color(INK).underline();
        g.draw_text(&self.body, "Underlined", at, &both);
        g.draw_text(
            &self.body,
            "Struck through",
            at + vec2(0.0, 34.0),
            &TextStyle::new(24.0).color(INK).strikethrough(),
        );
        g.draw_text(
            &self.mono,
            &format!("body font: {}", self.body_name),
            at + vec2(0.0, 80.0),
            &TextStyle::new(12.0).color(AMBER),
        );
        let line = TextStyle::new(16.0)
            .color(INK)
            .line_height(1.6)
            .width(370.0);
        g.draw_text(
            &self.arabic,
            "Latin and عربي share one baseline",
            at + vec2(0.0, 104.0),
            &line,
        );
    }
}

fn main() -> Result<(), yowie::Error> {
    let config = Config {
        clear_color: Color::rgb8(20, 22, 30),
        ..Config::new("Yowie text").with_size(1280, 960)
    };
    yowie::run::<Text>(config)
}
