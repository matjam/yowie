//! The tone curves on the processor, matching `shaders/tonemap.wgsl`.

use yowie_core::{Mat3, Vec3};

use crate::scene::Tonemap;

impl Tonemap {
    /// Maps linear HDR light (already exposed) to linear display light in
    /// 0..1 with this curve, as the GPU does.
    pub fn apply(self, c: Vec3) -> Vec3 {
        match self {
            Tonemap::None => c.clamp(Vec3::ZERO, Vec3::ONE),
            Tonemap::Aces => aces(c),
            Tonemap::Agx => agx(c),
            Tonemap::Reinhard => reinhard(c),
            Tonemap::Neutral => neutral(c),
        }
    }

    /// The number the composite shader selects the curve by.
    pub(crate) fn shader_index(self) -> f32 {
        match self {
            Tonemap::None => 0.0,
            Tonemap::Aces => 1.0,
            Tonemap::Agx => 2.0,
            Tonemap::Reinhard => 3.0,
            Tonemap::Neutral => 4.0,
        }
    }
}

fn aces(c: Vec3) -> Vec3 {
    let m_in = Mat3::from_cols_array(&[
        0.59719, 0.07600, 0.02840, 0.35458, 0.90834, 0.13383, 0.04823, 0.01566, 0.83777,
    ]);
    let m_out = Mat3::from_cols_array(&[
        1.60475, -0.10208, -0.00327, -0.53108, 1.10813, -0.07276, -0.07367, -0.00605, 1.07602,
    ]);
    let v = m_in * c;
    let a = v * (v + 0.024_578_6) - 0.000_090_537;
    let b = v * (0.983_729 * v + 0.432_951) + 0.238_081;
    (m_out * (a / b)).clamp(Vec3::ZERO, Vec3::ONE)
}

fn agx_contrast(x: Vec3) -> Vec3 {
    let x2 = x * x;
    let x4 = x2 * x2;
    15.5 * x4 * x2 - 40.14 * x4 * x + 31.96 * x4 - 6.868 * x2 * x + 0.4298 * x2 + 0.1191 * x
        - 0.00232
}

fn agx(c: Vec3) -> Vec3 {
    let inset = Mat3::from_cols_array(&[
        0.842_479_06,
        0.042_328_24,
        0.042_375_655,
        0.078_433_6,
        0.878_468_6,
        0.078_433_6,
        0.079_223_745,
        0.079_166_13,
        0.879_142_97,
    ]);
    let outset = Mat3::from_cols_array(&[
        1.196_879,
        -0.052_896_85,
        -0.052_971_635,
        -0.098_020_88,
        1.151_903_1,
        -0.098_043_45,
        -0.099_029_74,
        -0.098_961_18,
        1.151_073_7,
    ]);
    let (min_ev, max_ev) = (-12.473_93_f32, 4.026_069_f32);
    let v = inset * c.max(Vec3::splat(1e-10));
    let v = Vec3::new(v.x.log2(), v.y.log2(), v.z.log2())
        .clamp(Vec3::splat(min_ev), Vec3::splat(max_ev));
    let v = (v - min_ev) / (max_ev - min_ev);
    let v = (outset * agx_contrast(v)).clamp(Vec3::ZERO, Vec3::ONE);
    Vec3::new(v.x.powf(2.2), v.y.powf(2.2), v.z.powf(2.2))
}

fn reinhard(c: Vec3) -> Vec3 {
    let c = c.max(Vec3::ZERO);
    c / (Vec3::ONE + c)
}

fn neutral(c: Vec3) -> Vec3 {
    const START: f32 = 0.8 - 0.04;
    const DESATURATION: f32 = 0.15;
    let mut c = c.max(Vec3::ZERO);
    let x = c.min_element();
    let offset = if x < 0.08 { x - 6.25 * x * x } else { 0.04 };
    c -= Vec3::splat(offset);
    let peak = c.max_element();
    if peak < START {
        return c;
    }
    let d = 1.0 - START;
    let new_peak = 1.0 - d * d / (peak + d - START);
    c *= new_peak / peak;
    let g = 1.0 - 1.0 / (DESATURATION * (peak - new_peak) + 1.0);
    c.lerp(Vec3::splat(new_peak), g)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CURVES: [Tonemap; 5] = [
        Tonemap::None,
        Tonemap::Aces,
        Tonemap::Agx,
        Tonemap::Reinhard,
        Tonemap::Neutral,
    ];

    #[test]
    fn curves_are_monotone_and_bounded() {
        for curve in CURVES {
            let mut last = -1.0;
            for i in 0..=400 {
                let x = (i as f32 / 400.0).powi(3) * 64.0;
                let y = curve.apply(Vec3::splat(x)).y;
                assert!(y >= last - 1e-6, "{curve:?} falls at {x}: {y} < {last}");
                assert!(
                    (0.0..=1.0).contains(&y),
                    "{curve:?} leaves 0..1 at {x}: {y}"
                );
                last = y;
            }
        }
    }

    #[test]
    fn mid_grey_lands_as_documented() {
        let table = [
            (Tonemap::None, 0.18),
            (Tonemap::Aces, 0.106),
            (Tonemap::Agx, 0.215),
            (Tonemap::Reinhard, 0.153),
            (Tonemap::Neutral, 0.14),
        ];
        let wrong: Vec<String> = table
            .iter()
            .map(|(curve, want)| (curve, want, curve.apply(Vec3::splat(0.18)).y))
            .filter(|(_, want, got)| (got - **want).abs() >= 0.0015)
            .map(|(curve, want, got)| format!("{curve:?}: {got}, documented {want}"))
            .collect();
        assert!(wrong.is_empty(), "{wrong:?}");
    }
}
