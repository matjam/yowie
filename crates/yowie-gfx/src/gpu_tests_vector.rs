//! GPU tests for vector paths and the transform stack.

use image::RgbaImage;
use yowie_core::{Color, Rect, Vec2, uvec2, vec2};

use crate::gpu_tests::{assert_px, headless, render};
use crate::{
    BlendMode, Cap, Dash, Fill, FillRule, Gradient, GradientStop, Join, Paint, Path, Stroke, Wrap,
};

fn lit(img: &RgbaImage, x: u32, y: u32) -> bool {
    let p = img.get_pixel(x, y).0;
    p[0].max(p[1]).max(p[2]) > 128
}

fn nested_squares() -> Path {
    let mut b = Path::builder();
    b.add_rect(Rect::new(4.0, 4.0, 40.0, 40.0));
    b.add_rect(Rect::new(14.0, 14.0, 20.0, 20.0));
    b.build()
}

#[test]
fn fill_rules_differ_inside_nested_contours() {
    let Some(mut gfx) = headless(uvec2(48, 48)) else {
        return;
    };
    let path = nested_squares();
    gfx.fill_path(&path, &Fill::new(Color::WHITE).rule(FillRule::NonZero));
    let nz = render(&mut gfx);
    gfx.begin_frame();
    gfx.fill_path(&path, &Fill::new(Color::WHITE).rule(FillRule::EvenOdd));
    let eo = render(&mut gfx);
    assert!(lit(&nz, 24, 24), "non-zero keeps the middle filled");
    assert!(!lit(&eo, 24, 24), "even-odd cuts the middle out");
    assert!(lit(&nz, 8, 24) && lit(&eo, 8, 24), "the ring is filled");
    assert!(!lit(&nz, 1, 1) && !lit(&eo, 1, 1));
}

#[test]
fn caps_extend_the_stroke_by_their_shape() {
    let Some(mut gfx) = headless(uvec2(48, 24)) else {
        return;
    };
    let mut b = Path::builder();
    b.move_to(vec2(14.0, 12.0)).line_to(vec2(34.0, 12.0));
    let line = b.build();
    let mut frames = Vec::new();
    for cap in [Cap::Butt, Cap::Square, Cap::Round] {
        gfx.begin_frame();
        gfx.stroke_path(
            &line,
            &Stroke::new(Color::WHITE, 8.0).cap(cap).antialias(false),
        );
        frames.push(render(&mut gfx));
    }
    let [butt, square, round] = [&frames[0], &frames[1], &frames[2]];
    // Pixel 10 is 3.5 units past the endpoint on its centre; row 9 is 2.5
    // above the line, outside a radius-4 circle but inside a square.
    assert!(!lit(butt, 10, 12), "a butt cap stops at the endpoint");
    assert!(
        lit(square, 10, 12) && lit(square, 10, 9),
        "square reaches the corner"
    );
    assert!(lit(round, 10, 12), "round reaches past the endpoint");
    assert!(!lit(round, 10, 9), "round does not fill the corner");
}

#[test]
fn joins_shape_the_corner() {
    let Some(mut gfx) = headless(uvec2(64, 64)) else {
        return;
    };
    let mut b = Path::builder();
    b.move_to(vec2(10.0, 50.0))
        .line_to(vec2(32.0, 30.0))
        .line_to(vec2(54.0, 50.0));
    let path = b.build();
    let mut count = |join| {
        gfx.begin_frame();
        gfx.stroke_path(
            &path,
            &Stroke::new(Color::WHITE, 8.0)
                .join(join)
                .miter_limit(10.0)
                .antialias(false),
        );
        let img = render(&mut gfx);
        (0..64u32)
            .flat_map(|y| (0..64u32).map(move |x| (x, y)))
            .filter(|&(x, y)| lit(&img, x, y))
            .count()
    };
    let (miter, round, bevel) = (count(Join::Miter), count(Join::Round), count(Join::Bevel));
    assert!(miter > round && round > bevel, "{miter} {round} {bevel}");
}

#[test]
fn dashes_leave_gaps() {
    let Some(mut gfx) = headless(uvec2(64, 16)) else {
        return;
    };
    let mut b = Path::builder();
    b.move_to(vec2(0.0, 8.0)).line_to(vec2(64.0, 8.0));
    let stroke = Stroke::new(Color::WHITE, 4.0).dash(Dash::new(8.0, 8.0));
    gfx.stroke_path(&b.build(), &stroke);
    let img = render(&mut gfx);
    assert!(lit(&img, 4, 8) && !lit(&img, 12, 8) && lit(&img, 20, 8));
}

