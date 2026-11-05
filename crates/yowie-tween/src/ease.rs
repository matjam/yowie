use std::f32::consts::PI;

/// A family of easing shapes. Each shape has an [`Ease::In`] form that
/// starts slowly, an [`Ease::Out`] form that ends slowly, and an
/// [`Ease::InOut`] form that does both.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Curve {
    /// Quadratic, `t^2`.
    Quad,
    /// Cubic, `t^3`.
    Cubic,
    /// Quartic, `t^4`.
    Quart,
    /// Quintic, `t^5`.
    Quint,
    /// Sine quarter wave.
    Sine,
    /// Exponential, `2^(10t - 10)`.
    Expo,
    /// Circular arc.
    Circ,
    /// Overshoots the target before settling.
    Back,
    /// Oscillates around the target like a spring.
    Elastic,
    /// Bounces against the target like a dropped ball.
    Bounce,
}

/// Maps linear progress in `0..=1` to eased progress.
///
/// Back and Elastic curves leave `0..=1` on the way; all curves start at 0
/// and end at 1. [`Ease::InOut`] is the In form played over the first half
/// and the Out form over the second, so a Back in-out overshoots by the same
/// amount as a Back in.
#[derive(Clone, Copy, Debug)]
pub enum Ease {
    /// No easing.
    Linear,
    /// `3t^2 - 2t^3`: zero slope at both ends.
    Smoothstep,
    /// Jumps through this many equal levels, from 0 up to 1 inclusive. At
    /// least 1; one step stays at 0 until progress reaches 1.
    Steps(u32),
    /// A CSS-style cubic Bézier through (0,0) and (1,1) with control points
    /// `(x1, y1)` and `(x2, y2)`; `x1` and `x2` should lie in `0..=1`.
    Bezier(f32, f32, f32, f32),
    /// A caller-supplied function of progress in `0..=1`.
    Custom(fn(f32) -> f32),
    /// Slow start.
    In(Curve),
    /// Slow end.
    Out(Curve),
    /// Slow start and end.
    InOut(Curve),
}

impl Ease {
    /// Returns the eased progress for linear progress `t`. `t` is clamped
    /// to `0..=1` first.
    pub fn apply(&self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match *self {
            Ease::Linear => t,
            Ease::Smoothstep => t * t * (3.0 - 2.0 * t),
            Ease::Steps(n) => {
                let n = n.max(1);
                if n == 1 {
                    return if t >= 1.0 { 1.0 } else { 0.0 };
                }
                let level = (t * n as f32).floor().min((n - 1) as f32);
                level / (n - 1) as f32
            }
            Ease::Bezier(x1, y1, x2, y2) => bezier(x1, y1, x2, y2, t),
            Ease::Custom(f) => f(t),
            Ease::In(c) => ease_in(c, t),
            Ease::Out(c) => ease_out(c, t),
            Ease::InOut(c) => {
                if t < 0.5 {
                    ease_in(c, t * 2.0) / 2.0
                } else {
                    0.5 + ease_out(c, t * 2.0 - 1.0) / 2.0
                }
            }
        }
    }
}

fn ease_in(c: Curve, t: f32) -> f32 {
    1.0 - ease_out(c, 1.0 - t)
}

fn ease_out(c: Curve, t: f32) -> f32 {
    const C1: f32 = 1.70158;
    const C3: f32 = C1 + 1.0;
    const C4: f32 = 2.0 * PI / 3.0;
    let u = 1.0 - t;
    match c {
        Curve::Quad => 1.0 - u * u,
        Curve::Cubic => 1.0 - u * u * u,
        Curve::Quart => 1.0 - u.powi(4),
        Curve::Quint => 1.0 - u.powi(5),
        Curve::Sine => (t * PI / 2.0).sin(),
        Curve::Expo => {
            if t >= 1.0 {
                1.0
            } else {
                1.0 - 2f32.powf(-10.0 * t)
            }
        }
        Curve::Circ => (1.0 - u * u).max(0.0).sqrt(),
        Curve::Back => 1.0 - C3 * u * u * u + C1 * u * u,
        Curve::Elastic => {
            if t <= 0.0 || t >= 1.0 {
                t
            } else {
                2f32.powf(-10.0 * t) * ((t * 10.0 - 0.75) * C4).sin() + 1.0
            }
        }
        Curve::Bounce => bounce_out(t),
    }
}

fn bounce_out(mut t: f32) -> f32 {
    const N1: f32 = 7.5625;
    const D1: f32 = 2.75;
    if t < 1.0 / D1 {
        N1 * t * t
    } else if t < 2.0 / D1 {
        t -= 1.5 / D1;
        N1 * t * t + 0.75
    } else if t < 2.5 / D1 {
        t -= 2.25 / D1;
        N1 * t * t + 0.9375
    } else {
        t -= 2.625 / D1;
        N1 * t * t + 0.984375
    }
}

