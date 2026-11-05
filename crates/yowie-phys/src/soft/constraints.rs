//! XPBD constraint projections for 3D particles.
//!
//! Each projection moves particles to reduce a constraint error `C`. The
//! multiplier `lambda` accumulates over the iterations of one substep;
//! `alpha` is `compliance / h^2` for substep length `h`, so a compliance of
//! zero is rigid and larger values are softer.

use yowie_core::{Mat3, Quat, Vec3};

/// A distance constraint between two particles. Cloth uses these for
/// stretch, shear and bending (links across a hinge between triangles).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Link {
    /// First particle.
    pub a: u32,
    /// Second particle.
    pub b: u32,
    /// Distance the constraint holds the particles at.
    pub rest: f32,
    /// Softness in metres per newton; 0 is rigid.
    pub compliance: f32,
}

/// A volume constraint on four particles forming a tetrahedron.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tet {
    /// The four particle indices.
    pub p: [u32; 4],
    /// Signed volume at rest.
    pub rest: f32,
    /// Softness; 0 is rigid.
    pub compliance: f32,
}

/// Signed volume of the tetrahedron `p0 p1 p2 p3`.
pub fn tet_volume(p0: Vec3, p1: Vec3, p2: Vec3, p3: Vec3) -> f32 {
    (p1 - p0).cross(p2 - p0).dot(p3 - p0) / 6.0
}

/// Signed volume enclosed by a closed triangle surface with outward
/// counter-clockwise winding.
pub fn enclosed_volume(pos: &[Vec3], tris: &[[u32; 3]]) -> f32 {
    tris.iter()
        .map(|t| {
            let [a, b, c] = t.map(|i| pos[i as usize]);
            a.dot(b.cross(c)) / 6.0
        })
        .sum()
}

/// Projects one distance constraint.
pub fn project_link(pos: &mut [Vec3], inv: &[f32], l: &Link, lambda: &mut f32, alpha: f32) {
    let (a, b) = (l.a as usize, l.b as usize);
    let (wa, wb) = (inv[a], inv[b]);
    let w = wa + wb;
    if w == 0.0 {
        return;
    }
    let d = pos[a] - pos[b];
    let len = d.length();
    if len < 1.0e-9 {
        return;
    }
    let n = d / len;
    let c = len - l.rest;
    let dl = (-c - alpha * *lambda) / (w + alpha);
    *lambda += dl;
    pos[a] += n * (wa * dl);
    pos[b] -= n * (wb * dl);
}

/// Projects one tetrahedron volume constraint.
pub fn project_tet(pos: &mut [Vec3], inv: &[f32], t: &Tet, lambda: &mut f32, alpha: f32) {
    let [i0, i1, i2, i3] = t.p.map(|i| i as usize);
    let (p0, p1, p2, p3) = (pos[i0], pos[i1], pos[i2], pos[i3]);
    let g1 = (p2 - p0).cross(p3 - p0) / 6.0;
    let g2 = (p3 - p0).cross(p1 - p0) / 6.0;
    let g3 = (p1 - p0).cross(p2 - p0) / 6.0;
    let g0 = -(g1 + g2 + g3);
    let denom = inv[i0] * g0.length_squared()
        + inv[i1] * g1.length_squared()
        + inv[i2] * g2.length_squared()
        + inv[i3] * g3.length_squared();
    if denom + alpha < 1.0e-12 {
        return;
    }
    let c = tet_volume(p0, p1, p2, p3) - t.rest;
    let dl = (-c - alpha * *lambda) / (denom + alpha);
    *lambda += dl;
    pos[i0] += g0 * (inv[i0] * dl);
    pos[i1] += g1 * (inv[i1] * dl);
    pos[i2] += g2 * (inv[i2] * dl);
    pos[i3] += g3 * (inv[i3] * dl);
}

