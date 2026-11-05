//! How 2D drawing combines with what is already in the target.
//!
//! The renderer works in linear light with premultiplied alpha: textures
//! are premultiplied when uploaded, tints are premultiplied on the
//! processor, and fragment programs return premultiplied colour. Each mode
//! below states its result in those terms, with `S` the source colour,
//! `Sa` its alpha and `D` the destination.

use wgpu::{BlendComponent, BlendFactor as F, BlendOperation as Op, BlendState};

/// A blend mode for 2D drawing. Set one for a stretch of drawing with
/// [`Graphics::with_blend`](crate::Graphics::with_blend).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum BlendMode {
    /// Ordinary transparency: `S + D * (1 - Sa)`. The tint's colour is
    /// multiplied by its alpha before drawing.
    #[default]
    Alpha,
    /// The same equation as `Alpha`, but the tint is taken as already
    /// premultiplied: a tint with zero alpha and non-zero colour adds
    /// light, which suits glows that also darken nothing.
    PremultipliedAlpha,
    /// Adds the source to the destination: `D + S`. For light, fire and
    /// explosions.
    Additive,
    /// Multiplies the destination by the source: `D * S + D * (1 - Sa)`.
    /// For shadows and tinted glass.
    Multiply,
    /// Brightens: `S + D * (1 - S)`.
    Screen,
    /// Subtracts the source from the destination: `D - S`.
    Subtract,
    /// Writes the source, ignoring the destination.
    Replace,
}

impl BlendMode {
    /// Every mode, in declaration order.
    pub const ALL: [BlendMode; 7] = [
        BlendMode::Alpha,
        BlendMode::PremultipliedAlpha,
        BlendMode::Additive,
        BlendMode::Multiply,
        BlendMode::Screen,
        BlendMode::Subtract,
        BlendMode::Replace,
    ];

    /// Whether the engine premultiplies the tint by its alpha.
    pub(crate) fn premultiplies_tint(self) -> bool {
        self != BlendMode::PremultipliedAlpha
    }

    /// The fixed-function blend state for this mode.
    pub(crate) fn state(self) -> BlendState {
        let c = |src_factor, dst_factor, operation| BlendComponent {
            src_factor,
            dst_factor,
            operation,
        };
        let over = c(F::One, F::OneMinusSrcAlpha, Op::Add);
        let (color, alpha) = match self {
            BlendMode::Alpha | BlendMode::PremultipliedAlpha => (over, over),
            BlendMode::Additive => (c(F::One, F::One, Op::Add), c(F::Zero, F::One, Op::Add)),
            BlendMode::Multiply => (c(F::Dst, F::OneMinusSrcAlpha, Op::Add), over),
            BlendMode::Screen => (c(F::One, F::OneMinusSrc, Op::Add), over),
            BlendMode::Subtract => (
                c(F::One, F::One, Op::ReverseSubtract),
                c(F::Zero, F::One, Op::Add),
            ),
            BlendMode::Replace => (c(F::One, F::Zero, Op::Add), c(F::One, F::Zero, Op::Add)),
        };
        BlendState { color, alpha }
    }
}
