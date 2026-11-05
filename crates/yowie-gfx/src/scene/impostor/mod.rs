//! Impostors: a model baked into an atlas of views around it, drawn far
//! away as one camera-facing quad that shows the view nearest the
//! direction it is seen from.
//!
//! The bake stores the model's base colour and its normals, not its lit
//! colour, so an impostor is lit by the scene's sun, sky and lights each
//! frame like the model it stands for, and turns its shading as they
//! move. [`Scene::draw_model_or_impostor`] draws the model near and the
//! impostor far, cross-fading the two over a band of distance with
//! complementary dither patterns.

mod bake;

use yowie_core::{Mat4, Vec2, Vec3};

use super::billboard::{AtlasGrid, FLAG_IMPOSTOR, GpuBillboard, Sheet};
use super::fade::FadeDraw;
use super::model::ModelInstance;
use super::submit::{Placement, Scene};
use crate::Graphics;
use crate::error::Result;
use crate::texture::Texture;

/// The most views one impostor holds.
pub const MAX_IMPOSTOR_VIEWS: u32 = 64;

/// How [`Impostor::bake`] renders a model.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ImpostorOptions {
    /// Directions around the model, evenly spaced about the up axis; at
    /// most [`MAX_IMPOSTOR_VIEWS`]. More views turn more smoothly and take
    /// more atlas.
    pub views: u32,
    /// Pixels across one view.
    pub resolution: u32,
    /// How far above level each view looks down on the model, in radians.
    /// Match it to the camera's usual elevation.
    pub pitch: f32,
    /// The roughness the impostor is lit with.
    pub roughness: f32,
}

impl Default for ImpostorOptions {
    fn default() -> Self {
        Self {
            views: 12,
            resolution: 128,
            pitch: 15f32.to_radians(),
            roughness: 0.85,
        }
    }
}

/// A model baked into views; see the module documentation. Cloning
/// shares the atlas.
#[derive(Clone, Debug)]
pub struct Impostor {
    albedo: Texture,
    normal: Texture,
    grid: AtlasGrid,
    /// The middle of the model's bounds, in model space.
    center: Vec3,
    /// The side of the quad in model units.
    size: f32,
    roughness: f32,
    /// The camera distance at which
    /// [`Scene::draw_model_or_impostor`] switches from the model to the
    /// impostor, in world units.
    pub distance: f32,
    /// The width of the band of distance, centred on `distance`, over
    /// which the two cross-fade; zero switches at once.
    pub fade: f32,
}

impl Impostor {
    /// Renders `instance` (its parts, pose and material overrides) from
    /// each view into an albedo atlas and a normal atlas, with mipmaps.
    /// Runs and submits its own GPU work at once, so it may be called at
    /// any time, usually while loading. Fails when the model is empty or
    /// the atlas would be larger than the device allows.
    pub fn bake(
        gfx: &mut Graphics,
        instance: &ModelInstance,
        options: ImpostorOptions,
    ) -> Result<Impostor> {
        bake::bake(gfx, instance, &options)
    }

    /// The baked base colours: one view per cell, row by row.
    pub fn albedo(&self) -> &Texture {
        &self.albedo
    }

    /// The baked normals, each in its view's frame (X right, Y up, Z
    /// towards the viewer), encoded as `n * 0.5 + 0.5`.
    pub fn normals(&self) -> &Texture {
        &self.normal
    }

    /// The number of views.
    pub fn views(&self) -> u32 {
        self.grid.views
    }

    /// The side of the quad in world units at a scale of one.
    pub fn size(&self) -> f32 {
        self.size
    }

    fn sheet(&self) -> Sheet {
        Sheet {
            albedo: Some(self.albedo.clone()),
            normal: Some(self.normal.clone()),
            cutoff: 0.5,
            blend: false,
            lit: true,
            roughness: self.roughness,
            shadows: true,
            grid: Some(self.grid),
        }
    }
}

/// The position, uniform scale and yaw of a placement matrix.
fn decompose(m: &Mat4) -> (Vec3, f32, f32) {
    let x = m.x_axis.truncate();
    let scale = x.length().max(m.y_axis.truncate().length());
    let yaw = (-x.z).atan2(x.x);
    (m.w_axis.truncate(), scale, yaw)
}

