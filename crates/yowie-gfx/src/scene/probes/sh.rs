//! Second-order (L2) spherical harmonics on the processor: projecting a
//! captured cube onto nine coefficients per colour channel and evaluating
//! them. Matches `grid_irradiance` in `probes.wgsl`.

use yowie_core::Vec3;

use super::capture::capture_dir;

/// Coefficients per channel.
pub(crate) const COEFFS: usize = 9;
/// Floats per cell: nine coefficients of red, green and blue, packed
/// `c0.r c0.g c0.b c1.r ...`.
pub(crate) const CELL_FLOATS: usize = COEFFS * 3;

/// The cosine lobe's band factors over pi: coefficients of radiance times
/// these evaluate to irradiance over pi.
const LOBE: [f32; COEFFS] = [
    1.0,
    2.0 / 3.0,
    2.0 / 3.0,
    2.0 / 3.0,
    0.25,
    0.25,
    0.25,
    0.25,
    0.25,
];

/// The nine real basis functions at unit direction `d`.
pub(crate) fn basis(d: Vec3) -> [f32; COEFFS] {
    [
        0.282_095,
        0.488_603 * d.y,
        0.488_603 * d.z,
        0.488_603 * d.x,
        1.092_548 * d.x * d.y,
        1.092_548 * d.y * d.z,
        0.315_392 * (3.0 * d.z * d.z - 1.0),
        1.092_548 * d.x * d.z,
        0.546_274 * (d.x * d.x - d.y * d.y),
    ]
}

/// Projects a capture (six faces of `size` by `size` linear RGB texels in
/// face order, rows from the top, stored in the capture convention) onto
/// the basis, convolved with the cosine lobe.
pub(crate) fn project(texels: &[[f32; 3]], size: usize) -> [f32; CELL_FLOATS] {
    let mut acc = [0.0f64; CELL_FLOATS];
    let mut total = 0.0f64;
    let step = 2.0 / size as f32;
    for face in 0..6 {
        for y in 0..size {
            let v = (y as f32 + 0.5) * step - 1.0;
            for x in 0..size {
                let u = (x as f32 + 0.5) * step - 1.0;
                let Some(texel) = texels.get((face * size + y) * size + x) else {
                    continue;
                };
                // The solid angle a texel at (u, v) covers, up to a
                // constant the normalisation below removes.
                let w = 1.0 / (1.0 + u * u + v * v).powf(1.5);
                let b = basis(capture_dir(face, u, v).normalize());
                for (i, bi) in b.iter().enumerate() {
                    for (c, t) in texel.iter().enumerate() {
                        acc[i * 3 + c] += f64::from(t * bi * w);
                    }
                }
                total += f64::from(w);
            }
        }
    }
    let mut out = [0.0f32; CELL_FLOATS];
    if total <= 0.0 {
        return out;
    }
    let norm = 4.0 * std::f64::consts::PI / total;
    for (i, o) in out.iter_mut().enumerate() {
        *o = (acc[i] * norm) as f32 * LOBE[i / 3];
    }
    out
}

/// Evaluates convolved coefficients towards unit normal `n`: irradiance
/// over pi, clamped at zero.
pub(crate) fn eval(cell: &[f32; CELL_FLOATS], n: Vec3) -> Vec3 {
    let b = basis(n);
    let mut e = Vec3::ZERO;
    for (i, bi) in b.iter().enumerate() {
        e += Vec3::new(cell[i * 3], cell[i * 3 + 1], cell[i * 3 + 2]) * *bi;
    }
    e.max(Vec3::ZERO)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn capture(size: usize, light: impl Fn(Vec3) -> [f32; 3]) -> Vec<[f32; 3]> {
        let mut t = Vec::new();
        let step = 2.0 / size as f32;
        for face in 0..6 {
            for y in 0..size {
                for x in 0..size {
                    let u = (x as f32 + 0.5) * step - 1.0;
                    let v = (y as f32 + 0.5) * step - 1.0;
                    t.push(light(capture_dir(face, u, v).normalize()));
                }
            }
        }
        t
    }

    #[test]
    fn constant_light_evaluates_to_itself() {
        let cell = project(&capture(8, |_| [0.5, 1.0, 2.0]), 8);
        for n in [Vec3::X, Vec3::NEG_Y, Vec3::new(0.3, 0.4, -0.8).normalize()] {
            let e = eval(&cell, n);
            assert!((e - Vec3::new(0.5, 1.0, 2.0)).length() < 1e-3, "{n}: {e}");
        }
    }

    #[test]
    fn light_from_one_side_lights_that_side() {
        // A hemisphere of red light around +X: a normal facing it sees half
        // the sky lit at most, cos-weighted, so close to the full value.
        let cell = project(
            &capture(16, |d| if d.x > 0.0 { [1.0, 0.0, 0.0] } else { [0.0; 3] }),
            16,
        );
        let toward = eval(&cell, Vec3::X);
        let away = eval(&cell, Vec3::NEG_X);
        let side = eval(&cell, Vec3::Z);
        assert!(toward.x > 0.85 && toward.x < 1.1, "{toward}");
        assert!(away.x < 0.1, "{away}");
        assert!((side.x - 0.5).abs() < 0.05, "{side}");
        assert!(toward.y.abs() < 1e-4);
    }
}
