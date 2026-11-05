//! The submission API: [`Graphics::scene`] and the [`Scene`] a game draws
//! into.

use yowie_core::{Affine3A, Color, Mat4, Transform, Vec2, Vec3};

use super::bounds::Ray;
use super::camera::Camera3D;
use super::environment::Environment;
use super::light::{DirectionalLight, LocalLight, PointLight, SpotLight};
use super::material::Material;
use super::mesh::{Mesh, RayHit};
use super::morph::pick_weights;
use super::post::PostSettings;
use super::record::{DrawRecord, SceneRecord};
use super::sky::{Fog, Sky, Tonemap};
use crate::Graphics;

/// Something that places a mesh in the world: a matrix, a
/// [`Transform`], an affine transform, or a position.
pub trait Placement {
    /// The model matrix.
    fn model_matrix(&self) -> Mat4;
}

impl Placement for Mat4 {
    fn model_matrix(&self) -> Mat4 {
        *self
    }
}

impl Placement for Transform {
    fn model_matrix(&self) -> Mat4 {
        self.to_matrix()
    }
}

impl Placement for Affine3A {
    fn model_matrix(&self) -> Mat4 {
        Mat4::from(*self)
    }
}

impl Placement for Vec3 {
    fn model_matrix(&self) -> Mat4 {
        Mat4::from_translation(*self)
    }
}

impl<T: Placement> Placement for &T {
    fn model_matrix(&self) -> Mat4 {
        (*self).model_matrix()
    }
}

/// Names one draw of a scene, in submission order; returned by the draw
/// calls and by [`Scene::pick`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DrawId(pub u32);

/// What [`Scene::pick`] hit.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScenePick {
    /// The draw that was hit.
    pub draw: DrawId,
    /// Where it was hit.
    pub hit: RayHit,
}

/// A 3D scene being recorded; see [`Graphics::scene`].
///
/// Every call queues work; nothing is rendered until the frame ends.
/// Settings (`set_*`) apply to the whole scene whatever the order of the
/// calls.
pub struct Scene<'a> {
    pub(crate) rec: &'a mut SceneRecord,
}

impl std::fmt::Debug for Scene<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Scene")
            .field("draws", &self.rec.draws.len())
            .field("lights", &self.rec.lights.len())
            .finish_non_exhaustive()
    }
}

impl Graphics {
    /// Records a 3D scene seen through `camera` into the current output
    /// (the screen, or the render texture of an enclosing
    /// [`draw_to`](Graphics::draw_to)).
    ///
    /// The scene renders into its own HDR image with depth when the frame
    /// ends, is tone mapped into the camera's viewport, and 2D drawing
    /// goes over it. Several scenes may share an output (split screens,
    /// picture in picture); they render in call order.
    ///
    /// ```no_run
    /// # use yowie_gfx::{Graphics, scene::*};
    /// # use yowie_core::{Color, Vec3};
    /// # fn f(gfx: &mut Graphics, cube: &Mesh) {
    /// let camera = Camera3D::perspective(Vec3::new(0.0, 2.0, 6.0), Vec3::ZERO, 1.0);
    /// gfx.scene(&camera, |s| {
    ///     s.set_sun(DirectionalLight::default().with_shadows(30.0));
    ///     s.add_point_light(PointLight::new(Vec3::Y * 2.0, Color::ORANGE, 4.0, 8.0));
    ///     s.draw(cube, &Material::color(Color::RED, 0.0, 0.4), Vec3::ZERO);
    /// });
    /// # }
    /// ```
    pub fn scene<R>(&mut self, camera: &Camera3D, f: impl FnOnce(&mut Scene<'_>) -> R) -> R {
        let list = self.current_index();
        let view_size = self.current_list().mapping.view_size;
        let renderer = self.scene_renderer();
        let mut rec = renderer.take_record();
        rec.reset(list, *camera, view_size);
        let r = f(&mut Scene { rec: &mut rec });
        renderer.push_record(rec);
        r
    }
}

impl Scene<'_> {
    /// The camera the scene is seen through.
    pub fn camera(&self) -> &Camera3D {
        &self.rec.camera
    }

    /// The size of the output's view in view units.
    pub fn view_size(&self) -> Vec2 {
        self.rec.view_size
    }

    /// Sets the directional light; `None` (the default) has none.
    pub fn set_sun(&mut self, sun: impl Into<Option<DirectionalLight>>) {
        self.rec.sun = sun.into();
    }