/// A constraint that holds the volume enclosed by a closed surface at
/// `scale` times its rest volume. It keeps a soft body from deflating.
#[derive(Debug, Clone, PartialEq)]
pub struct Pressure {
    /// Outward-facing triangles of the closed surface.
    pub tris: Vec<[u32; 3]>,
    /// Enclosed volume at rest.
    pub rest: f32,
    /// Target volume as a multiple of `rest`; above 1 inflates.
    pub scale: f32,
    /// Softness; 0 is rigid.
    pub compliance: f32,
}

/// Projects the enclosed-volume constraint. `grad` is scratch space of at
/// least `pos.len()` elements.
pub fn project_pressure(
    pos: &mut [Vec3],
    inv: &[f32],
    p: &Pressure,
    grad: &mut [Vec3],
    lambda: &mut f32,
    alpha: f32,
) {
    grad.fill(Vec3::ZERO);
    for t in &p.tris {
        let [a, b, c] = t.map(|i| i as usize);
        grad[a] += pos[b].cross(pos[c]) / 6.0;
        grad[b] += pos[c].cross(pos[a]) / 6.0;
        grad[c] += pos[a].cross(pos[b]) / 6.0;
    }
    let denom: f32 = grad
        .iter()
        .zip(inv)
        .map(|(g, w)| w * g.length_squared())
        .sum();
    if denom + alpha < 1.0e-12 {
        return;
    }
    let c = enclosed_volume(pos, &p.tris) - p.rest * p.scale;
    let dl = (-c - alpha * *lambda) / (denom + alpha);
    *lambda += dl;
    for ((x, g), w) in pos.iter_mut().zip(grad.iter()).zip(inv) {
        *x += *g * (w * dl);
    }
}

/// Pulls a group of particles toward a rigidly transformed copy of their
/// rest shape, which keeps a soft body from collapsing or twisting.
#[derive(Debug, Clone, PartialEq)]
pub struct ShapeMatch {
    /// Rest positions relative to the rest centre of mass.
    pub rest: Vec<Vec3>,
    /// Fraction of the distance to the goal covered per substep, in `[0, 1]`.
    pub stiffness: f32,
}

impl ShapeMatch {
    /// Captures the rest shape of `pos` with the given inverse masses.
    pub fn new(pos: &[Vec3], inv: &[f32], stiffness: f32) -> Self {
        let (c, _) = centre(pos, inv);
        Self {
            rest: pos.iter().map(|p| *p - c).collect(),
            stiffness: stiffness.clamp(0.0, 1.0),
        }
    }

    /// Moves every free particle toward its goal position.
    pub fn apply(&self, pos: &mut [Vec3], inv: &[f32]) {
        let (c, total) = centre(pos, inv);
        if total == 0.0 {
            return;
        }
        let mut a = Mat3::ZERO;
        for ((p, r), w) in pos.iter().zip(&self.rest).zip(inv) {
            if *w > 0.0 {
                let m = 1.0 / w;
                let q = *p - c;
                a += Mat3::from_cols(q * r.x, q * r.y, q * r.z) * m;
            }
        }
        let rot = Mat3::from_quat(extract_rotation(&a));
        for ((p, r), w) in pos.iter_mut().zip(&self.rest).zip(inv) {
            if *w > 0.0 {
                *p += (c + rot * *r - *p) * self.stiffness;
            }
        }
    }
}

fn centre(pos: &[Vec3], inv: &[f32]) -> (Vec3, f32) {
    let (mut sum, mut total) = (Vec3::ZERO, 0.0);
    for (p, w) in pos.iter().zip(inv) {
        if *w > 0.0 {
            let m = 1.0 / w;
            sum += *p * m;
            total += m;
        }
    }
    if total > 0.0 {
        (sum / total, total)
    } else {
        (Vec3::ZERO, 0.0)
    }
}

