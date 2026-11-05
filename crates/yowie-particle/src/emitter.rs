//! The emitter definition: plain data, loadable from JSON.

use serde::{Deserialize, Serialize};
use yowie_core::{Color, Vec2};
use yowie_gfx::BlendMode;

use crate::curve::{ColorKey, Curve, Range};
use crate::error::{ParticleError, Result};

/// Where particles are born, relative to the system's position.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Shape {
    /// The system's position.
    #[default]
    Point,
    /// A disc, or only its rim with `edge`.
    Circle {
        /// Radius in world units.
        radius: f32,
        /// Born on the rim instead of anywhere in the disc.
        #[serde(default)]
        edge: bool,
    },
    /// A box centred on the position, or only its border with `edge`.
    Rect {
        /// Width.
        w: f32,
        /// Height.
        h: f32,
        /// Born on the border instead of anywhere inside.
        #[serde(default)]
        edge: bool,
    },
    /// The segment from the position to `position + to`.
    Line {
        /// The far end relative to the position.
        to: Vec2,
    },
    /// A cone opening along the emitter's `direction`. Particles are born
    /// anywhere in a disc of `base_radius` (the apex when 0) and fly in a
    /// direction within `angle` radians, centred on `direction`. The
    /// emitter's `spread` is ignored.
    Cone {
        /// The full opening angle in radians.
        angle: f32,
        /// Radius of the disc particles are born in.
        #[serde(default)]
        base_radius: f32,
    },
}

/// How a system's particles blend with what is behind them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Blend {
    /// Ordinary transparency.
    #[default]
    Alpha,
    /// Adds light; fire, sparks and glows.
    Additive,
    /// Brightens without clipping as fast as adding.
    Screen,
    /// Darkens; shadows and soot.
    Multiply,
    /// The tint is taken as premultiplied by alpha.
    PremultipliedAlpha,
}

impl From<Blend> for BlendMode {
    fn from(b: Blend) -> Self {
        match b {
            Blend::Alpha => BlendMode::Alpha,
            Blend::Additive => BlendMode::Additive,
            Blend::Screen => BlendMode::Screen,
            Blend::Multiply => BlendMode::Multiply,
            Blend::PremultipliedAlpha => BlendMode::PremultipliedAlpha,
        }
    }
}

/// A point that pulls particles towards it, or pushes them away when
/// `strength` is negative. The acceleration is `strength` at the point and
/// falls off linearly to nothing at `radius`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Attractor {
    /// Where it is: world units for a world-space emitter, otherwise
    /// relative to the system's position.
    pub position: Vec2,
    /// Acceleration at the centre, in units per second squared.
    pub strength: f32,
    /// Distance beyond which it has no effect.
    pub radius: f32,
}

/// A burst of particles at a time after the system starts.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Burst {
    /// Seconds after the system starts.
    pub time: f32,
    /// Particles born.
    pub count: u32,
}

