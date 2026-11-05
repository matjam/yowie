//! Two interface panels that edit every field of `PostSettings`: the
//! picture (exposure, tone curve, grade, bloom, occlusion, reflections)
//! on the left and the camera and lens (anti-aliasing, depth of field,
//! motion blur, light shafts, lens effects) on the right.

use yowie::Rect;
use yowie::gfx::scene::{Lut, PostSettings, Tonemap};
use yowie::ui::Frame;

const TONEMAPS: [(&str, Tonemap); 5] = [
    ("None", Tonemap::None),
    ("ACES", Tonemap::Aces),
    ("AgX", Tonemap::Agx),
    ("Reinhard", Tonemap::Reinhard),
    ("Khronos PBR Neutral", Tonemap::Neutral),
];

const SAMPLES: [u32; 4] = [1, 2, 4, 8];

fn slider_u32(f: &mut Frame<'_>, label: &str, value: &mut u32, lo: i32, hi: i32) {
    let mut v = *value as i32;
    if f.slider_int(label, &mut v, lo..=hi).changed {
        *value = v.max(0) as u32;
    }
}

/// The picture panel at `rect`. `top` draws the caller's own rows first;
/// `lut` is the lookup table the checkbox turns on.
pub fn picture_panel(
    f: &mut Frame<'_>,
    rect: Rect,
    post: &mut PostSettings,
    lut: &Lut,
    top: impl FnOnce(&mut Frame<'_>),
) {
    f.panel(Some("Post-processing"), rect, |f| {
        top(f);
        f.separator();
        f.slider("Exposure", &mut post.exposure, 0.1..=4.0);
        f.checkbox("Auto exposure", &mut post.auto_exposure);
        f.slider(
            "Exposure compensation",
            &mut post.exposure_compensation,
            -3.0..=3.0,
        );
        f.slider("Adaptation speed", &mut post.adaptation_speed, 0.1..=10.0);
        let mut tonemap = TONEMAPS
            .iter()
            .position(|(_, t)| *t == post.tonemap)
            .unwrap_or(1);
        let names: Vec<&str> = TONEMAPS.iter().map(|(n, _)| *n).collect();
        if f.combo_box("Tone curve", &mut tonemap, &names).changed {
            post.tonemap = TONEMAPS[tonemap].1;
        }
        f.collapsing_header("Colour grade", false, |f| {
            f.slider("Temperature", &mut post.temperature, -1.0..=1.0);
            f.slider("Tint", &mut post.tint, -1.0..=1.0);
            f.slider("Saturation", &mut post.saturation, 0.0..=2.0);
            f.slider("Contrast", &mut post.contrast, 0.5..=1.5);
            for (name, v, lo, hi) in [
                ("Lift", &mut post.lift, -0.2, 0.2),
                ("Gamma", &mut post.gamma, 0.5, 2.0),
                ("Gain", &mut post.gain, 0.5, 2.0),
            ] {
                f.slider(&format!("{name} red"), &mut v.x, lo..=hi);
                f.slider(&format!("{name} green"), &mut v.y, lo..=hi);
                f.slider(&format!("{name} blue"), &mut v.z, lo..=hi);
            }
            let mut on = post.lut.is_some();
            if f.checkbox("Lookup table (warm)", &mut on).changed {
                post.lut = on.then(|| lut.clone());
            }
            f.slider("Lookup table strength", &mut post.lut_strength, 0.0..=1.0);
        });
        f.collapsing_header("Bloom", true, |f| {
            f.slider("Bloom", &mut post.bloom, 0.0..=1.0);
            f.slider("Bloom threshold", &mut post.bloom_threshold, 0.2..=3.0);
        });
        f.collapsing_header("Ambient occlusion", true, |f| {
            f.slider("Ambient occlusion", &mut post.ambient_occlusion, 0.0..=1.0);
            f.slider("Occlusion radius", &mut post.occlusion_radius, 0.2..=3.0);
            f.checkbox("Show occlusion buffer", &mut post.show_occlusion);
        });
        f.collapsing_header("Reflections", false, |f| {
            f.slider("Reflections", &mut post.reflections, 0.0..=1.0);
            f.slider(
                "Reflection roughness",
                &mut post.reflection_roughness,
                0.05..=1.0,
            );
            f.slider(
                "Reflection distance",
                &mut post.reflection_distance,
                1.0..=100.0,
            );
            slider_u32(f, "Reflection steps", &mut post.reflection_steps, 16, 256);
        });
    });
}

/// The camera and lens panel at `rect`.
pub fn lens_panel(f: &mut Frame<'_>, rect: Rect, post: &mut PostSettings) {
    f.panel(Some("Camera and lens"), rect, |f| {
        let mut samples = SAMPLES.iter().position(|s| *s == post.samples).unwrap_or(0);
        if f.combo_box("Multisampling", &mut samples, &["1x", "2x", "4x", "8x"])
            .changed
        {
            post.samples = SAMPLES[samples];
        }
        f.checkbox("Anti-alias (FXAA)", &mut post.fxaa);
        f.checkbox("Temporal anti-alias", &mut post.temporal_aa);
        f.slider("Temporal blend", &mut post.temporal_blend, 0.02..=0.5);
        f.collapsing_header("Depth of field", false, |f| {
            f.slider(
                "Focus distance (0 off)",
                &mut post.focus_distance,
                0.0..=25.0,
            );
            f.slider("Focus range (0 auto)", &mut post.focus_range, 0.0..=10.0);
            f.slider("Bokeh radius", &mut post.bokeh_radius, 1.0..=40.0);
            slider_u32(f, "Bokeh samples", &mut post.bokeh_samples, 4, 64);
        });
        f.collapsing_header("Motion blur", false, |f| {
            f.slider("Motion blur", &mut post.motion_blur, 0.0..=1.0);
            slider_u32(f, "Motion samples", &mut post.motion_samples, 2, 32);
        });
        f.collapsing_header("God rays", false, |f| {
            f.slider("God rays", &mut post.god_rays, 0.0..=2.0);
            f.slider("God ray decay", &mut post.god_ray_decay, 0.8..=1.0);
            f.slider("God ray density", &mut post.god_ray_density, 0.1..=1.0);
            slider_u32(f, "God ray samples", &mut post.god_ray_samples, 8, 128);
        });
        f.collapsing_header("Lens", false, |f| {
            f.slider("Vignette", &mut post.vignette, 0.0..=1.0);
            f.slider("Chromatic aberration", &mut post.aberration, 0.0..=6.0);
            f.slider("Lens distortion", &mut post.distortion, -1.0..=1.0);
            f.slider("Lens ghosts", &mut post.ghosts, 0.0..=1.0);
            f.slider("Film grain", &mut post.grain, 0.0..=0.2);
        });
    });
}
