//! Stencil masks between draws: one material marks the stencil buffer
//! where it draws and another draws only where the mark is (portals,
//! cutaways, magic windows).
//!
//! The scene's stencil buffer starts each frame at zero. Games use its low
//! seven bits (references 0 to 127); the top bit is the outline mark.

/// When a material's fragments pass the stencil test: how the value
/// already in the stencil buffer compares to [`Stencil::reference`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum StencilTest {
    /// No test: the material draws everywhere (the default).
    #[default]
    Always,
    /// Only where the buffer holds the reference.
    Equal,
    /// Only where the buffer holds anything else.
    NotEqual,
    /// Only where the buffer holds less than the reference.
    Less,
    /// Only where the buffer holds more than the reference.
    Greater,
    /// Only where the buffer holds at most the reference.
    LessEqual,
    /// Only where the buffer holds at least the reference.
    GreaterEqual,
    /// Nowhere.
    Never,
}

/// What a fragment that passes the stencil and depth tests does to the
/// stencil buffer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum StencilOp {
    /// Leaves the value alone (the default).
    #[default]
    Keep,
    /// Stores the reference.
    Replace,
    /// Stores zero.
    Zero,
    /// Adds one, stopping at the largest value.
    Increment,
    /// Subtracts one, stopping at zero.
    Decrement,
    /// Inverts the bits.
    Invert,
}

/// A material's stencil test and write. The default draws everywhere and
/// leaves the buffer alone.
///
/// Materials that write the buffer draw before those that do not, so the
/// order the game draws them in does not matter. A masked draw still has
/// to pass the depth test, so it must stand in front of its mask.
///
/// ```
/// # use yowie_gfx::scene::{Material, Stencil};
/// let window = Material { stencil: Stencil::mark(1), ..Material::default() };
/// let seen_through_it = Material { stencil: Stencil::inside(1), ..Material::default() };
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Stencil {
    /// When the material's fragments draw.
    pub test: StencilTest,
    /// The value tested against and written, 0 to 127.
    pub reference: u8,
    /// What drawn fragments store.
    pub write: StencilOp,
}

impl Stencil {
    /// Marks the buffer with `reference` wherever the material draws.
    pub fn mark(reference: u8) -> Self {
        Self {
            test: StencilTest::Always,
            reference,
            write: StencilOp::Replace,
        }
    }

    /// Draws only where the buffer holds `reference`.
    pub fn inside(reference: u8) -> Self {
        Self {
            test: StencilTest::Equal,
            reference,
            write: StencilOp::Keep,
        }
    }

    /// Draws only where the buffer does not hold `reference`.
    pub fn outside(reference: u8) -> Self {
        Self {
            test: StencilTest::NotEqual,
            reference,
            write: StencilOp::Keep,
        }
    }

    /// Whether the material changes the buffer.
    pub fn writes(&self) -> bool {
        self.write != StencilOp::Keep
    }

    /// Whether the material tests or writes the buffer at all.
    pub fn is_active(&self) -> bool {
        self.test != StencilTest::Always || self.writes()
    }
}

/// The bits the game's masks use; the top bit marks outlined meshes.
pub(crate) const GAME_BITS: u32 = 0x7f;
/// The bit an outlined mesh writes and its outline tests.
pub(crate) const OUTLINE_BIT: u32 = 0x80;

/// The stencil state of a mesh pipeline, part of its cache key.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum StencilKey {
    /// No test, no write.
    #[default]
    None,
    /// A game mask: test and operation as numbers.
    Game { test: u8, op: u8 },
    /// An outlined mesh marking the outline bit.
    OutlineMark,
    /// An outline drawn where the outline bit is clear.
    OutlineTest,
}

impl StencilKey {
    pub(crate) fn of(s: &Stencil) -> Self {
        if s.is_active() {
            StencilKey::Game {
                test: s.test as u8,
                op: s.write as u8,
            }
        } else {
            StencilKey::None
        }
    }