#[test]
fn gradients_reach_their_end_colours() {
    let Some(mut gfx) = headless(uvec2(64, 32)) else {
        return;
    };
    let red = Color::rgb8(255, 0, 0);
    let blue = Color::rgb8(0, 0, 255);
    let rect = Path::rect(Rect::new(0.0, 0.0, 64.0, 16.0));
    // Stops inset from the ends hold each end colour flat, so the end
    // pixels show the stop colours exactly.
    let gradient = Gradient::new(&[GradientStop::new(0.1, red), GradientStop::new(0.9, blue)]);
    let paint = Paint::linear(vec2(0.0, 0.0), vec2(64.0, 0.0), gradient);
    gfx.fill_path(&rect, &Fill::new(paint).antialias(false));
    let disc = Path::circle(vec2(32.0, 24.0), 7.0);
    let radial = Paint::radial(
        vec2(32.0, 24.0),
        7.0,
        Gradient::new(&[
            GradientStop::new(0.3, Color::rgb8(0, 255, 0)),
            GradientStop::new(0.8, Color::rgb8(255, 255, 0)),
        ])
        .spread(Wrap::Clamp),
    );
    gfx.fill_path(&disc, &Fill::new(radial).antialias(false));
    let img = render(&mut gfx);
    assert_px(&img, 0, 4, [255, 0, 0, 255], 6);
    assert_px(&img, 63, 4, [0, 0, 255, 255], 6);
    let mid = img.get_pixel(32, 4).0;
    assert!(
        mid[0] > 60 && mid[2] > 60 && mid[1] < 20,
        "mixed middle {mid:?}"
    );
    assert_px(&img, 32, 24, [0, 255, 0, 255], 12);
    let rim = img.get_pixel(32, 30).0;
    assert!(rim[0] > 180 && rim[1] > 240, "yellow at the rim {rim:?}");
}

#[test]
fn anti_aliasing_feathers_the_edge() {
    let Some(mut gfx) = headless(uvec2(32, 32)) else {
        return;
    };
    let tri = {
        let mut b = Path::builder();
        b.move_to(vec2(2.0, 28.0))
            .line_to(vec2(28.0, 28.0))
            .line_to(vec2(28.0, 2.0))
            .close();
        b.build()
    };
    let partial = |img: &RgbaImage| img.pixels().filter(|p| p.0[0] > 20 && p.0[0] < 235).count();
    gfx.fill_path(&tri, &Fill::new(Color::WHITE).antialias(true));
    let soft = partial(&render(&mut gfx));
    gfx.begin_frame();
    gfx.fill_path(&tri, &Fill::new(Color::WHITE).antialias(false));
    let hard = partial(&render(&mut gfx));
    assert!(soft > 20, "feathered edge has {soft} partial pixels");
    assert_eq!(hard, 0, "an aliased edge has none");
}

#[test]
fn blend_modes_apply_to_paths() {
    let Some(mut gfx) = headless(uvec2(16, 16)) else {
        return;
    };
    gfx.set_clear_color(Color::rgb8(100, 0, 0));
    gfx.begin_frame();
    let rect = Path::rect(Rect::new(0.0, 0.0, 16.0, 16.0));
    gfx.with_blend(BlendMode::Additive, |g| {
        g.fill_path(&rect, &Fill::new(Color::rgb8(100, 0, 0)).antialias(false));
    });
    let img = render(&mut gfx);
    let p = img.get_pixel(8, 8).0;
    // Linear light: 100 sRGB twice over is about 148 in sRGB.
    assert!(p[0] > 130 && p[0] < 175 && p[1] == 0, "additive {p:?}");
}

#[test]
fn transform_stack_nests() {
    let Some(mut gfx) = headless(uvec2(64, 32)) else {
        return;
    };
    let draw = |g: &mut crate::Graphics| g.fill_rect(Rect::new(0.0, 0.0, 4.0, 4.0), Color::WHITE);
    draw(&mut gfx);
    gfx.push_transform();
    gfx.translate(vec2(20.0, 0.0));
    draw(&mut gfx);
    gfx.push_transform();
    gfx.translate(vec2(20.0, 0.0));
    gfx.scale(Vec2::splat(2.0));
    draw(&mut gfx);
    gfx.pop_transform();
    gfx.translate(vec2(0.0, 20.0));
    draw(&mut gfx);
    gfx.pop_transform();
    draw(&mut gfx);
    let img = render(&mut gfx);
    assert!(lit(&img, 2, 2), "identity");
    assert!(lit(&img, 22, 2), "first translate");
    assert!(
        lit(&img, 43, 3) && lit(&img, 47, 7),
        "translate then scale 2"
    );
    assert!(!lit(&img, 49, 9), "the scaled square ends at 48");
    assert!(lit(&img, 22, 22), "pop restored the first translate");
    assert!(!lit(&img, 42, 22), "the popped scale is gone");
}

#[test]
fn rotation_and_shear_move_corners() {
    let Some(mut gfx) = headless(uvec2(32, 32)) else {
        return;
    };
    gfx.translate(vec2(16.0, 16.0));
    gfx.rotate(std::f32::consts::FRAC_PI_2);
    // Clockwise on screen: +X turns to +Y.
    gfx.fill_rect(Rect::new(2.0, -2.0, 10.0, 4.0), Color::WHITE);
    let img = render(&mut gfx);
    assert!(lit(&img, 16, 22) && !lit(&img, 22, 16));
    gfx.begin_frame();
    gfx.shear(vec2(1.0, 0.0));
    gfx.fill_rect(Rect::new(0.0, 8.0, 4.0, 4.0), Color::WHITE);
    let img = render(&mut gfx);
    assert!(lit(&img, 11, 9), "y = 9 shifts x by 9");
    assert!(!lit(&img, 2, 9));
}
