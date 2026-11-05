//! Drawing a posed model into a scene.

use yowie_core::Mat4;

use super::{ModelInstance, ModelPart};
use crate::scene::material::Material;
use crate::scene::submit::{DrawId, Placement, Scene};

impl Scene<'_> {
    /// Draws every part of a posed model placed by `placement`, with the
    /// instance's material overrides. Skinned parts take the instance's
    /// joint matrices; morphed parts its weights. Returns the first part's
    /// draw id; the others follow in part order.
    pub fn draw_model(&mut self, instance: &ModelInstance, placement: impl Placement) -> DrawId {
        self.draw_model_with(instance, placement, |_, _| None)
    }

    /// Draws a posed model like [`draw_model`](Self::draw_model), passing
    /// each part (its index and the part) to `override_material`, which
    /// returns a material to draw it with instead, or `None` to keep the
    /// instance's choice.
    pub fn draw_model_with(
        &mut self,
        instance: &ModelInstance,
        placement: impl Placement,
        mut override_material: impl FnMut(usize, &ModelPart) -> Option<Material>,
    ) -> DrawId {
        let model = placement.model_matrix();
        let data = &instance.model().inner;
        let first = DrawId(self.rec.draws.len() as u32);
        // Parts sharing a skin share one copy of its joint matrices.
        let mut skin_first: Vec<Option<u32>> = vec![None; data.skins.len()];
        for (i, part) in data.parts.iter().enumerate() {
            let replaced = override_material(i, part);
            let material = replaced
                .as_ref()
                .or(instance.overrides[i].as_ref())
                .unwrap_or(&data.materials[part.material]);
            let weights = instance
                .weights
                .get(part.node)
                .map_or(&[][..], Vec::as_slice);
            let skin = data.nodes[part.node].skin;
            match skin {
                Some(s) if part.mesh.is_skinned() => {
                    // glTF ignores a skinned node's own transform: the joint
                    // matrices are already in model space.
                    let first_joint = *skin_first[s].get_or_insert_with(|| {
                        let at = self.rec.joints.len() as u32;
                        self.rec.joints.extend_from_slice(&instance.joints[s]);
                        at
                    });
                    self.push(&part.mesh, material, model, first_joint, weights);
                }
                _ => {
                    let node: Mat4 = instance.world[part.node];
                    self.push(&part.mesh, material, model * node, u32::MAX, weights);
                }
            }
        }
        first
    }
}
