//! How a path is painted: fill rules, stroke shape and paints.

use std::hash::{Hash, Hasher};

use yowie_core::{Color, Vec2};

use crate::texture::{Texture, Wrap};

/// Which parts of a self-overlapping or nested path count as inside.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum FillRule {
    /// Inside where the signed count of times the outline winds around a
    /// point is not zero. Nested contours drawn in the same direction stay
    /// filled.
    #[default]
    NonZero,
    /// Inside where a ray from the point crosses the outline an odd number
    /// of times. Nested contours cut holes.
    EvenOdd,
}

/// The shape at the ends of an open stroked subpath.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Cap {
    /// The stroke ends flat at the endpoint.
    #[default]
    Butt,
    /// The stroke ends in a half circle centred on the endpoint.
    Round,
    /// The stroke extends half its width past the endpoint, squared off.
    Square,
}

/// The shape where two stroked segments meet.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Join {
    /// The outer edges extend to a point, up to the miter limit.
    #[default]
    Miter,
    /// The corner is a circular arc.
    Round,
    /// The corner is cut off flat.
    Bevel,
}

/// A dash pattern for strokes.
#[derive(Clone, Debug, PartialEq)]
pub struct Dash {
    /// Alternating lengths of drawn and skipped stretches, in drawing
    /// units, starting with a drawn one. An odd count repeats the pattern
    /// with the roles swapped. Lengths must be non-negative with a
    /// positive sum, or the stroke is solid.
    pub pattern: Vec<f32>,
    /// How far into the pattern the stroke starts.
    pub offset: f32,
}

impl Dash {
    /// A pattern of `on` drawn then `off` skipped, both in drawing units.
    pub fn new(on: f32, off: f32) -> Self {
        Self {
            pattern: vec![on, off],
            offset: 0.0,
        }
    }

    pub(crate) fn valid(&self) -> bool {
        !self.pattern.is_empty()
            && self.pattern.iter().all(|l| l.is_finite() && *l >= 0.0)
            && self.pattern.iter().sum::<f32>() > 0.0
    }
}

/// A colour at a position along a gradient.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GradientStop {
    /// Position from 0 (the start) to 1 (the end).
    pub offset: f32,
    /// The colour, sRGB with straight alpha.
    pub color: Color,
}

impl GradientStop {
    /// A stop at `offset` with `color`.
    pub fn new(offset: f32, color: Color) -> Self {
        Self { offset, color }
    }
}

/// Colour stops and what happens past the ends.
#[derive(Clone, Debug, PartialEq)]
pub struct Gradient {
    /// The stops, sorted by offset.
    pub stops: Vec<GradientStop>,
    /// Behaviour outside 0 to 1: [`Wrap::Clamp`] holds the end colours,
    /// [`Wrap::Repeat`] restarts and [`Wrap::Mirror`] bounces.
    pub spread: Wrap,
}

impl Gradient {
    /// A gradient through `stops`, which are sorted by offset. Colours are
    /// mixed in linear light with premultiplied alpha. An empty list is
    /// transparent.
    pub fn new(stops: &[GradientStop]) -> Self {
        let mut stops = stops.to_vec();
        stops.sort_by(|a, b| a.offset.total_cmp(&b.offset));
        Self {
            stops,
            spread: Wrap::Clamp,
        }
    }

    /// A gradient from `from` at 0 to `to` at 1.
    pub fn two(from: Color, to: Color) -> Self {
        Self::new(&[GradientStop::new(0.0, from), GradientStop::new(1.0, to)])
    }

    /// Sets the spread.
    pub fn spread(self, spread: Wrap) -> Self {
        Self { spread, ..self }
    }

    pub(crate) fn hash_key(&self) -> u64 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        for s in &self.stops {
            s.offset.to_bits().hash(&mut h);
            for c in s.color.to_array() {
                c.to_bits().hash(&mut h);
            }
        }
        (self.spread as u8).hash(&mut h);
        h.finish()
    }

    /// The premultiplied linear colour at `t` in 0 to 1.
    pub(crate) fn sample(&self, t: f32) -> [f32; 4] {
        let premult = |c: Color| c.to_linear_premultiplied();
        let (Some(first), Some(last)) = (self.stops.first(), self.stops.last()) else {
            return [0.0; 4];
        };
        if t <= first.offset {
            return premult(first.color);
        }
        if t >= last.offset {
            return premult(last.color);
        }
        for pair in self.stops.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            if t >= a.offset && t <= b.offset {
                let span = (b.offset - a.offset).max(1e-9);
                let f = (t - a.offset) / span;
                let (ca, cb) = (premult(a.color), premult(b.color));
                return std::array::from_fn(|i| ca[i] + (cb[i] - ca[i]) * f);
            }
        }
        premult(last.color)
    }
}

