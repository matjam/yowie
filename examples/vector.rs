//! Vector paths: shapes filled under both rules, curves and arcs, every cap
//! and join, dashes, gradient and textured fills, all seven blend modes,
//! the transform stack, and anti-aliasing on and off. Escape quits.

use std::f32::consts::{PI, TAU};

use yowie::gfx::{
    BlendMode, Cap, Dash, Fill, FillRule, Filter, Font, Gradient, GradientStop, Graphics, Join,
    Paint, Path, Stroke, TextStyle, Texture, TextureOptions, TexturePaint, Wrap,
};
use yowie::input::Key;
use yowie::{Color, Config, Context, Game, GameResult, Rect, Vec2, vec2};

const INK: Color = Color::rgb8(235, 232, 220);
const AMBER: Color = Color::rgb8(250, 200, 90);
const TEAL: Color = Color::rgb8(70, 200, 190);
const CORAL: Color = Color::rgb8(255, 120, 90);

struct Vector {
    font: Font,
    checker: Texture,
}

/// A 16 by 16 checkerboard in two greens.
fn checker_pixels() -> Vec<u8> {
    let mut px = Vec::new();
    for y in 0..16u32 {
        for x in 0..16u32 {
            let on = (x / 4 + y / 4) % 2 == 0;
            let c = if on {
                [60, 170, 90, 255]
            } else {
                [20, 70, 50, 255]
            };
            px.extend_from_slice(&c);
        }
    }
    px
}