    /// Sets the light that reaches everything evenly when there is no sky
    /// or environment (sRGB). The default is a dim blue-grey.
    pub fn set_ambient(&mut self, color: Color) {
        self.rec.ambient = color;
    }

    /// Sets the procedural sky, which lights the scene and draws behind
    /// it. An environment set with
    /// [`set_environment`](Self::set_environment) takes precedence for
    /// lighting and background.
    pub fn set_sky(&mut self, sky: impl Into<Option<Sky>>) {
        self.rec.sky = sky.into();
    }

    /// Lights the scene with an image environment (prefiltered specular
    /// reflections and diffuse irradiance) and draws it behind the scene.
    pub fn set_environment(&mut self, environment: Option<&Environment>) {
        self.rec.environment = environment.cloned();
    }

    /// Whether the sky or environment is drawn behind the scene (the
    /// default). When off, the clear colour shows behind it and the sky
    /// or environment only lights it.
    pub fn set_background(&mut self, on: bool) {
        self.rec.background = on;
    }

    /// Sets the fog; `None` (the default) has none.
    pub fn set_fog(&mut self, fog: impl Into<Option<Fog>>) {
        self.rec.fog = fog.into();
    }

    /// Multiplies the scene's light before tone mapping; 1 by default.
    /// The same as setting [`PostSettings::exposure`].
    pub fn set_exposure(&mut self, exposure: f32) {
        self.rec.post.exposure = exposure;
    }

    /// Sets the tone curve; ACES by default. The same as setting
    /// [`PostSettings::tonemap`].
    pub fn set_tonemap(&mut self, tonemap: Tonemap) {
        self.rec.post.tonemap = tonemap;
    }

    /// Sets how the scene's HDR image is finished on its way to the
    /// output: exposure, tone curve, grade and effects. The default turns
    /// every effect off.
    pub fn set_post(&mut self, post: PostSettings) {
        self.rec.post = post;
    }

    /// The scene's post-processing settings, to change one field.
    pub fn post_mut(&mut self) -> &mut PostSettings {
        &mut self.rec.post
    }

    /// Tells temporal effects that the camera jumped this frame (a cut to
    /// another shot, a teleport), so the frames before it are not
    /// blended into this one.
    pub fn cut(&mut self) {
        self.rec.cut = true;
    }

    /// Queues custom GPU work in the scene's HDR pass, after the blended
    /// meshes and before the debug lines; see [`GpuDraw`](crate::GpuDraw).
    /// GPU particles draw this way.
    pub fn draw_gpu(&mut self, draw: std::sync::Arc<dyn crate::GpuDraw>) {
        self.rec.customs.push(crate::custom::CustomEntry(draw));
    }

    /// Adds a point light. A scene keeps up to
    /// [`MAX_LIGHTS`](super::MAX_LIGHTS) point and spot lights, the first
    /// added first; shadows go to the first
    /// [`MAX_POINT_SHADOWS`](super::MAX_POINT_SHADOWS) point lights that
    /// ask.
    pub fn add_point_light(&mut self, light: PointLight) {
        self.rec.lights.push(LocalLight::Point(light));
    }

    /// Adds a spot light; shadows go to the first
    /// [`MAX_SPOT_SHADOWS`](super::MAX_SPOT_SHADOWS) spot lights that ask.
    pub fn add_spot_light(&mut self, light: SpotLight) {
        self.rec.lights.push(LocalLight::Spot(light));
    }

    /// Draws `mesh` with `material` placed by `placement`. Draws of the
    /// same mesh with an equal material become one instanced draw call.
    pub fn draw(&mut self, mesh: &Mesh, material: &Material, placement: impl Placement) -> DrawId {
        self.push(mesh, material, placement.model_matrix(), u32::MAX, &[])
    }

    /// Draws `mesh` like [`draw`](Self::draw) and says where it was placed
    /// last frame, so temporal anti-aliasing and motion blur follow its
    /// own motion as well as the camera's. Skinned and morphed meshes move
    /// by their placement alone.
    pub fn draw_moved(
        &mut self,
        mesh: &Mesh,
        material: &Material,
        placement: impl Placement,
        previous: impl Placement,
    ) -> DrawId {
        let id = self.draw(mesh, material, placement);
        self.rec.moved.push((id.0, previous.model_matrix()));
        id
    }

