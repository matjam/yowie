//! The transform stack: a 2D transform applied to everything drawn after
//! it, saved and restored with `push_transform` and `pop_transform`.

use yowie_core::{Affine2, Vec2};

use crate::Graphics;

impl Graphics {
    /// The transform applied to later 2D drawing. It maps drawing
    /// coordinates (before any camera) to where they land; the identity
    /// when none is set.
    pub fn transform(&self) -> Affine2 {
        self.cursor.transform
    }

    /// Replaces the current transform. The camera and clip rectangle are
    /// not affected: a clip stays in view units.
    pub fn set_transform(&mut self, transform: Affine2) {
        self.cursor.transform = transform;
    }

    /// Resets the transform to the identity without touching saved ones.
    pub fn reset_transform(&mut self) {
        self.cursor.transform = Affine2::IDENTITY;
    }

    /// Saves the current transform. Pair it with
    /// [`pop_transform`](Self::pop_transform). The stack is cleared at the
    /// start of each frame.
    pub fn push_transform(&mut self) {
        let t = self.cursor.transform;
        self.cursor.transform_stack.push(t);
    }

    /// Restores the transform saved by the last
    /// [`push_transform`](Self::push_transform). With nothing saved it
    /// resets to the identity.
    pub fn pop_transform(&mut self) {
        self.cursor.transform = self
            .cursor
            .transform_stack
            .pop()
            .unwrap_or(Affine2::IDENTITY);
    }

    /// Moves later drawing by `offset` in the current transform's space.
    pub fn translate(&mut self, offset: Vec2) {
        self.cursor.transform *= Affine2::from_translation(offset);
    }

    /// Rotates later drawing by `angle` radians, clockwise on screen,
    /// about the current origin.
    pub fn rotate(&mut self, angle: f32) {
        self.cursor.transform *= Affine2::from_angle(angle);
    }

    /// Scales later drawing about the current origin.
    pub fn scale(&mut self, factors: Vec2) {
        self.cursor.transform *= Affine2::from_scale(factors);
    }

    /// Shears later drawing: each point moves along X by `shear.x` times its
    /// Y and along Y by `shear.y` times its X.
    pub fn shear(&mut self, shear: Vec2) {
        let m = glam_shear(shear);
        self.cursor.transform *= m;
    }

    /// Runs `f` with `transform` applied on top of the current one, then
    /// restores the previous transform.
    pub fn with_transform<R>(&mut self, transform: Affine2, f: impl FnOnce(&mut Self) -> R) -> R {
        let old = self.cursor.transform;
        self.cursor.transform = old * transform;
        let r = f(self);
        self.cursor.transform = old;
        r
    }

    /// The uniform scale of the current transform: the square root of the
    /// area its matrix maps a unit square to.
    pub(crate) fn transform_scale(&self) -> f32 {
        self.cursor.transform.matrix2.determinant().abs().sqrt()
    }
}

fn glam_shear(shear: Vec2) -> Affine2 {
    Affine2::from_mat2(yowie_core::Mat2::from_cols(
        Vec2::new(1.0, shear.y),
        Vec2::new(shear.x, 1.0),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use yowie_core::vec2;

    #[test]
    fn stack_nests_and_restores() {
        let mut t = Affine2::IDENTITY;
        let mut stack: Vec<Affine2> = Vec::new();
        // The same arithmetic the methods use, checked without a GPU.
        stack.push(t);
        t *= Affine2::from_translation(vec2(10.0, 0.0));
        stack.push(t);
        t *= Affine2::from_scale(vec2(2.0, 2.0));
        let p = t.transform_point2(vec2(1.0, 1.0));
        assert!((p - vec2(12.0, 2.0)).length() < 1e-5, "{p:?}");
        t = stack.pop().unwrap_or(Affine2::IDENTITY);
        assert!((t.transform_point2(vec2(1.0, 1.0)) - vec2(11.0, 1.0)).length() < 1e-5);
        let sh = glam_shear(vec2(1.0, 0.0));
        assert!((sh.transform_point2(vec2(0.0, 2.0)) - vec2(2.0, 2.0)).length() < 1e-5);
    }
}