/// A texture painted across a path.
#[derive(Clone, Debug)]
pub struct TexturePaint {
    /// The texture. Its wrap mode decides what happens past its edges;
    /// use [`Wrap::Repeat`] to tile it.
    pub texture: Texture,
    /// Where the texture's top-left corner lands, in drawing units.
    pub origin: Vec2,
    /// Drawing units per texel.
    pub scale: Vec2,
    /// Multiplied with the texture, sRGB with straight alpha.
    pub tint: Color,
}

impl TexturePaint {
    /// A texture at its own size with its corner at `origin`.
    pub fn new(texture: &Texture, origin: Vec2) -> Self {
        Self {
            texture: texture.clone(),
            origin,
            scale: Vec2::ONE,
            tint: Color::WHITE,
        }
    }
}

/// What colours a filled or stroked path.
#[derive(Clone, Debug)]
pub enum Paint {
    /// One colour, sRGB with straight alpha.
    Solid(Color),
    /// Colours along the line from `start` to `end`, constant across it.
    Linear {
        /// Where the gradient's 0 lies.
        start: Vec2,
        /// Where the gradient's 1 lies.
        end: Vec2,
        /// The colours.
        gradient: Gradient,
    },
    /// Colours by distance from `center`, reaching 1 at `radius`.
    Radial {
        /// The gradient's centre.
        center: Vec2,
        /// The distance at which the gradient reaches 1.
        radius: f32,
        /// The colours.
        gradient: Gradient,
    },
    /// A texture.
    Texture(TexturePaint),
}

impl Paint {
    /// A linear gradient from `start` to `end` through `gradient`.
    pub fn linear(start: Vec2, end: Vec2, gradient: Gradient) -> Self {
        Paint::Linear {
            start,
            end,
            gradient,
        }
    }

    /// A radial gradient around `center` out to `radius`.
    pub fn radial(center: Vec2, radius: f32, gradient: Gradient) -> Self {
        Paint::Radial {
            center,
            radius,
            gradient,
        }
    }
}

impl From<Color> for Paint {
    fn from(color: Color) -> Self {
        Paint::Solid(color)
    }
}

/// How to fill a path.
#[derive(Clone, Debug)]
pub struct Fill {
    /// The colours.
    pub paint: Paint,
    /// Which regions are inside.
    pub rule: FillRule,
    /// Whether the edge is feathered by about one pixel. On by default.
    pub antialias: bool,
}

impl Fill {
    /// A fill with `paint`, non-zero rule and anti-aliasing on.
    pub fn new(paint: impl Into<Paint>) -> Self {
        Self {
            paint: paint.into(),
            rule: FillRule::NonZero,
            antialias: true,
        }
    }

    /// Sets the fill rule.
    pub fn rule(self, rule: FillRule) -> Self {
        Self { rule, ..self }
    }

    /// Turns edge feathering on or off.
    pub fn antialias(self, antialias: bool) -> Self {
        Self { antialias, ..self }
    }
}

/// How to outline a path.
#[derive(Clone, Debug)]
pub struct Stroke {
    /// The colours.
    pub paint: Paint,
    /// The line width in drawing units.
    pub width: f32,
    /// The ends of open subpaths.
    pub cap: Cap,
    /// The corners.
    pub join: Join,
    /// For [`Join::Miter`], the longest miter as a multiple of half the
    /// width; sharper corners are bevelled. The default is 4.
    pub miter_limit: f32,
    /// A dash pattern, or `None` for a solid line.
    pub dash: Option<Dash>,
    /// Whether the edge is feathered by about one pixel. On by default.
    pub antialias: bool,
}

impl Stroke {
    /// A solid stroke of `width` with `paint`, butt caps, miter joins and
    /// anti-aliasing on.
    pub fn new(paint: impl Into<Paint>, width: f32) -> Self {
        Self {
            paint: paint.into(),
            width,
            cap: Cap::Butt,
            join: Join::Miter,
            miter_limit: 4.0,
            dash: None,
            antialias: true,
        }
    }

    /// Sets the cap.
    pub fn cap(self, cap: Cap) -> Self {
        Self { cap, ..self }
    }

    /// Sets the join.
    pub fn join(self, join: Join) -> Self {
        Self { join, ..self }
    }

    /// Sets the miter limit.
    pub fn miter_limit(self, miter_limit: f32) -> Self {
        Self {
            miter_limit,
            ..self
        }
    }

    /// Sets the dash pattern.
    pub fn dash(self, dash: Dash) -> Self {
        Self {
            dash: Some(dash),
            ..self
        }
    }

    /// Turns edge feathering on or off.
    pub fn antialias(self, antialias: bool) -> Self {
        Self { antialias, ..self }
    }
}
