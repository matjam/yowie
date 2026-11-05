//! Propagation of local transforms down the hierarchy.

use hecs::Entity;
use yowie_core::{Affine2, Mat4, Transform, Transform2D};

use crate::hierarchy::{Children, Parent};
use crate::world::World;

/// World-space matrix of an entity with a [`Transform`], written by
/// [`propagate_transforms`]. Never save or edit it by hand.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GlobalTransform(pub Mat4);

/// World-space affine map of an entity with a [`Transform2D`], written by
/// [`propagate_transforms`]. Never save or edit it by hand.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GlobalTransform2D(pub Affine2);

/// One transform space: a local component and the global one derived from it.
trait Space {
    type Local: hecs::Component + Copy;
    type Global: hecs::Component + Copy;
    const IDENTITY: Self::Global;
    fn local(local: &Self::Local) -> Self::Global;
    fn compose(parent: Self::Global, local: Self::Global) -> Self::Global;
}

struct Space3;
struct Space2;

impl Space for Space3 {
    type Local = Transform;
    type Global = GlobalTransform;
    const IDENTITY: GlobalTransform = GlobalTransform(Mat4::IDENTITY);
    fn local(t: &Transform) -> GlobalTransform {
        GlobalTransform(t.to_matrix())
    }
    fn compose(p: GlobalTransform, l: GlobalTransform) -> GlobalTransform {
        GlobalTransform(p.0 * l.0)
    }
}

impl Space for Space2 {
    type Local = Transform2D;
    type Global = GlobalTransform2D;
    const IDENTITY: GlobalTransform2D = GlobalTransform2D(Affine2::IDENTITY);
    fn local(t: &Transform2D) -> GlobalTransform2D {
        GlobalTransform2D(t.to_affine())
    }
    fn compose(p: GlobalTransform2D, l: GlobalTransform2D) -> GlobalTransform2D {
        GlobalTransform2D(p.0 * l.0)
    }
}

/// Recomputes [`GlobalTransform`] from [`Transform`] and
/// [`GlobalTransform2D`] from [`Transform2D`] for every entity.
///
/// A root's global transform equals its local one. A child's is its
/// parent's global transform times its own local one. An entity in the
/// middle of a chain that has no local transform of the space passes its
/// parent's global transform through unchanged and gets no global
/// component. 2D and 3D chains are independent. Call it once per update
/// after gameplay systems move things, usually in the post-update stage.
pub fn propagate_transforms(world: &mut World) {
    propagate::<Space3>(world);
    propagate::<Space2>(world);
}

fn propagate<S: Space>(world: &mut World) {
    let roots: Vec<Entity> = world
        .inner
        .iter()
        .filter(|e| !e.has::<Parent>() && (e.has::<S::Local>() || e.has::<Children>()))
        .map(|e| e.entity())
        .collect();
    let mut stack: Vec<(Entity, S::Global)> = roots.into_iter().map(|e| (e, S::IDENTITY)).collect();
    while let Some((entity, parent_global)) = stack.pop() {
        let local = world
            .inner
            .get::<&S::Local>(entity)
            .ok()
            .map(|l| S::local(&l));
        let global = match local {
            Some(l) => {
                let g = S::compose(parent_global, l);
                store::<S>(world, entity, g);
                g
            }
            None => parent_global,
        };
        for child in world.children(entity) {
            stack.push((child, global));
        }
    }
}

fn store<S: Space>(world: &mut World, entity: Entity, global: S::Global) {
    let updated = match world.inner.get::<&mut S::Global>(entity) {
        Ok(mut g) => {
            *g = global;
            true
        }
        Err(_) => false,
    };
    if !updated {
        let _ = world.inner.insert_one(entity, global);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use yowie_core::{Quat, Vec2, Vec3, vec2, vec3};

    #[test]
    fn child_follows_rotated_parent_3d() {
        let mut w = World::new();
        let parent = w.spawn((Transform::from_translation(vec3(10.0, 0.0, 0.0))
            .with_rotation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_2)),));
        let child = w.spawn((Transform::from_translation(vec3(1.0, 0.0, 0.0)),));
        w.set_parent(child, parent).unwrap();
        propagate_transforms(&mut w);
        let g = w.get::<&GlobalTransform>(child).unwrap().0;
        let p = g.transform_point3(Vec3::ZERO);
        assert!((p - vec3(10.0, 1.0, 0.0)).length() < 1e-5, "{p:?}");
        assert!(w.get::<&GlobalTransform>(parent).is_ok());
    }

    #[test]
    fn two_d_chain_skips_entity_without_transform() {
        let mut w = World::new();
        let root = w.spawn((Transform2D::from_position(vec2(5.0, 0.0)),));
        let mid = w.spawn(());
        let leaf = w.spawn((Transform2D::from_position(vec2(0.0, 2.0)),));
        w.set_parent(mid, root).unwrap();
        w.set_parent(leaf, mid).unwrap();
        propagate_transforms(&mut w);
        let g = w.get::<&GlobalTransform2D>(leaf).unwrap().0;
        assert!((g.transform_point2(Vec2::ZERO) - vec2(5.0, 2.0)).length() < 1e-5);
        assert!(w.get::<&GlobalTransform2D>(mid).is_err());
    }

    #[test]
    fn recomputes_after_move_and_reparent() {
        let mut w = World::new();
        let a = w.spawn((Transform2D::from_position(vec2(1.0, 0.0)),));
        let b = w.spawn((Transform2D::from_position(vec2(0.0, 1.0)),));
        propagate_transforms(&mut w);
        w.set_parent(b, a).unwrap();
        w.get::<&mut Transform2D>(a).unwrap().position = vec2(3.0, 0.0);
        propagate_transforms(&mut w);
        let g = w.get::<&GlobalTransform2D>(b).unwrap().0;
        assert!((g.translation - vec2(3.0, 1.0)).length() < 1e-5);
    }
}