impl Scene<'_> {
    /// Draws an impostor placed by `placement` (its position, largest
    /// scale and turn about the up axis are used).
    pub fn draw_impostor(&mut self, impostor: &Impostor, placement: impl Placement) {
        self.push_impostor(impostor, &placement.model_matrix(), 1.0);
    }

    fn push_impostor(&mut self, imp: &Impostor, m: &Mat4, shown: f32) {
        let (_, scale, yaw) = decompose(m);
        let center = m.transform_point3(imp.center);
        let size = imp.size * scale;
        let rec = &mut self.rec.large;
        let sheet = rec.billboards.sheet_slot(imp.sheet());
        rec.billboards.instances.push((
            sheet,
            GpuBillboard {
                position: center.to_array(),
                flags: FLAG_IMPOSTOR,
                size_pivot: [size, size, 0.5, 0.5],
                uv: [0.0, 0.0, 1.0, 1.0],
                color: [1.0; 4],
                fade_yaw: [shown, yaw],
                view: [0, 0],
            },
        ));
        rec.impostors += 1;
    }

    /// Draws a posed model near the camera and its impostor far from it,
    /// switching at the impostor's `distance` from the camera to the
    /// model's centre. Within the impostor's `fade` band both draw, the
    /// model's pixels dithering out as the impostor's dither in. Skinned
    /// parts do not dither; they draw whole until the band's far edge.
    pub fn draw_model_or_impostor(
        &mut self,
        instance: &ModelInstance,
        impostor: &Impostor,
        placement: impl Placement,
    ) {
        let m = placement.model_matrix();
        let center = m.transform_point3(impostor.center);
        let d = (center - self.rec.camera.position).length();
        let half = impostor.fade.max(0.0) * 0.5;
        let (near, far) = (impostor.distance - half, impostor.distance + half);
        if d < near || (half == 0.0 && d < impostor.distance) {
            self.draw_model(instance, m);
            return;
        }
        if d >= far {
            self.push_impostor(impostor, &m, 1.0);
            return;
        }
        let t = ((d - near) / (far - near)).clamp(0.0, 1.0);
        self.push_impostor(impostor, &m, t);
        let model = instance.model();
        for (i, part) in model.parts().iter().enumerate() {
            let material = instance.overrides[i]
                .as_ref()
                .unwrap_or(&model.materials()[part.material]);
            let world = m * instance.world[part.node];
            if part.mesh.is_skinned() {
                self.draw(&part.mesh, material, world);
                continue;
            }
            let mesh = self.rec.mesh_slot(&part.mesh);
            let material = self.rec.material_slot(material);
            self.rec.large.fades.push(FadeDraw {
                mesh,
                material,
                model: world,
                hidden: t,
            });
        }
    }
}

/// The size in world units a quad must have to hold a sphere of `radius`
/// with a margin that keeps filtering inside each view's cell.
pub(crate) fn cell_size(radius: f32) -> f32 {
    radius * 2.0 * 1.08
}

/// Where view `k` of a `cols`-wide grid sits, as its cell's centre in a
/// plane whose top-left corner is the origin and whose rows go down.
pub(crate) fn cell_center(k: u32, cols: u32, rows: u32, cell: f32) -> Vec2 {
    let (col, row) = (k % cols, k / cols);
    Vec2::new(
        (col as f32 + 0.5) * cell,
        (rows - row) as f32 * cell - cell * 0.5,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placement_decomposes() {
        let m = Mat4::from_scale_rotation_translation(
            Vec3::splat(2.0),
            yowie_core::Quat::from_rotation_y(0.7),
            Vec3::new(1.0, 2.0, 3.0),
        );
        let (p, s, yaw) = decompose(&m);
        assert!((p - Vec3::new(1.0, 2.0, 3.0)).length() < 1e-5);
        assert!((s - 2.0).abs() < 1e-5);
        assert!((yaw - 0.7).abs() < 1e-5);
        let c = cell_center(4, 3, 2, 2.0);
        assert_eq!(c, Vec2::new(3.0, 1.0));
    }
}