/// Solves x(s) = t for the Bézier parameter s by Newton steps with a
/// bisection fallback, then returns y(s).
fn bezier(x1: f32, y1: f32, x2: f32, y2: f32, t: f32) -> f32 {
    let curve = |a: f32, b: f32, s: f32| {
        let u = 1.0 - s;
        3.0 * u * u * s * a + 3.0 * u * s * s * b + s * s * s
    };
    let slope = |a: f32, b: f32, s: f32| {
        let u = 1.0 - s;
        3.0 * u * u * a + 6.0 * u * s * (b - a) + 3.0 * s * s * (1.0 - b)
    };
    let mut s = t;
    for _ in 0..8 {
        let err = curve(x1, x2, s) - t;
        if err.abs() < 1e-6 {
            return curve(y1, y2, s);
        }
        let d = slope(x1, x2, s);
        if d.abs() < 1e-6 {
            break;
        }
        s -= err / d;
    }
    let (mut lo, mut hi) = (0.0_f32, 1.0_f32);
    s = t;
    for _ in 0..32 {
        if curve(x1, x2, s) < t {
            lo = s;
        } else {
            hi = s;
        }
        s = (lo + hi) / 2.0;
    }
    curve(y1, y2, s)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CURVES: [Curve; 10] = [
        Curve::Quad,
        Curve::Cubic,
        Curve::Quart,
        Curve::Quint,
        Curve::Sine,
        Curve::Expo,
        Curve::Circ,
        Curve::Back,
        Curve::Elastic,
        Curve::Bounce,
    ];

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    #[test]
    fn every_ease_starts_at_zero_and_ends_at_one() {
        let mut eases = vec![
            Ease::Linear,
            Ease::Smoothstep,
            Ease::Bezier(0.25, 0.1, 0.25, 1.0),
        ];
        for c in CURVES {
            eases.extend([Ease::In(c), Ease::Out(c), Ease::InOut(c)]);
        }
        for e in eases {
            assert!(close(e.apply(0.0), 0.0), "{e:?} at 0 = {}", e.apply(0.0));
            assert!(close(e.apply(1.0), 1.0), "{e:?} at 1 = {}", e.apply(1.0));
        }
    }

    #[test]
    fn known_values() {
        let cases = [
            (Ease::Linear, 0.5, 0.5),
            (Ease::In(Curve::Quad), 0.5, 0.25),
            (Ease::Out(Curve::Quad), 0.5, 0.75),
            (Ease::InOut(Curve::Quad), 0.25, 0.125),
            (Ease::In(Curve::Cubic), 0.5, 0.125),
            (Ease::Out(Curve::Cubic), 0.5, 0.875),
            (Ease::In(Curve::Quart), 0.5, 0.0625),
            (Ease::In(Curve::Quint), 0.5, 0.03125),
            (Ease::InOut(Curve::Sine), 0.5, 0.5),
            (Ease::In(Curve::Expo), 0.5, 2f32.powf(-5.0)),
            (Ease::Smoothstep, 0.25, 0.15625),
            (Ease::Out(Curve::Bounce), 0.5, 0.765625),
            (Ease::Linear, 2.0, 1.0),
            (Ease::Linear, -1.0, 0.0),
        ];
        for (e, t, want) in cases {
            assert!(
                close(e.apply(t), want),
                "{e:?}({t}) = {} want {want}",
                e.apply(t)
            );
        }
    }

    #[test]
    fn non_overshooting_curves_are_monotonic() {
        let curves = [
            Curve::Quad,
            Curve::Cubic,
            Curve::Quart,
            Curve::Quint,
            Curve::Sine,
            Curve::Expo,
            Curve::Circ,
        ];
        for c in curves {
            for e in [Ease::In(c), Ease::Out(c), Ease::InOut(c)] {
                let mut prev = 0.0;
                for i in 0..=100 {
                    let v = e.apply(i as f32 / 100.0);
                    assert!(v >= prev - 1e-5, "{e:?} dips at {i}");
                    prev = v;
                }
            }
        }
    }

    #[test]
    fn back_and_elastic_overshoot() {
        let peak = |e: Ease| {
            (0..=100)
                .map(|i| e.apply(i as f32 / 100.0))
                .fold(0.0, f32::max)
        };
        assert!(peak(Ease::Out(Curve::Back)) > 1.05);
        assert!(peak(Ease::Out(Curve::Elastic)) > 1.05);
        assert!(peak(Ease::Out(Curve::Bounce)) <= 1.0 + 1e-5);
    }

    #[test]
    fn steps_quantise() {
        let cases = [
            (0.0, 0.0),
            (0.24, 0.0),
            (0.26, 1.0 / 3.0),
            (0.7, 2.0 / 3.0),
            (1.0, 1.0),
        ];
        for (t, want) in cases {
            assert!(
                close(Ease::Steps(4).apply(t), want),
                "t {t}: {}",
                Ease::Steps(4).apply(t)
            );
        }
        assert_eq!(Ease::Steps(0).apply(0.5), 0.0);
        assert_eq!(Ease::Steps(0).apply(1.0), 1.0);
    }

    #[test]
    fn bezier_linear_and_custom() {
        let lin = Ease::Bezier(0.0, 0.0, 1.0, 1.0);
        for i in 0..=10 {
            let t = i as f32 / 10.0;
            assert!(close(lin.apply(t), t));
        }
        let ease_in = Ease::Bezier(0.42, 0.0, 1.0, 1.0);
        assert!(ease_in.apply(0.5) < 0.5);
        assert!(close(Ease::Custom(|t| t * t).apply(0.5), 0.25));
    }
}
