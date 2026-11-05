//! GPU tests for rich text, bidirectional lines, hyphenation, fallback,
//! colour glyphs, distance-field text, vertical text and hit testing.

use image::RgbaImage;
use yowie_core::{Color, Rect, Vec2, uvec2, vec2};

use crate::gpu_tests::{headless, render};
use crate::{Font, Graphics, Language, Orientation, TextStyle};

const ARABIC: &[u8] = include_bytes!("../../../examples/assets/fonts/NotoSansArabic-Regular.ttf");
const EMOJI: &[u8] = include_bytes!("../../../examples/assets/fonts/NotoColorEmoji-subset.ttf");

fn load(gfx: &mut Graphics, bytes: &[u8]) -> Font {
    match Font::from_bytes(gfx, bytes.to_vec()) {
        Ok(f) => f,
        Err(e) => panic!("font: {e}"),
    }
}

/// Pixels in `area` brighter than `min` in any channel.
fn ink(img: &RgbaImage, area: Rect, min: u8) -> Vec<(u32, u32, [u8; 4])> {
    let mut out = Vec::new();
    for y in area.y as u32..((area.y + area.h) as u32).min(img.height()) {
        for x in area.x as u32..((area.x + area.w) as u32).min(img.width()) {
            let p = img.get_pixel(x, y).0;
            if p[0].max(p[1]).max(p[2]) > min {
                out.push((x, y, p));
            }
        }
    }
    out
}

#[test]
fn markup_colours_the_right_glyphs() {
    let Some(mut gfx) = headless(uvec2(80, 32)) else {
        return;
    };
    let font = gfx.default_mono_font();
    let style = TextStyle::new(24.0);
    let markup = "[#ff0000]MM[/#][#00ff00]MM[/#]";
    let layout = gfx.layout_markup(&font, markup, &style);
    assert_eq!(layout.plain(), "MMMM");
    let mid = 2.0 + layout.caret(2).position.x;
    gfx.draw_layout(&layout, vec2(2.0, 2.0));
    let img = render(&mut gfx);
    let all = ink(&img, Rect::new(0.0, 0.0, 80.0, 32.0), 100);
    assert!(all.len() > 80);
    for (x, _, p) in &all {
        if (*x as f32) < mid - 1.0 {
            assert!(p[0] > p[1] + 40, "left half is red at {x}: {p:?}");
        } else if (*x as f32) > mid + 1.0 {
            assert!(p[1] > p[0] + 40, "right half is green at {x}: {p:?}");
        }
    }
}

#[test]
fn bold_and_underline_markup() {
    let Some(mut gfx) = headless(uvec2(120, 40)) else {
        return;
    };
    let font = gfx.default_font();
    let style = TextStyle::new(20.0);
    let plain = gfx.measure_markup(&font, "Hamburg", &style);
    let bold = gfx.measure_markup(&font, "[b]Hamburg[/b]", &style);
    assert!(bold.x > plain.x + 1.0, "bold is wider: {plain:?} {bold:?}");
    let under = gfx.layout_markup(&font, "[u]Hamburg[/u]", &style);
    assert_eq!(under.data.decos.len(), 1);
    gfx.draw_layout(&under, vec2(2.0, 2.0));
    let img = render(&mut gfx);
    let d = under.data.decos[0].rect;
    let row = (2.0 + d.y + d.h * 0.5) as u32;
    let run = (3..(2.0 + d.w) as u32)
        .filter(|x| img.get_pixel(*x, row).0[0] > 128)
        .count();
    assert!(
        run as f32 > d.w * 0.9,
        "underline spans the text: {run} of {}",
        d.w
    );
}

#[test]
fn right_to_left_lines_run_backwards() {
    let Some(mut gfx) = headless(uvec2(8, 8)) else {
        return;
    };
    let arabic = load(&mut gfx, ARABIC);
    let style = TextStyle::new(24.0);
    let rtl = gfx.layout_text(&arabic, "بيت", &style);
    let len = rtl.plain().len();
    assert!(rtl.data.lines[0].rtl);
    assert!(
        rtl.caret(0).position.x > rtl.caret(len).position.x,
        "start is on the right"
    );
    assert_eq!(rtl.hit_test(vec2(rtl.size().x + 5.0, 5.0)), 0);
    assert_eq!(rtl.hit_test(vec2(-5.0, 5.0)), len);
    // Joined letters: fewer glyphs than the isolated forms would need is
    // not guaranteed, but every glyph comes from the Arabic font.
    assert!(
        rtl.data
            .glyphs
            .iter()
            .all(|g| g.glyph.font_id == arabic.face)
    );

    let font = gfx
        .default_font()
        .with_fallback(std::slice::from_ref(&arabic));
    let mixed = gfx.layout_text(&font, "abc بيت def", &style);
    let text = mixed.plain().to_owned();
    let a = mixed.caret(0).position.x;
    let d = mixed.caret(text.find('d').unwrap_or(0)).position.x;
    assert!(a < d, "Latin runs stay left to right");
    let first_ar = text.find('ب').unwrap_or(0);
    let last_ar = text.find('ت').unwrap_or(0);
    assert!(
        mixed.caret(first_ar).position.x > mixed.caret(last_ar).position.x,
        "the Arabic run reads right to left inside the line"
    );
    assert!(
        mixed
            .data
            .glyphs
            .iter()
            .any(|g| g.glyph.font_id == arabic.face)
    );
}

