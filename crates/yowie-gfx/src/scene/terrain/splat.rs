//! Painting the splat map, the four layer weights of every sample, from
//! the ground's height and slope.

use super::heightmap::HeightMap;

/// Where one ground layer covers the terrain: a band of world heights and
/// a band of slopes, each with soft edges. A sample's weight for the
/// layer is the product of how far inside both bands it lies.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LayerRule {
    /// The lowest world height the layer covers.
    pub min_height: f32,
    /// The highest world height the layer covers.
    pub max_height: f32,
    /// The gentlest slope the layer covers, in radians from level.
    pub min_slope: f32,
    /// The steepest slope the layer covers, in radians from level.
    pub max_slope: f32,
    /// The world units over which the height edges fade.
    pub height_blend: f32,
    /// The radians over which the slope edges fade.
    pub slope_blend: f32,
}

impl Default for LayerRule {
    fn default() -> Self {
        Self::EVERYWHERE
    }
}

impl LayerRule {
    /// A rule covering every height and slope.
    pub const EVERYWHERE: LayerRule = LayerRule {
        min_height: f32::NEG_INFINITY,
        max_height: f32::INFINITY,
        min_slope: -1.0,
        max_slope: std::f32::consts::FRAC_PI_2 + 1.0,
        height_blend: 1.0,
        slope_blend: 0.1,
    };

    /// The ground below world height `height`, fading over `blend` units.
    pub fn below(height: f32, blend: f32) -> Self {
        Self {
            max_height: height,
            height_blend: blend,
            ..Self::EVERYWHERE
        }
    }

    /// The ground above world height `height`, fading over `blend` units.
    pub fn above(height: f32, blend: f32) -> Self {
        Self {
            min_height: height,
            height_blend: blend,
            ..Self::EVERYWHERE
        }
    }

    /// The ground steeper than `angle` radians, fading over `blend`.
    pub fn steeper_than(angle: f32, blend: f32) -> Self {
        Self {
            min_slope: angle,
            slope_blend: blend,
            ..Self::EVERYWHERE
        }
    }

    /// The same rule limited to slopes gentler than `angle` radians.
    pub fn gentler_than(self, angle: f32) -> Self {
        Self {
            max_slope: angle,
            ..self
        }
    }

    /// The weight of the layer at world height `h` and slope `slope`.
    pub fn weight(&self, h: f32, slope: f32) -> f32 {
        band(h, self.min_height, self.max_height, self.height_blend)
            * band(slope, self.min_slope, self.max_slope, self.slope_blend)
    }
}

/// 1 inside `lo..hi`, falling to 0 over `blend` past either edge.
fn band(v: f32, lo: f32, hi: f32, blend: f32) -> f32 {
    let b = blend.max(1e-4);
    let rise = ((v - lo) / b + 0.5).clamp(0.0, 1.0);
    let fall = ((hi - v) / b + 0.5).clamp(0.0, 1.0);
    let s = |t: f32| t * t * (3.0 - 2.0 * t);
    s(rise) * s(fall)
}

/// How the splat map is painted from height and slope. The `base` layer
/// covers everything; each other layer with a rule is then painted over
/// what lies below it in layer order, so later layers win where rules
/// overlap.
///
/// The default suits layers of sand, grass, rock and snow in that order:
/// grass everywhere, sand below a height of 0.6, rock steeper than 33
/// degrees, snow above 14 on slopes gentler than 50 degrees.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SplatRules {
    /// The layer under all others.
    pub base: usize,
    /// Each layer's rule; `None` leaves the layer to a painted splat map.
    pub layers: [Option<LayerRule>; 4],
    /// World units of smooth jitter added to each sample's height before
    /// the rules, so the edges between layers wander instead of following
    /// contour lines.
    pub jitter: f32,
}

impl Default for SplatRules {
    fn default() -> Self {
        Self {
            base: 1,
            layers: [
                Some(LayerRule::below(0.6, 0.8)),
                None,
                Some(LayerRule::steeper_than(0.58, 0.12)),
                Some(LayerRule::above(14.0, 3.0).gentler_than(0.87)),
            ],
            jitter: 0.8,
        }
    }
}

impl SplatRules {
    /// The four layer weights, summing to one, at world height `h` and
    /// slope `slope` radians.
    pub fn weights(&self, h: f32, slope: f32) -> [f32; 4] {
        let mut w = [0.0; 4];
        let base = self.base.min(3);
        w[base] = 1.0;
        for (i, rule) in self.layers.iter().enumerate() {
            let Some(rule) = rule.filter(|_| i != base) else {
                continue;
            };
            let a = rule.weight(h, slope);
            for v in &mut w {
                *v *= 1.0 - a;
            }
            w[i] += a;
        }
        w
    }
}

/// A smooth value in -1 to 1 that wanders over about eight samples.
fn jitter(x: u32, z: u32) -> f32 {
    let hash = |x: i64, z: i64| {
        let mut h = (x as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
            ^ (z as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
        h ^= h >> 29;
        h = h.wrapping_mul(0xBF58_476D_1CE4_E5B9);
        h ^= h >> 32;
        (h & 0xffff) as f32 / 32767.5 - 1.0
    };
    let (fx, fz) = (x as f32 / 8.0, z as f32 / 8.0);
    let (ix, iz) = (fx.floor(), fz.floor());
    let (tx, tz) = (fx - ix, fz - iz);
    let s = |t: f32| t * t * (3.0 - 2.0 * t);
    let (ix, iz) = (ix as i64, iz as i64);
    let a = hash(ix, iz) + (hash(ix + 1, iz) - hash(ix, iz)) * s(tx);
    let b = hash(ix, iz + 1) + (hash(ix + 1, iz + 1) - hash(ix, iz + 1)) * s(tx);
    a + (b - a) * s(tz)
}

/// Paints the weights of the inclusive sample rectangle `(x0, z0)` to
/// `(x1, z1)` into `out`, which holds four bytes per sample of the whole
/// grid. `base_y` is added to every height and `cell` spaces the samples.
pub(crate) fn paint(
    map: &HeightMap,
    rules: &SplatRules,
    base_y: f32,
    cell: f32,
    rect: [u32; 4],
    out: &mut [u8],
) {
    let [x0, z0, x1, z1] = rect;
    let cols = map.cols();
    for z in z0..=z1.min(map.rows() - 1) {
        for x in x0..=x1.min(cols - 1) {
            let h = base_y + map.get(i64::from(x), i64::from(z)) + jitter(x, z) * rules.jitter;
            let n = map.normal(i64::from(x), i64::from(z), cell);
            let slope = n.y.clamp(-1.0, 1.0).acos();
            let w = rules.weights(h, slope);
            let at = (z * cols + x) as usize * 4;
            for c in 0..4 {
                out[at + c] = (w[c].clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_rules_pick_the_expected_layer() {
        let rules = SplatRules::default();
        let cases = [
            ("beach", -1.0, 0.0, 0),
            ("meadow", 5.0, 0.1, 1),
            ("cliff", 5.0, 1.0, 2),
            ("peak", 20.0, 0.2, 3),
            ("steep peak", 20.0, 1.2, 2),
        ];
        for (name, h, slope, want) in cases {
            let w = rules.weights(h, slope);
            let sum: f32 = w.iter().sum();
            assert!((sum - 1.0).abs() < 1e-4, "{name}: {w:?}");
            let best = (0..4).max_by(|a, b| w[*a].total_cmp(&w[*b])).unwrap_or(0);
            assert_eq!(best, want, "{name}: {w:?}");
        }
    }
}
