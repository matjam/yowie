//! Converting `f32` to IEEE 754 half precision for 16-bit float textures.

/// Converts to the nearest half-precision value (ties to even). Values
/// beyond the half range become infinity; NaN stays NaN.
pub(crate) fn f32_to_f16(value: f32) -> u16 {
    let bits = value.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let exp = ((bits >> 23) & 0xff) as i32;
    let man = bits & 0x007f_ffff;
    if exp == 0xff {
        // Infinity keeps a zero mantissa; NaN gets a quiet bit.
        return sign | 0x7c00 | if man != 0 { 0x0200 } else { 0 };
    }
    let e = exp - 127 + 15;
    if e >= 0x1f {
        return sign | 0x7c00;
    }
    if e <= 0 {
        // Subnormal half, or zero when the value is too small.
        if e < -10 {
            return sign;
        }
        let m = man | 0x0080_0000;
        let shift = (14 - e) as u32;
        let half = m >> shift;
        let rem = m & ((1 << shift) - 1);
        let halfway = 1 << (shift - 1);
        let round = u32::from(rem > halfway || (rem == halfway && half & 1 == 1));
        return sign | (half + round) as u16;
    }
    let half = ((e as u32) << 10) | (man >> 13);
    let rem = man & 0x1fff;
    let round = u32::from(rem > 0x1000 || (rem == 0x1000 && half & 1 == 1));
    // A carry out of the mantissa rolls into the exponent, which is correct
    // (and reaches infinity at the top of the range).
    sign | (half + round) as u16
}

/// Converts a half-precision value to `f32` exactly.
pub(crate) fn f16_to_f32(h: u16) -> f32 {
    let sign = u32::from(h & 0x8000) << 16;
    let exp = u32::from((h >> 10) & 0x1f);
    let man = u32::from(h & 0x03ff);
    let bits = match (exp, man) {
        (0, 0) => sign,
        (0, m) => {
            // Subnormal: normalise the mantissa.
            let shift = m.leading_zeros() - 21;
            sign | ((113 - shift) << 23) | ((m << shift) & 0x03ff) << 13
        }
        (0x1f, m) => sign | 0x7f80_0000 | (m << 13),
        (e, m) => sign | ((e + 112) << 23) | (m << 13),
    };
    f32::from_bits(bits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_values() {
        let cases: [(f32, u16); 10] = [
            (0.0, 0x0000),
            (-0.0, 0x8000),
            (1.0, 0x3c00),
            (-2.0, 0xc000),
            (0.5, 0x3800),
            (65504.0, 0x7bff),
            (65520.0, 0x7c00),
            (f32::INFINITY, 0x7c00),
            (5.960_464_5e-8, 0x0001),
            (1.0e-10, 0x0000),
        ];
        for (value, want) in cases {
            assert_eq!(f32_to_f16(value), want, "{value}");
        }
        assert_eq!(f32_to_f16(f32::NAN) & 0x7c00, 0x7c00);
        assert_ne!(f32_to_f16(f32::NAN) & 0x03ff, 0);
    }

    #[test]
    fn every_half_survives_the_round_trip() {
        for h in 0..=u16::MAX {
            let f = f16_to_f32(h);
            if f.is_nan() {
                assert!((h & 0x7c00) == 0x7c00 && (h & 0x03ff) != 0, "{h:#06x}");
                continue;
            }
            assert_eq!(f32_to_f16(f), h, "{h:#06x} -> {f}");
        }
    }
}
