//! Which passes a draw takes part in, by its material: the lit pass it
//! draws in (opaque, cut-out, sorted or order-independent blending,
//! transmission) and the extra passes its features add (fur shells,
//! outline hull, x-ray tint).

use super::material::{AlphaMode, Material};
use super::pipelines::{MeshKey, MeshPass};
use super::prepare::{Batch, Resolved, emit, instance, pack_key};
use super::record::SceneRecord;
use super::stencil::StencilKey;
use super::transparency::Transparency;
use super::vertex::GpuInstance;

/// What a draw's material asks of the passes.
#[derive(Clone, Copy, Debug)]
pub(crate) struct DrawClass {
    pub(crate) pass: MeshPass,
    pub(crate) stencil: StencilKey,
    pub(crate) shader: (u64, bool),
    /// Fur shells, outline and x-ray, for opaque and cut-out draws.
    pub(crate) fur: Option<(u32, f32)>,
    pub(crate) outline: bool,
    pub(crate) xray: bool,
}

impl DrawClass {
    /// Whether the draw's mesh goes into the shadow maps.
    pub(crate) fn casts_shadow(&self) -> bool {
        matches!(self.pass, MeshPass::Opaque | MeshPass::Mask)
    }

    /// How far fur reaches past the mesh's bounds.
    pub(crate) fn reach(&self) -> f32 {
        self.fur.map_or(0.0, |f| f.1)
    }
}

pub(crate) fn classify(m: &Material, transparency: Transparency) -> DrawClass {
    let pass = if m.transmits() {
        MeshPass::Transmit
    } else {
        match m.alpha_mode {
            AlphaMode::Opaque => MeshPass::Opaque,
            AlphaMode::Mask(_) => MeshPass::Mask,
            AlphaMode::Blend if transparency == Transparency::Sorted => MeshPass::Blend,
            AlphaMode::Blend => MeshPass::Oit,
        }
    };
    let solid = matches!(pass, MeshPass::Opaque | MeshPass::Mask);
    let outline = solid && m.outline.is_some_and(|o| o.width > 0.0 && o.color.a > 0.0);
    let stencil = if outline {
        StencilKey::OutlineMark
    } else {
        StencilKey::of(&m.stencil)
    };
    DrawClass {
        pass,
        stencil,
        shader: m
            .shader
            .as_ref()
            .map_or((0, false), |s| (s.id(), s.uniform_size() > 0)),
        fur: m
            .fur
            .as_ref()
            .filter(|f| solid && f.shells > 0 && f.length > 0.0)
            .map(|f| (f.shells.min(64), f.length)),
        outline,
        xray: solid && m.xray.is_some_and(|c| c.a > 0.0),
    }
}

/// The draws of the feature passes, collected while the lit draws are
/// sorted into their own lists.
#[derive(Debug, Default)]
pub(crate) struct Extras {
    pub(crate) fur: Vec<(Resolved, u32)>,
    pub(crate) outline: Vec<Resolved>,
    pub(crate) xray: Vec<Resolved>,
    pub(crate) transmit: Vec<Resolved>,
    pub(crate) oit: Vec<Resolved>,
}

/// `r` with its pipeline changed to `pass` and `stencil`.
fn with_pass(r: &Resolved, rec: &SceneRecord, pass: MeshPass, stencil: StencilKey) -> Resolved {
    let d = &rec.draws[r.draw as usize];
    let mesh_key = MeshKey {
        pass,
        stencil,
        shader: if pass == MeshPass::Fur {
            r.mesh_key.shader
        } else {
            (0, false)
        },
        ..r.mesh_key
    };
    Resolved {
        key: pack_key(&mesh_key, d.material, d.mesh),
        mesh_key,
        ..*r
    }
}

impl Extras {
    /// Files a visible draw under the passes its class adds or takes.
    pub(crate) fn add(&mut self, rec: &SceneRecord, class: &DrawClass, r: Resolved) {
        match class.pass {
            MeshPass::Transmit => self.transmit.push(r),
            MeshPass::Oit => self.oit.push(r),
            _ => {}
        }
        if let Some((shells, _)) = class.fur {
            let fur = with_pass(&r, rec, MeshPass::Fur, StencilKey::None);
            self.fur.push((fur, shells));
        }
        if class.outline {
            self.outline.push(with_pass(
                &r,
                rec,
                MeshPass::Outline,
                StencilKey::OutlineTest,
            ));
        }
        if class.xray {
            self.xray
                .push(with_pass(&r, rec, MeshPass::XRay, StencilKey::None));
        }
    }

    /// Builds the feature passes' batches: fur shells after the opaque
    /// batches, transmissive draws back to front, order-independent draws
    /// grouped by key (their order does not matter), outlines and x-ray.
    pub(crate) fn emit(
        &mut self,
        rec: &SceneRecord,
        instances: &mut Vec<GpuInstance>,
        out: &mut PassLists<'_>,
    ) {
        self.fur.sort_unstable_by_key(|(r, _)| r.key);
        let mut start = 0;
        while start < self.fur.len() {
            let key = self.fur[start].0.key;
            let end = start + self.fur[start..].partition_point(|(r, _)| r.key == key);
            emit_shells(rec, &self.fur[start..end], instances, out.opaque);
            start = end;
        }
        self.transmit.sort_by(|a, b| b.depth.total_cmp(&a.depth));
        emit(rec, &self.transmit, 0, instances, out.transmit);
        self.oit.sort_unstable_by_key(|r| r.key);
        emit(rec, &self.oit, 0, instances, out.oit);
        self.outline.sort_unstable_by_key(|r| r.key);
        emit(rec, &self.outline, 0, instances, out.outline);
        self.xray.sort_unstable_by_key(|r| r.key);
        emit(rec, &self.xray, 0, instances, out.xray);
    }
}

/// The batch lists the feature passes add to.
pub(crate) struct PassLists<'a> {
    pub(crate) opaque: &'a mut Vec<Batch>,
    pub(crate) transmit: &'a mut Vec<Batch>,
    pub(crate) oit: &'a mut Vec<Batch>,
    pub(crate) outline: &'a mut Vec<Batch>,
    pub(crate) xray: &'a mut Vec<Batch>,
}

/// One batch for a run of fur draws sharing a key: every draw's
/// instance once per shell, the shell number above the morph count.
fn emit_shells(
    rec: &SceneRecord,
    run: &[(Resolved, u32)],
    instances: &mut Vec<GpuInstance>,
    out: &mut Vec<Batch>,
) {
    let Some((first, shells)) = run.first() else {
        return;
    };
    let d0 = &rec.draws[first.draw as usize];
    let at = instances.len() as u32;
    for shell in 0..*shells {
        for (r, _) in run {
            let mut inst = instance(&rec.draws[r.draw as usize], 0);
            inst.params[3] = (inst.params[3] & 0xff) | (shell << 8);
            instances.push(inst);
        }
    }
    out.push(Batch {
        key: first.mesh_key,
        sort: first.key,
        mesh: d0.mesh,
        material: d0.material,
        first: at,
        count: instances.len() as u32 - at,
    });
}
