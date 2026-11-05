use std::f64::consts::{PI, TAU};

/// Wraps an angle into `[0, 2π)`.
pub fn wrap_angle(a: f64) -> f64 {
    let w = a.rem_euclid(TAU);
    // rem_euclid can return exactly TAU for tiny negative inputs.
    if w >= TAU { 0.0 } else { w }
}

/// Wraps an angle into `(-π, π]`.
fn wrap_signed(a: f64) -> f64 {
    let w = wrap_angle(a);
    if w > PI { w - TAU } else { w }
}

/// Solves Kepler's equation `M = E - e sin E` for the eccentric anomaly
/// `E`, for an elliptic orbit (`0 <= e < 1`). `mean_anomaly` is in radians
/// and may be any value; the result is in `[0, 2π)`.
pub fn solve_kepler(mean_anomaly: f64, e: f64) -> f64 {
    let e = e.clamp(0.0, 1.0 - 1e-15);
    let m = wrap_angle(mean_anomaly);
    let mut ea = if e > 0.8 { PI } else { m };
    for _ in 0..100 {
        let step = (ea - e * ea.sin() - m) / (1.0 - e * ea.cos());
        ea -= step;
        if step.abs() < 1e-14 {
            break;
        }
    }
    ea
}

/// Solves the hyperbolic Kepler equation `M = e sinh H - H` for the
/// hyperbolic anomaly `H` (`e > 1`). `M` is not periodic; any value is
/// valid.
pub fn solve_kepler_hyperbolic(mean_anomaly: f64, e: f64) -> f64 {
    let e = e.max(1.0 + 1e-15);
    let m = mean_anomaly;
    let mut h = if m == 0.0 {
        0.0
    } else {
        m.signum() * (2.0 * m.abs() / e + 1.8).ln()
    };
    for _ in 0..100 {
        let step = (e * h.sinh() - h - m) / (e * h.cosh() - 1.0);
        h -= step;
        if step.abs() < 1e-14 * h.abs().max(1.0) {
            break;
        }
    }
    h
}

/// Converts a mean anomaly to a true anomaly for an elliptic orbit. The
/// result is in `[0, 2π)`.
pub fn true_anomaly_from_mean(mean_anomaly: f64, e: f64) -> f64 {
    let ea = solve_kepler(mean_anomaly, e);
    wrap_angle(
        2.0 * f64::atan2(
            (1.0 + e).sqrt() * (ea / 2.0).sin(),
            (1.0 - e).sqrt() * (ea / 2.0).cos(),
        ),
    )
}

/// Converts a true anomaly to a mean anomaly for an elliptic orbit. The
/// result is in `[0, 2π)`.
pub fn mean_anomaly_from_true(true_anomaly: f64, e: f64) -> f64 {
    let ea = 2.0
        * f64::atan2(
            (1.0 - e).sqrt() * (true_anomaly / 2.0).sin(),
            (1.0 + e).sqrt() * (true_anomaly / 2.0).cos(),
        );
    wrap_angle(ea - e * ea.sin())
}

/// Converts a hyperbolic mean anomaly to a true anomaly (`e > 1`). The
/// result is in `(-π, π)`: negative on the way in, positive on the way out.
pub fn true_anomaly_from_mean_hyperbolic(mean_anomaly: f64, e: f64) -> f64 {
    let h = solve_kepler_hyperbolic(mean_anomaly, e);
    2.0 * f64::atan2(
        (e + 1.0).sqrt() * (h / 2.0).sinh(),
        (e - 1.0).sqrt() * (h / 2.0).cosh(),
    )
}

/// Converts a true anomaly to a hyperbolic mean anomaly (`e > 1`). The true
/// anomaly is taken modulo 2π as an angle in `(-π, π]` and must lie within
/// the orbit's asymptotes, `|ν| < acos(-1/e)`.
pub fn mean_anomaly_from_true_hyperbolic(true_anomaly: f64, e: f64) -> f64 {
    let nu = wrap_signed(true_anomaly);
    let sinh_h = (e * e - 1.0).sqrt() * nu.sin() / (1.0 + e * nu.cos());
    let h = sinh_h.asinh();
    e * h.sinh() - h
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kepler_equation_residual_is_tiny() {
        let es = [0.0, 0.1, 0.5, 0.9, 0.99, 0.999];
        for e in es {
            for i in 0..=40 {
                let m = f64::from(i) * TAU / 40.0;
                let ea = solve_kepler(m, e);
                let residual = ea - e * ea.sin() - wrap_angle(m);
                assert!(residual.abs() < 1e-11, "e {e} m {m}: {residual}");
            }
        }
    }

    #[test]
    fn hyperbolic_residual_is_tiny() {
        for e in [1.01, 1.5, 3.0, 10.0] {
            for m in [-50.0, -3.0, -0.1, 0.0, 0.2, 2.0, 40.0] {
                let h = solve_kepler_hyperbolic(m, e);
                let residual = e * h.sinh() - h - m;
                assert!(
                    residual.abs() < 1e-9 * m.abs().max(1.0),
                    "e {e} m {m}: {residual}"
                );
            }
        }
    }

    #[test]
    fn anomaly_conversions_round_trip() {
        for e in [0.0, 0.3, 0.8, 0.97] {
            for i in 1..40 {
                let nu = f64::from(i) * TAU / 40.0;
                let back = true_anomaly_from_mean(mean_anomaly_from_true(nu, e), e);
                assert!((wrap_signed(back - nu)).abs() < 1e-9, "e {e} nu {nu}");
            }
        }
        for e in [1.1, 2.0, 5.0] {
            let limit = (-1.0_f64 / e).acos() * 0.95;
            for k in -8..=8 {
                let nu = limit * f64::from(k) / 8.0;
                let back =
                    true_anomaly_from_mean_hyperbolic(mean_anomaly_from_true_hyperbolic(nu, e), e);
                assert!((back - nu).abs() < 1e-9, "e {e} nu {nu}: {back}");
            }
        }
    }

    #[test]
    fn known_anomalies() {
        // A quarter period after periapsis of a circular orbit is 90 degrees.
        assert!((true_anomaly_from_mean(PI / 2.0, 0.0) - PI / 2.0).abs() < 1e-12);
        // Apoapsis is at M = pi for any eccentricity.
        assert!((true_anomaly_from_mean(PI, 0.7) - PI).abs() < 1e-12);
        assert_eq!(true_anomaly_from_mean(0.0, 0.7), 0.0);
        assert!(wrap_angle(-1e-20) < TAU);
        assert!((wrap_angle(-PI / 2.0) - 1.5 * PI).abs() < 1e-12);
    }
}