#[test]
fn hyphenation_inserts_a_hyphen() {
    let Some(mut gfx) = headless(uvec2(8, 8)) else {
        return;
    };
    let font = gfx.default_mono_font();
    let word = "internationalization";
    let plain = TextStyle::new(16.0).width(100.0);
    let hyph = plain.hyphenate(Language::English);
    let without = gfx.layout_text(&font, word, &plain);
    let with = gfx.layout_text(&font, word, &hyph);
    assert!(without.data.glyphs.iter().all(|g| !g.hyphen));
    let hyphens = with.data.glyphs.iter().filter(|g| g.hyphen).count();
    assert!(hyphens >= 1, "a hyphen ends a broken line");
    assert_eq!(with.plain(), word, "plain text keeps no hyphen");
    for line in &with.data.lines {
        assert!(line.width + 16.0 * 0.6 <= 100.5 || line.glyphs.len() <= 1);
    }
    // Every line but the last ends in a hyphen, and each fits the width.
    for g in with.data.glyphs.iter().filter(|g| g.hyphen) {
        assert!(g.glyph.x + g.glyph.w <= 100.5);
    }
}

#[test]
fn fallback_font_covers_missing_glyphs() {
    let Some(mut gfx) = headless(uvec2(8, 8)) else {
        return;
    };
    let arabic = load(&mut gfx, ARABIC);
    let base = gfx.default_font();
    let chain = base.with_fallback(std::slice::from_ref(&arabic));
    let layout = gfx.layout_text(&chain, "a ب", &TextStyle::new(20.0));
    let faces: Vec<_> = layout.data.glyphs.iter().map(|g| g.glyph.font_id).collect();
    assert!(
        faces.contains(&base.face) && faces.contains(&arabic.face),
        "{faces:?}"
    );
}

#[test]
fn colour_glyphs_keep_their_colours() {
    let Some(mut gfx) = headless(uvec2(80, 80)) else {
        return;
    };
    let emoji = load(&mut gfx, EMOJI);
    gfx.draw_text(
        &emoji,
        "\u{1F600}",
        vec2(4.0, 4.0),
        &TextStyle::new(64.0).color(Color::rgb8(0, 0, 255)),
    );
    let img = render(&mut gfx);
    let warm = ink(&img, Rect::new(0.0, 0.0, 80.0, 80.0), 100)
        .into_iter()
        .filter(|(_, _, p)| p[0] > 200 && p[1] > 150 && p[2] < 100)
        .count();
    assert!(
        warm > 300,
        "a yellow face, not tinted blue: {warm} warm pixels"
    );
}

#[test]
fn distance_field_text_stays_sharp_when_scaled() {
    let Some(mut gfx) = headless(uvec2(260, 140)) else {
        return;
    };
    let font = gfx.default_mono_font();
    let style = TextStyle::new(120.0).sdf(true);
    gfx.draw_text(&font, "I", vec2(60.0, 0.0), &style);
    let img = render(&mut gfx);
    // Walk a row through the middle of the stem and count the pixels that
    // are neither ink nor background.
    let row = 70;
    let line: Vec<u8> = (0..260).map(|x| img.get_pixel(x, row).0[0]).collect();
    let solid = line.iter().filter(|v| **v > 240).count();
    let soft = line.iter().filter(|v| **v > 12 && **v < 240).count();
    assert!(solid >= 6, "a stem of {solid} solid pixels");
    assert!(
        soft <= 8,
        "{soft} soft pixels across the stem's edges: {line:?}"
    );
    // A small size draws too.
    gfx.begin_frame();
    gfx.draw_text(
        &font,
        "I",
        vec2(10.0, 10.0),
        &TextStyle::new(12.0).sdf(true),
    );
    let small = render(&mut gfx);
    assert!(!ink(&small, Rect::new(0.0, 0.0, 40.0, 30.0), 60).is_empty());
}

#[test]
fn upright_text_stacks_in_a_column() {
    let Some(mut gfx) = headless(uvec2(8, 8)) else {
        return;
    };
    let font = gfx.default_font();
    let style = TextStyle::new(20.0).orientation(Orientation::Upright);
    let layout = gfx.layout_text(&font, "ABC", &style);
    let ys: Vec<f32> = layout
        .data
        .glyphs
        .iter()
        .map(|g| g.line_y + g.dy + g.glyph.y)
        .collect();
    assert!(ys.windows(2).all(|w| w[1] > w[0] + 10.0), "{ys:?}");
    let s = layout.size();
    assert!(s.x < 30.0 && s.y > 40.0, "a tall narrow box: {s:?}");
    let side = gfx.layout_text(
        &font,
        "ABC",
        &TextStyle::new(20.0).orientation(Orientation::Sideways),
    );
    assert!(side.size().y > side.size().x, "sideways swaps the size");
}

#[test]
fn hit_testing_round_trips_with_the_caret() {
    let Some(mut gfx) = headless(uvec2(8, 8)) else {
        return;
    };
    let font = gfx.default_font();
    let style = TextStyle::new(18.0).width(120.0);
    let text = "the quick brown fox jumps over the lazy dog";
    let layout = gfx.layout_text(&font, text, &style);
    assert!(layout.line_count() >= 3);
    for i in 0..=text.len() {
        let c = layout.caret(i);
        let hit = layout.hit_test(c.position + Vec2::new(0.5, c.height * 0.5));
        assert_eq!(hit, i, "offset {i} at {c:?}");
    }
    let linked = gfx.layout_markup(
        &font,
        "go [link=home]home[/link] now",
        &TextStyle::new(18.0),
    );
    let x = linked.caret(5).position.x + 2.0;
    assert_eq!(linked.link_at(vec2(x, 8.0)), Some("home"));
    assert_eq!(linked.link_at(vec2(1.0, 8.0)), None);
}