/// Finds the rotation closest to the linear map `a` by iterating on a
/// quaternion (Muller et al., "A robust method to extract the rotational
/// part of deformations").
fn extract_rotation(a: &Mat3) -> Quat {
    let mut q = Quat::IDENTITY;
    for _ in 0..16 {
        let r = Mat3::from_quat(q);
        let omega = r.x_axis.cross(a.x_axis) + r.y_axis.cross(a.y_axis) + r.z_axis.cross(a.z_axis);
        let denom = (r.x_axis.dot(a.x_axis) + r.y_axis.dot(a.y_axis) + r.z_axis.dot(a.z_axis))
            .abs()
            + 1.0e-9;
        let omega = omega / denom;
        let w = omega.length();
        if w < 1.0e-9 {
            break;
        }
        q = (Quat::from_axis_angle(omega / w, w) * q).normalize();
    }
    q
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn link_projection_restores_rest_length() {
        let mut pos = vec![Vec3::ZERO, Vec3::new(3.0, 0.0, 0.0)];
        let inv = [1.0, 1.0];
        let link = Link {
            a: 0,
            b: 1,
            rest: 1.0,
            compliance: 0.0,
        };
        let mut lambda = 0.0;
        project_link(&mut pos, &inv, &link, &mut lambda, 0.0);
        assert!(((pos[1] - pos[0]).length() - 1.0).abs() < 1e-5);
        // A pinned particle does not move.
        let mut pos = vec![Vec3::ZERO, Vec3::new(3.0, 0.0, 0.0)];
        let mut lambda = 0.0;
        project_link(&mut pos, &[0.0, 1.0], &link, &mut lambda, 0.0);
        assert_eq!(pos[0], Vec3::ZERO);
        assert!((pos[1].x - 1.0).abs() < 1e-5);
    }

    #[test]
    fn tet_projection_restores_volume() {
        let mut pos = vec![Vec3::ZERO, Vec3::X, Vec3::Y, Vec3::Z * 0.5];
        let rest = tet_volume(Vec3::ZERO, Vec3::X, Vec3::Y, Vec3::Z);
        let tet = Tet {
            p: [0, 1, 2, 3],
            rest,
            compliance: 0.0,
        };
        let mut lambda = 0.0;
        for _ in 0..30 {
            project_tet(&mut pos, &[1.0; 4], &tet, &mut lambda, 0.0);
            lambda = 0.0;
        }
        let v = tet_volume(pos[0], pos[1], pos[2], pos[3]);
        assert!((v - rest).abs() < 1e-3, "{v} vs {rest}");
    }

    #[test]
    fn shape_match_recovers_rotation() {
        let rest = vec![
            Vec3::X,
            -Vec3::X,
            Vec3::Y,
            -Vec3::Y,
            Vec3::Z,
            -Vec3::Z * 2.0,
        ];
        let inv = vec![1.0; rest.len()];
        let m = ShapeMatch::new(&rest, &inv, 1.0);
        let q = Quat::from_axis_angle(Vec3::new(1.0, 2.0, 3.0).normalize(), 0.7);
        let mut pos: Vec<_> = rest.iter().map(|p| q * *p + Vec3::splat(5.0)).collect();
        let want = pos.clone();
        pos[0] += Vec3::new(0.2, 0.0, 0.0);
        m.apply(&mut pos, &inv);
        for (p, w) in pos.iter().zip(&want) {
            assert!((*p - *w).length() < 0.1, "{p:?} vs {w:?}");
        }
    }

    #[test]
    fn unit_cube_surface_volume() {
        let pos = vec![
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(1.0, 1.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(0.0, 0.0, 1.0),
            Vec3::new(1.0, 0.0, 1.0),
            Vec3::new(1.0, 1.0, 1.0),
            Vec3::new(0.0, 1.0, 1.0),
        ];
        let q = [
            [0, 3, 2, 1],
            [4, 5, 6, 7],
            [0, 1, 5, 4],
            [2, 3, 7, 6],
            [1, 2, 6, 5],
            [3, 0, 4, 7],
        ];
        let tris: Vec<[u32; 3]> = q
            .iter()
            .flat_map(|f| [[f[0], f[1], f[2]], [f[0], f[2], f[3]]])
            .collect();
        assert!((enclosed_volume(&pos, &tris) - 1.0).abs() < 1e-5);
    }
}