impl Game for Vector {
    fn init(ctx: &mut Context) -> GameResult<Self> {
        let options = TextureOptions {
            filter: Filter::Nearest,
            wrap: Wrap::Repeat,
            ..TextureOptions::default()
        };
        Ok(Vector {
            font: ctx.gfx.default_mono_font(),
            checker: Texture::from_pixels(&ctx.gfx, 16, 16, &checker_pixels(), options)?,
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
        let at = |g: &mut Graphics, font: &Font, col: f32, row: f32, title: &str| -> Vec2 {
            let rect = Rect::new(14.0 + col * 396.0, 14.0 + row * 268.0, 384.0, 256.0);
            g.fill_rect(rect, Color::rgb8(30, 34, 48));
            g.stroke_rect(rect, 1.0, Color::rgb8(80, 90, 130));
            g.draw_text(
                font,
                title,
                rect.min() + vec2(8.0, 4.0),
                &TextStyle::new(11.0).color(AMBER),
            );
            rect.min() + vec2(0.0, 22.0)
        };
        let font = self.font.clone();
        let o = at(
            g,
            &font,
            0.0,
            0.0,
            "Fill rules: non-zero (top), even-odd (bottom)",
        );
        fill_rules(g, o);
        let o = at(g, &font, 1.0, 0.0, "Curves and arcs");
        curves(g, o);
        let o = at(g, &font, 2.0, 0.0, "Caps (butt, square, round) and joins");
        caps_and_joins(g, o);
        let o = at(g, &font, 0.0, 1.0, "Dashes");
        dashes(g, o);
        let o = at(g, &font, 1.0, 1.0, "Gradient and textured fills");
        paints(g, o, &self.checker);
        let o = at(g, &font, 2.0, 1.0, "Blend modes");
        blends(g, o, &font);
        let o = at(g, &font, 0.0, 2.0, "Transform stack");
        transforms(g, o);
        let o = at(g, &font, 1.0, 2.0, "Anti-aliasing: on (left), off (right)");
        antialiasing(g, o);
        let o = at(g, &font, 2.0, 2.0, "Strokes of every width");
        widths(g, o);
        Ok(())
    }
}

fn star(center: Vec2, radius: f32) -> Path {
    let mut b = Path::builder();
    for k in 0..5 {
        let a = -PI / 2.0 + (k * 2) as f32 * TAU / 5.0;
        let p = center + Vec2::from_angle(a) * radius;
        if k == 0 {
            b.move_to(p);
        } else {
            b.line_to(p);
        }
    }
    b.close();
    b.build()
}

fn nested(origin: Vec2, size: f32) -> Path {
    let mut b = Path::builder();
    b.add_rect(Rect::new(origin.x, origin.y, size, size));
    b.add_rect(Rect::new(
        origin.x + size * 0.25,
        origin.y + size * 0.25,
        size * 0.5,
        size * 0.5,
    ));
    b.build()
}

fn fill_rules(g: &mut Graphics, o: Vec2) {
    for (row, rule) in [FillRule::NonZero, FillRule::EvenOdd]
        .into_iter()
        .enumerate()
    {
        let y = o.y + row as f32 * 114.0;
        let fill = Fill::new(TEAL).rule(rule);
        g.fill_path(&star(vec2(o.x + 70.0, y + 54.0), 52.0), &fill);
        g.fill_path(
            &nested(vec2(o.x + 150.0, y + 4.0), 100.0),
            &Fill::new(CORAL).rule(rule),
        );
        let mut b = Path::builder();
        b.add_circle(vec2(o.x + 316.0, y + 54.0), 46.0);
        b.add_circle(vec2(o.x + 316.0, y + 54.0), 22.0);
        g.fill_path(&b.build(), &Fill::new(AMBER).rule(rule));
    }
}

fn curves(g: &mut Graphics, o: Vec2) {
    let ink = Stroke::new(INK, 3.0).cap(Cap::Round);
    let mut b = Path::builder();
    b.move_to(o + vec2(14.0, 70.0));
    b.quad_to(o + vec2(60.0, 0.0), o + vec2(110.0, 70.0));
    b.cubic_to(
        o + vec2(140.0, 130.0),
        o + vec2(190.0, 10.0),
        o + vec2(230.0, 70.0),
    );
    g.stroke_path(&b.build(), &ink);

    let mut b = Path::builder();
    b.move_to(o + vec2(260.0, 70.0));
    b.arc(o + vec2(300.0, 70.0), vec2(40.0, 40.0), PI, 0.0);
    g.stroke_path(&b.build(), &Stroke::new(TEAL, 4.0).cap(Cap::Round));

    let mut b = Path::builder();
    b.move_to(o + vec2(20.0, 130.0));
    b.arc_to(vec2(40.0, 25.0), 0.3, false, true, o + vec2(100.0, 150.0));
    b.arc_to(vec2(30.0, 30.0), 0.0, true, false, o + vec2(140.0, 190.0));
    g.stroke_path(&b.build(), &Stroke::new(CORAL, 4.0).cap(Cap::Round));

    g.fill_path(
        &Path::rounded_rect(Rect::new(170.0, 120.0, 90.0, 60.0).translate(o), 16.0),
        &Fill::new(AMBER),
    );
    g.fill_path(
        &Path::ellipse(o + vec2(320.0, 150.0), vec2(48.0, 26.0)),
        &Fill::new(TEAL),
    );
    g.stroke_path(
        &Path::circle(o + vec2(70.0, 200.0), 24.0),
        &Stroke::new(INK, 2.0),
    );
    let mut b = Path::builder();
    b.move_to(o + vec2(130.0, 215.0));
    b.cubic_to(
        o + vec2(160.0, 160.0),
        o + vec2(220.0, 250.0),
        o + vec2(260.0, 200.0),
    );
    b.line_to(o + vec2(330.0, 215.0));
    g.stroke_path(&b.build(), &Stroke::new(Color::rgb8(190, 150, 255), 3.0));
}

fn caps_and_joins(g: &mut Graphics, o: Vec2) {
    for (i, cap) in [Cap::Butt, Cap::Square, Cap::Round].into_iter().enumerate() {
        let y = o.y + 20.0 + i as f32 * 34.0;
        let (a, b) = (o + vec2(24.0, y - o.y), o + vec2(150.0, y - o.y));
        let mut p = Path::builder();
        p.move_to(a).line_to(b);
        let path = p.build();
        g.stroke_path(&path, &Stroke::new(TEAL, 18.0).cap(cap));
        g.stroke_path(
            &path,
            &Stroke::new(Color::rgb8(20, 22, 30), 1.0).antialias(false),
        );
    }
    for (i, join) in [Join::Miter, Join::Round, Join::Bevel]
        .into_iter()
        .enumerate()
    {
        let x = o.x + 180.0 + i as f32 * 66.0;
        let mut p = Path::builder();
        p.move_to(vec2(x, o.y + 100.0))
            .line_to(vec2(x + 24.0, o.y + 30.0))
            .line_to(vec2(x + 48.0, o.y + 100.0));
        g.stroke_path(
            &p.build(),
            &Stroke::new(CORAL, 14.0).join(join).miter_limit(6.0),
        );
    }
    let mut p = Path::builder();
    p.move_to(o + vec2(180.0, 190.0))
        .line_to(o + vec2(230.0, 130.0))
        .line_to(o + vec2(250.0, 190.0))
        .line_to(o + vec2(300.0, 140.0))
        .line_to(o + vec2(350.0, 190.0));
    g.stroke_path(
        &p.build(),
        &Stroke::new(AMBER, 10.0).join(Join::Miter).miter_limit(1.2),
    );
    let mut p = Path::builder();
    p.add_rect(Rect::new(30.0, 130.0, 100.0, 70.0).translate(o));
    g.stroke_path(&p.build(), &Stroke::new(INK, 12.0).join(Join::Round));
}

fn dashes(g: &mut Graphics, o: Vec2) {
    let rows = [
        (Dash::new(18.0, 10.0), Cap::Butt, 8.0),
        (Dash::new(0.0, 14.0), Cap::Round, 8.0),
        (
            Dash {
                pattern: vec![24.0, 8.0, 4.0, 8.0],
                offset: 6.0,
            },
            Cap::Butt,
            6.0,
        ),
    ];
    for (i, (dash, cap, width)) in rows.into_iter().enumerate() {
        let y = o.y + 18.0 + i as f32 * 28.0;
        let mut p = Path::builder();
        p.move_to(vec2(o.x + 20.0, y)).line_to(vec2(o.x + 360.0, y));
        g.stroke_path(&p.build(), &Stroke::new(TEAL, width).cap(cap).dash(dash));
    }
    let circle = Path::circle(o + vec2(110.0, 160.0), 56.0);
    g.stroke_path(
        &circle,
        &Stroke::new(CORAL, 6.0)
            .dash(Dash::new(20.0, 12.0))
            .cap(Cap::Round),
    );
    let rect = Path::rounded_rect(Rect::new(210.0, 110.0, 150.0, 100.0).translate(o), 20.0);
    g.stroke_path(&rect, &Stroke::new(AMBER, 5.0).dash(Dash::new(12.0, 6.0)));
}

fn paints(g: &mut Graphics, o: Vec2, checker: &Texture) {
    let sky = Gradient::new(&[
        GradientStop::new(0.0, Color::rgb8(255, 210, 90)),
        GradientStop::new(0.5, Color::rgb8(255, 90, 120)),
        GradientStop::new(1.0, Color::rgb8(60, 70, 200)),
    ]);
    let r = Rect::new(14.0, 10.0, 170.0, 100.0).translate(o);
    g.fill_path(
        &Path::rounded_rect(r, 12.0),
        &Fill::new(Paint::linear(r.min(), r.min() + vec2(170.0, 100.0), sky)),
    );
    let glow = Gradient::new(&[
        GradientStop::new(0.0, Color::WHITE),
        GradientStop::new(0.4, Color::rgb8(90, 220, 255)),
        GradientStop::new(1.0, Color::rgba(0.0, 0.2, 0.6, 0.0)),
    ]);
    let c = o + vec2(280.0, 60.0);
    g.fill_path(
        &Path::circle(c, 56.0),
        &Fill::new(Paint::radial(c, 56.0, glow)),
    );
    let bands =
        Gradient::two(Color::rgb8(255, 255, 255), Color::rgb8(40, 40, 120)).spread(Wrap::Mirror);
    let r2 = Rect::new(14.0, 124.0, 170.0, 100.0).translate(o);
    g.fill_path(
        &Path::rect(r2),
        &Fill::new(Paint::linear(r2.min(), r2.min() + vec2(60.0, 0.0), bands)),
    );
    let mut tile = TexturePaint::new(checker, o + vec2(200.0, 124.0));
    tile.scale = vec2(3.0, 3.0);
    let mut b = Path::builder();
    b.move_to(o + vec2(200.0, 224.0))
        .line_to(o + vec2(290.0, 124.0))
        .line_to(o + vec2(376.0, 224.0))
        .close();
    g.fill_path(&b.build(), &Fill::new(Paint::Texture(tile)));
}

fn blends(g: &mut Graphics, o: Vec2, font: &Font) {
    for i in 0..16 {
        let stripe = if i % 2 == 0 {
            Color::rgb8(70, 80, 110)
        } else {
            Color::rgb8(110, 100, 70)
        };
        g.fill_rect(
            Rect::new(10.0 + i as f32 * 22.0, 8.0, 22.0, 214.0).translate(o),
            stripe,
        );
    }
    for (i, mode) in BlendMode::ALL.into_iter().enumerate() {
        let (col, row) = (i % 4, i / 4);
        let c = o + vec2(46.0 + col as f32 * 88.0, 50.0 + row as f32 * 100.0);
        g.with_blend(mode, |g| {
            g.fill_path(
                &Path::circle(c, 36.0),
                &Fill::new(Color::rgb8(240, 120, 60)),
            );
            g.fill_path(
                &Path::circle(c + vec2(20.0, 8.0), 24.0),
                &Fill::new(Color::rgba8(70, 150, 255, 200)),
            );
        });
        let name = if mode == BlendMode::PremultipliedAlpha {
            "PreAlpha".to_owned()
        } else {
            format!("{mode:?}")
        };
        g.draw_text(
            font,
            &name[..name.len().min(10)],
            c + vec2(-34.0, 46.0),
            &TextStyle::new(10.0).color(INK),
        );
    }
}

fn transforms(g: &mut Graphics, o: Vec2) {
    g.push_transform();
    g.translate(o + vec2(110.0, 120.0));
    for k in 0..12 {
        g.push_transform();
        g.rotate(k as f32 * TAU / 12.0);
        g.translate(vec2(52.0, 0.0));
        g.scale(vec2(1.0 + k as f32 * 0.08, 1.0));
        let hue = k as f32 * 30.0;
        g.fill_path(
            &Path::rounded_rect(Rect::new(-14.0, -8.0, 28.0, 16.0), 4.0),
            &Fill::new(Color::hsv(hue, 0.6, 0.95)),
        );
        g.pop_transform();
    }
    g.pop_transform();
    g.push_transform();
    g.translate(o + vec2(215.0, 50.0));
    for k in 0..3 {
        g.push_transform();
        g.shear(vec2(0.15 * k as f32, 0.0));
        g.translate(vec2(0.0, k as f32 * 52.0));
        g.fill_rect(Rect::new(0.0, 0.0, 80.0, 40.0), Color::rgb8(90, 200, 150));
        g.stroke_rect(Rect::new(0.0, 0.0, 80.0, 40.0), 2.0, INK);
        g.pop_transform();
    }
    g.pop_transform();
}

fn antialiasing(g: &mut Graphics, o: Vec2) {
    for (i, aa) in [true, false].into_iter().enumerate() {
        let x = o.x + 100.0 + i as f32 * 190.0;
        let c = vec2(x, o.y + 80.0);
        g.fill_path(&Path::circle(c, 60.0), &Fill::new(TEAL).antialias(aa));
        for k in 0..8 {
            let a = k as f32 * TAU / 16.0 + 0.1;
            let d = Vec2::from_angle(a) * 52.0;
            let mut p = Path::builder();
            p.move_to(c - d).line_to(c + d);
            g.stroke_path(
                &p.build(),
                &Stroke::new(Color::rgb8(20, 22, 30), 1.0).antialias(aa),
            );
        }
        let mut p = Path::builder();
        p.move_to(vec2(x - 70.0, o.y + 170.0))
            .quad_to(vec2(x, o.y + 120.0), vec2(x + 70.0, o.y + 200.0));
        g.stroke_path(&p.build(), &Stroke::new(CORAL, 2.0).antialias(aa));
    }
}

fn widths(g: &mut Graphics, o: Vec2) {
    for (i, w) in [0.5, 1.0, 1.5, 2.0, 3.0, 5.0, 8.0].into_iter().enumerate() {
        let y = o.y + 16.0 + i as f32 * 30.0;
        let mut p = Path::builder();
        p.move_to(vec2(o.x + 20.0, y)).cubic_to(
            vec2(o.x + 120.0, y - 18.0),
            vec2(o.x + 220.0, y + 18.0),
            vec2(o.x + 360.0, y),
        );
        g.stroke_path(&p.build(), &Stroke::new(INK, w));
    }
}

fn main() -> Result<(), yowie::Error> {
    let config = Config {
        clear_color: Color::rgb8(20, 22, 30),
        ..Config::new("Yowie vector").with_size(1204, 820)
    };
    yowie::run::<Vector>(config)
}
