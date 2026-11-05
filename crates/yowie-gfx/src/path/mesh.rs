//! Turning paths into triangles: lyon fills and strokes, dashing, and the
//! feathered fringe that anti-aliases the result.

use std::collections::HashMap;

use lyon::path::PathEvent;
use lyon::path::iterator::PathIterator;
use lyon::tessellation::{
    BuffersBuilder, FillOptions, FillTessellator, FillVertex, LineCap, LineJoin, StrokeOptions,
    StrokeTessellator, StrokeVertex, VertexBuffers,
};
use yowie_core::{Vec2, vec2};

use super::build::Path;
use super::style::{Cap, Dash, FillRule, Join, Stroke};

/// Triangles with a coverage per vertex: 1 inside, falling to 0 across the
/// feathered fringe.
#[derive(Debug, Default)]
pub(crate) struct Mesh {
    pub(crate) points: Vec<Vec2>,
    pub(crate) alpha: Vec<f32>,
    pub(crate) indices: Vec<u32>,
}

fn to_mesh(buffers: VertexBuffers<Vec2, u32>) -> Mesh {
    Mesh {
        alpha: vec![1.0; buffers.vertices.len()],
        points: buffers.vertices,
        indices: buffers.indices,
    }
}

/// Tessellates the interior of `path`. `tolerance` is the largest distance
/// a curve may stray from its flattened form, in drawing units.
pub(crate) fn fill(path: &Path, rule: FillRule, tolerance: f32) -> Mesh {
    let rule = match rule {
        FillRule::NonZero => lyon::tessellation::FillRule::NonZero,
        FillRule::EvenOdd => lyon::tessellation::FillRule::EvenOdd,
    };
    let options = FillOptions::default()
        .with_fill_rule(rule)
        .with_tolerance(tolerance);
    let mut buffers: VertexBuffers<Vec2, u32> = VertexBuffers::new();
    let result = FillTessellator::new().tessellate_path(
        &path.inner,
        &options,
        &mut BuffersBuilder::new(&mut buffers, |v: FillVertex| {
            vec2(v.position().x, v.position().y)
        }),
    );
    if let Err(e) = result {
        log::warn!("yowie-gfx: path fill failed: {e:?}");
        return Mesh::default();
    }
    to_mesh(buffers)
}

/// Tessellates the outline of `path` as `stroke` describes, dashing it
/// first when the stroke has a dash pattern.
pub(crate) fn stroke(path: &Path, stroke: &Stroke, tolerance: f32) -> Mesh {
    let dashed;
    let source = match &stroke.dash {
        Some(d) if d.valid() => {
            dashed = dash(path, d, tolerance);
            &dashed
        }
        _ => path,
    };
    let cap = match stroke.cap {
        Cap::Butt => LineCap::Butt,
        Cap::Round => LineCap::Round,
        Cap::Square => LineCap::Square,
    };
    let join = match stroke.join {
        Join::Miter => LineJoin::Miter,
        Join::Round => LineJoin::Round,
        Join::Bevel => LineJoin::Bevel,
    };
    let options = StrokeOptions::default()
        .with_line_width(stroke.width.max(0.0))
        .with_line_cap(cap)
        .with_line_join(join)
        .with_miter_limit(stroke.miter_limit.max(1.0))
        .with_tolerance(tolerance);
    let mut buffers: VertexBuffers<Vec2, u32> = VertexBuffers::new();
    let result = StrokeTessellator::new().tessellate_path(
        &source.inner,
        &options,
        &mut BuffersBuilder::new(&mut buffers, |v: StrokeVertex| {
            vec2(v.position().x, v.position().y)
        }),
    );
    if let Err(e) = result {
        log::warn!("yowie-gfx: path stroke failed: {e:?}");
        return Mesh::default();
    }
    to_mesh(buffers)
}