    /// Draws `mesh` once per matrix, as one instanced draw call.
    /// Returns the first draw's id; the others follow it.
    pub fn draw_instanced(&mut self, mesh: &Mesh, material: &Material, models: &[Mat4]) -> DrawId {
        let first = DrawId(self.rec.draws.len() as u32);
        if models.is_empty() {
            return first;
        }
        let m = self.rec.mesh_slot(mesh);
        let mat = self.rec.material_slot(material);
        for model in models {
            self.rec.push_draw(DrawRecord {
                mesh: m,
                material: mat,
                model: *model,
                joints: u32::MAX,
                morph_first: 0,
                morph_count: 0,
            });
        }
        first
    }

    /// Draws a skinned mesh posed by `joints`: one matrix per joint, each
    /// the joint's model-space matrix times its inverse bind matrix (what
    /// `yowie_anim::Skeleton::joint_matrices` returns).
    pub fn draw_skinned(
        &mut self,
        mesh: &Mesh,
        material: &Material,
        placement: impl Placement,
        joints: &[Mat4],
    ) -> DrawId {
        let first = self.rec.joints.len() as u32;
        self.rec.joints.extend_from_slice(joints);
        self.push(mesh, material, placement.model_matrix(), first, &[])
    }

    /// Draws a mesh with morph targets blended by `weights`, one per
    /// target.
    pub fn draw_morphed(
        &mut self,
        mesh: &Mesh,
        material: &Material,
        placement: impl Placement,
        weights: &[f32],
    ) -> DrawId {
        self.push(mesh, material, placement.model_matrix(), u32::MAX, weights)
    }

    pub(crate) fn push(
        &mut self,
        mesh: &Mesh,
        material: &Material,
        model: Mat4,
        joints: u32,
        weights: &[f32],
    ) -> DrawId {
        let m = self.rec.mesh_slot(mesh);
        let mat = self.rec.material_slot(material);
        let morph_first = self.rec.morph_weights.len() as u32;
        let morph_count = if weights.is_empty() {
            0
        } else {
            pick_weights(
                weights,
                mesh.morph_target_count(),
                &mut self.rec.morph_weights,
            )
        };
        DrawId(self.rec.push_draw(DrawRecord {
            mesh: m,
            material: mat,
            model,
            joints,
            morph_first,
            morph_count,
        }))
    }

    /// Draws a line from `a` to `b` in world space, hidden by what stands
    /// in front of it. Physics debug drawing and gizmos use it.
    pub fn line(&mut self, a: Vec3, b: Vec3, color: Color) {
        self.rec.push_line(a.to_array(), b.to_array(), color, false);
    }

    /// Draws a line over everything in the scene.
    pub fn line_on_top(&mut self, a: Vec3, b: Vec3, color: Color) {
        self.rec.push_line(a.to_array(), b.to_array(), color, true);
    }

    /// The nearest draw recorded so far that `ray` hits, testing each
    /// draw's bounds and then its triangles. Skinned and morphed meshes
    /// are tested in their rest shape.
    pub fn pick(&self, ray: &Ray) -> Option<ScenePick> {
        let mut best: Option<ScenePick> = None;
        for (i, d) in self.rec.draws.iter().enumerate() {
            let mesh = &self.rec.meshes[d.mesh as usize];
            if let Some(hit) = mesh.intersect(&d.model, ray)
                && best.is_none_or(|b| hit.distance < b.hit.distance)
            {
                best = Some(ScenePick {
                    draw: DrawId(i as u32),
                    hit,
                });
            }
        }
        best
    }

    /// The world ray through a point of the view (in view units, such as
    /// the mouse position), through this scene's camera.
    pub fn screen_ray(&self, screen: Vec2) -> Ray {
        self.rec.camera.screen_ray(screen, self.rec.view_size)
    }
}

/// The nearest of `items` that `ray` hits, for picking outside a frame:
/// each item is a key, a mesh and its model matrix.
pub fn pick<'m, K>(
    ray: &Ray,
    items: impl IntoIterator<Item = (K, &'m Mesh, Mat4)>,
) -> Option<(K, RayHit)> {
    let mut best: Option<(K, RayHit)> = None;
    for (key, mesh, model) in items {
        if let Some(hit) = mesh.intersect(&model, ray)
            && best.as_ref().is_none_or(|(_, b)| hit.distance < b.distance)
        {
            best = Some((key, hit));
        }
    }
    best
}