    /// Whether pipelines with this key change the buffer, which draws them
    /// first.
    pub(crate) fn writes(&self) -> bool {
        match self {
            StencilKey::Game { op, .. } => *op != StencilOp::Keep as u8,
            StencilKey::OutlineMark => true,
            _ => false,
        }
    }

    /// The reference a pass sets for these pipelines, given the material's.
    pub(crate) fn reference(&self, material: &Stencil) -> u32 {
        match self {
            StencilKey::Game { .. } => u32::from(material.reference) & GAME_BITS,
            StencilKey::OutlineMark | StencilKey::OutlineTest => OUTLINE_BIT,
            StencilKey::None => 0,
        }
    }

    /// The pipeline's stencil state.
    pub(crate) fn state(&self) -> wgpu::StencilState {
        use wgpu::CompareFunction as C;
        use wgpu::StencilOperation as O;
        let (compare, op, mask) = match self {
            StencilKey::None => return wgpu::StencilState::default(),
            StencilKey::Game { test, op } => (test_fn(*test), op_fn(*op), GAME_BITS),
            StencilKey::OutlineMark => (C::Always, O::Replace, OUTLINE_BIT),
            StencilKey::OutlineTest => (C::NotEqual, O::Keep, OUTLINE_BIT),
        };
        let face = wgpu::StencilFaceState {
            compare,
            fail_op: O::Keep,
            depth_fail_op: O::Keep,
            pass_op: op,
        };
        wgpu::StencilState {
            front: face,
            back: face,
            read_mask: mask,
            write_mask: if op == O::Keep { 0 } else { mask },
        }
    }
}

/// The comparison for a test. wgpu compares the reference against the
/// stored value (`reference OP stored`), so a material's "stored is less
/// than the reference" is the reference being greater.
fn test_fn(test: u8) -> wgpu::CompareFunction {
    use wgpu::CompareFunction as C;
    match test {
        t if t == StencilTest::Equal as u8 => C::Equal,
        t if t == StencilTest::NotEqual as u8 => C::NotEqual,
        t if t == StencilTest::Less as u8 => C::Greater,
        t if t == StencilTest::Greater as u8 => C::Less,
        t if t == StencilTest::LessEqual as u8 => C::GreaterEqual,
        t if t == StencilTest::GreaterEqual as u8 => C::LessEqual,
        t if t == StencilTest::Never as u8 => C::Never,
        _ => C::Always,
    }
}

fn op_fn(op: u8) -> wgpu::StencilOperation {
    use wgpu::StencilOperation as O;
    match op {
        o if o == StencilOp::Replace as u8 => O::Replace,
        o if o == StencilOp::Zero as u8 => O::Zero,
        o if o == StencilOp::Increment as u8 => O::IncrementClamp,
        o if o == StencilOp::Decrement as u8 => O::DecrementClamp,
        o if o == StencilOp::Invert as u8 => O::Invert,
        _ => O::Keep,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tests_read_from_the_stored_side() {
        let cases = [
            (StencilTest::Less, wgpu::CompareFunction::Greater),
            (StencilTest::Greater, wgpu::CompareFunction::Less),
            (StencilTest::Equal, wgpu::CompareFunction::Equal),
            (StencilTest::Always, wgpu::CompareFunction::Always),
        ];
        for (test, want) in cases {
            assert_eq!(test_fn(test as u8), want, "{test:?}");
        }
    }

    #[test]
    fn writers_and_outlines_keep_to_their_bits() {
        let mark = StencilKey::of(&Stencil::mark(3));
        assert!(mark.writes());
        assert_eq!(mark.state().write_mask, GAME_BITS);
        assert_eq!(mark.reference(&Stencil::mark(200)), 200 & GAME_BITS);
        let inside = StencilKey::of(&Stencil::inside(3));
        assert!(!inside.writes());
        assert_eq!(inside.state().write_mask, 0);
        assert_eq!(StencilKey::OutlineTest.state().read_mask, OUTLINE_BIT);
        assert_eq!(StencilKey::of(&Stencil::default()), StencilKey::None);
    }
}