/// Splits `path` into the drawn stretches of `dash`, as open subpaths.
fn dash(path: &Path, dash: &Dash, tolerance: f32) -> Path {
    let mut pattern = dash.pattern.clone();
    if pattern.len() % 2 == 1 {
        pattern.extend_from_within(..);
    }
    let total: f32 = pattern.iter().sum();
    let mut builder = Path::builder();
    let mut run: Vec<Vec2> = Vec::new();
    let mut index = 0usize;
    let mut left = pattern[0];
    let mut on = true;
    // Start part way into the pattern.
    let mut skip = dash.offset.rem_euclid(total);
    while skip > 0.0 {
        if skip >= left {
            skip -= left;
            index = (index + 1) % pattern.len();
            left = pattern[index];
            on = !on;
        } else {
            left -= skip;
            skip = 0.0;
        }
    }
    let flush = |run: &mut Vec<Vec2>, builder: &mut super::build::PathBuilder| {
        if run.len() >= 2 {
            builder.move_to(run[0]);
            for p in &run[1..] {
                builder.line_to(*p);
            }
        }
        run.clear();
    };
    for event in path.inner.iter().flattened(tolerance) {
        let (from, to) = match event {
            PathEvent::Begin { at } => {
                flush(&mut run, &mut builder);
                if on {
                    run.push(vec2(at.x, at.y));
                }
                continue;
            }
            PathEvent::Line { from, to } => (from, to),
            PathEvent::End {
                last,
                first,
                close: true,
            } => (last, first),
            _ => continue,
        };
        let (a, b) = (vec2(from.x, from.y), vec2(to.x, to.y));
        let length = (b - a).length();
        if length <= 0.0 {
            continue;
        }
        let mut at = 0.0;
        while at < length {
            let step = left.min(length - at);
            at += step;
            left -= step;
            let p = a + (b - a) * (at / length);
            if on {
                if run.is_empty() {
                    run.push(p - (b - a) * (step / length));
                }
                run.push(p);
            }
            if left <= 1e-6 {
                if on {
                    flush(&mut run, &mut builder);
                }
                index = (index + 1) % pattern.len();
                left = pattern[index];
                on = !on;
                if on && at < length {
                    run.push(p);
                }
            }
        }
    }
    flush(&mut run, &mut builder);
    builder.build()
}

fn key(p: Vec2) -> (i32, i32) {
    ((p.x * 256.0).round() as i32, (p.y * 256.0).round() as i32)
}