/// Everything that describes a particle effect.
///
/// An emitter is plain data: build it in code, from a preset in
/// [`presets`](crate::presets), or from JSON with
/// [`Emitter::from_json`]. Every field has a default, so a JSON file
/// only names what it changes. Run it with [`System`](crate::System).
///
/// Units are world units, seconds and radians. Angles count clockwise on
/// screen with 0 pointing right, so straight up is `-PI / 2`. Colours are
/// sRGB with straight alpha.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Emitter {
    /// Continuous births per second; 0 for none.
    pub rate: f32,
    /// Particles born the moment the system starts.
    pub burst: u32,
    /// More bursts at later times.
    pub bursts: Vec<Burst>,
    /// Emission ends after this many seconds; `None` emits until stopped.
    pub duration: Option<f32>,
    /// Seconds a particle lives.
    pub lifetime: Range,
    /// Where particles are born.
    pub shape: Shape,
    /// The direction of flight in radians.
    pub direction: f32,
    /// Random deviation in radians, centred on `direction`.
    pub spread: f32,
    /// Initial speed in units per second.
    pub speed: Range,
    /// Multiplies the velocity over the particle's life.
    pub speed_over_life: Curve,
    /// Constant acceleration, such as gravity.
    pub gravity: Vec2,
    /// Fraction of velocity lost per second.
    pub drag: f32,
    /// Acceleration away from the birth point (negative towards it).
    pub radial_accel: f32,
    /// Acceleration around the birth point.
    pub tangential_accel: f32,
    /// Points that pull or push particles.
    pub attractors: Vec<Attractor>,
    /// Width at birth; 0 uses the texture's size.
    pub size: Range,
    /// Multiplies the size over the particle's life.
    pub size_over_life: Curve,
    /// Height divided by width; 0 uses the texture's proportions.
    pub aspect: f32,
    /// Initial rotation in radians.
    pub rotation: Range,
    /// Rotation speed in radians per second.
    pub spin: Range,
    /// The colour at birth, used when `color_over_life` is empty.
    pub color: Color,
    /// The colour at death; `None` keeps `color`.
    pub color_end: Option<Color>,
    /// A colour gradient over the particle's life; replaces `color` and
    /// `color_end` when present.
    pub color_over_life: Vec<ColorKey>,
    /// Multiplies alpha over the particle's life.
    pub alpha_over_life: Curve,
    /// Each particle takes one of these colours at birth, multiplied into
    /// its colour. Empty for white.
    pub palette: Vec<Color>,
    /// The name of the texture a game should supply, resolved by the game.
    pub texture: Option<String>,
    /// Sprite sheet frames to cycle through, as sheet indices; empty uses
    /// every frame of the sheet in order.
    pub frames: Vec<u32>,
    /// Maps age to the position in the frame list; empty plays the list
    /// once over the particle's life.
    pub frame_over_life: Curve,
    /// How particles blend.
    pub blend: Blend,
    /// The renderer layer particles draw on.
    pub layer: i32,
    /// Whether particles stay where they were born when the system moves
    /// (smoke, sparks) instead of moving with it (a flame on a torch).
    pub world_space: bool,
    /// Most live particles; births stop at the limit.
    pub max: usize,
    /// Seed of the random stream.
    pub seed: u64,
    /// Seconds simulated before the first frame, so a fire starts burning.
    pub prewarm: f32,
}

impl Default for Emitter {
    fn default() -> Self {
        Self {
            rate: 0.0,
            burst: 0,
            bursts: Vec::new(),
            duration: None,
            lifetime: Range::constant(1.0),
            shape: Shape::Point,
            direction: 0.0,
            spread: 0.0,
            speed: Range::default(),
            speed_over_life: Curve::default(),
            gravity: Vec2::ZERO,
            drag: 0.0,
            radial_accel: 0.0,
            tangential_accel: 0.0,
            attractors: Vec::new(),
            size: Range::default(),
            size_over_life: Curve::default(),
            aspect: 0.0,
            rotation: Range::default(),
            spin: Range::default(),
            color: Color::WHITE,
            color_end: None,
            color_over_life: Vec::new(),
            alpha_over_life: Curve::default(),
            palette: Vec::new(),
            texture: None,
            frames: Vec::new(),
            frame_over_life: Curve::default(),
            blend: Blend::Alpha,
            layer: 0,
            world_space: false,
            max: 1000,
            seed: 0,
            prewarm: 0.0,
        }
    }
}

impl Emitter {
    /// Reads an emitter from JSON. Fields left out take their defaults.
    pub fn from_json(json: &str) -> Result<Self> {
        serde_json::from_str(json).map_err(ParticleError::Json)
    }

    /// Writes the emitter as indented JSON.
    pub fn to_json(&self) -> Result<String> {
        serde_json::to_string_pretty(self).map_err(ParticleError::Json)
    }

    /// The colour at normalised age `t`, before the palette tint.
    pub(crate) fn color_at(&self, t: f32) -> Color {
        if let Some(c) = crate::curve::gradient_at(&self.color_over_life, t) {
            return c;
        }
        match self.color_end {
            Some(end) => self.color.lerp(end, t),
            None => self.color,
        }
    }
}