/// Adds a feathered fringe `width` drawing units wide around the mesh's
/// outer boundary: strips from full coverage at the boundary to none at
/// `width` outside it. The boundary is the set of edges used by a single
/// triangle, so it follows holes under either fill rule.
pub(crate) fn add_fringe(mesh: &mut Mesh, width: f32) {
    if mesh.indices.is_empty() || width <= 0.0 {
        return;
    }
    let mut ids: HashMap<(i32, i32), u32> = HashMap::new();
    let weld: Vec<u32> = mesh
        .points
        .iter()
        .map(|p| {
            let next = ids.len() as u32;
            *ids.entry(key(*p)).or_insert(next)
        })
        .collect();
    // Undirected edge -> (uses, vertex a, vertex b, the triangle's third point).
    let mut edges: HashMap<(u32, u32), (u32, u32, u32, Vec2)> = HashMap::new();
    for tri in mesh.indices.chunks_exact(3) {
        for k in 0..3 {
            let (a, b, c) = (tri[k], tri[(k + 1) % 3], tri[(k + 2) % 3]);
            let (wa, wb) = (weld[a as usize], weld[b as usize]);
            if wa == wb {
                continue;
            }
            let id = (wa.min(wb), wa.max(wb));
            let third = mesh.points[c as usize];
            let e = edges.entry(id).or_insert((0, a, b, third));
            e.0 += 1;
        }
    }
    struct Boundary {
        a: u32,
        b: u32,
        normal: Vec2,
    }
    let mut boundary = Vec::new();
    let mut sums: HashMap<u32, Vec2> = HashMap::new();
    for (uses, a, b, third) in edges.values().copied() {
        if uses != 1 {
            continue;
        }
        let (pa, pb) = (mesh.points[a as usize], mesh.points[b as usize]);
        let Some(dir) = (pb - pa).try_normalize() else {
            continue;
        };
        let mut n = dir.perp();
        if n.dot(third - pa) > 0.0 {
            n = -n;
        }
        *sums.entry(weld[a as usize]).or_insert(Vec2::ZERO) += n;
        *sums.entry(weld[b as usize]).or_insert(Vec2::ZERO) += n;
        boundary.push(Boundary { a, b, normal: n });
    }
    let mut outer: HashMap<u32, u32> = HashMap::new();
    let mut make = |mesh: &mut Mesh, v: u32, edge_normal: Vec2| -> u32 {
        let w = weld[v as usize];
        // Every edge meeting at a welded point shares one outer vertex, set
        // out along the mean of their normals.
        let sum = sums.get(&w).copied().unwrap_or(edge_normal);
        let n = sum.try_normalize().unwrap_or(edge_normal);
        let cos = n.dot(edge_normal).max(0.5);
        *outer.entry(w).or_insert_with(|| {
            mesh.points
                .push(mesh.points[v as usize] + n * (width / cos));
            mesh.alpha.push(0.0);
            (mesh.points.len() - 1) as u32
        })
    };
    for e in &boundary {
        let oa = make(mesh, e.a, e.normal);
        let ob = make(mesh, e.b, e.normal);
        mesh.indices.extend([e.a, e.b, ob, e.a, ob, oa]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use yowie_core::Rect;

    fn area(mesh: &Mesh) -> f32 {
        mesh.indices
            .chunks_exact(3)
            .map(|t| {
                let (a, b, c) = (
                    mesh.points[t[0] as usize],
                    mesh.points[t[1] as usize],
                    mesh.points[t[2] as usize],
                );
                ((b - a).perp_dot(c - a) * 0.5).abs()
            })
            .sum()
    }

    #[test]
    fn even_odd_cuts_a_hole_that_non_zero_keeps() {
        let mut b = Path::builder();
        b.add_rect(Rect::new(0.0, 0.0, 10.0, 10.0));
        b.add_rect(Rect::new(2.0, 2.0, 6.0, 6.0));
        let path = b.build();
        let nz = area(&fill(&path, FillRule::NonZero, 0.1));
        let eo = area(&fill(&path, FillRule::EvenOdd, 0.1));
        assert!((nz - 100.0).abs() < 0.01, "non-zero area {nz}");
        assert!((eo - 64.0).abs() < 0.01, "even-odd area {eo}");
    }

    #[test]
    fn dashes_cover_the_expected_length() {
        let mut b = Path::builder();
        b.move_to(vec2(0.0, 0.0)).line_to(vec2(100.0, 0.0));
        let path = b.build();
        let dashed = dash(&path, &Dash::new(10.0, 10.0), 0.1);
        let mut total = 0.0;
        let mut subpaths = 0;
        for e in dashed.inner.iter() {
            match e {
                PathEvent::Begin { .. } => subpaths += 1,
                PathEvent::Line { from, to } => total += (to - from).length(),
                _ => {}
            }
        }
        assert_eq!(subpaths, 5);
        assert!((total - 50.0).abs() < 0.01, "dashed length {total}");
    }

    #[test]
    fn fringe_surrounds_a_rectangle() {
        let path = Path::rect(Rect::new(0.0, 0.0, 10.0, 10.0));
        let mut mesh = fill(&path, FillRule::NonZero, 0.1);
        let inner = area(&mesh);
        add_fringe(&mut mesh, 1.0);
        let total = area(&mesh);
        assert!(total > inner + 35.0 && total < inner + 45.0, "{total}");
        assert!(mesh.alpha.iter().any(|a| *a == 0.0));
    }
}
