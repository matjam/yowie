//! Shape generators: cubes, spheres, planes, cylinders, capsules and tori,
//! each centred on the origin with normals, tangents and texture
//! coordinates.

use std::f32::consts::{PI, TAU};

use yowie_core::{Vec2, Vec3, vec2, vec3};

use super::vertex::{MeshData, Vertex3D};

impl MeshData {
    /// A cube `size` units on a side, with its own texture square on each
    /// face.
    pub fn cube(size: f32) -> MeshData {
        let h = size * 0.5;
        // (normal, u axis, v axis) per face; v runs down the texture.
        let faces = [
            (Vec3::X, Vec3::NEG_Z, Vec3::NEG_Y),
            (Vec3::NEG_X, Vec3::Z, Vec3::NEG_Y),
            (Vec3::Y, Vec3::X, Vec3::Z),
            (Vec3::NEG_Y, Vec3::X, Vec3::NEG_Z),
            (Vec3::Z, Vec3::X, Vec3::NEG_Y),
            (Vec3::NEG_Z, Vec3::NEG_X, Vec3::NEG_Y),
        ];
        let mut data = MeshData::default();
        for (n, u, v) in faces {
            let base = data.vertices.len() as u32;
            for (su, sv) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
                let p = (n + u * su + v * sv) * h;
                let mut vert = Vertex3D::new(p, n, vec2((su + 1.0) * 0.5, (sv + 1.0) * 0.5));
                vert.tangent = u.extend(1.0);
                data.vertices.push(vert);
            }
            // Counter-clockwise seen from outside: u cross v points inward
            // for these axes, so the quad winds 0 3 2, 0 2 1.
            let quad = if u.cross(v).dot(n) > 0.0 {
                [0, 1, 2, 0, 2, 3]
            } else {
                [0, 3, 2, 0, 2, 1]
            };
            data.indices.extend(quad.map(|i| base + i));
        }
        data
    }

    /// A UV sphere of `radius` with `rings` bands from pole to pole and
    /// `segments` around.
    pub fn sphere(radius: f32, rings: u32, segments: u32) -> MeshData {
        let (rings, segments) = (rings.max(2), segments.max(3));
        let mut data = MeshData::default();
        for r in 0..=rings {
            let v = r as f32 / rings as f32;
            let theta = v * PI;
            for s in 0..=segments {
                let u = s as f32 / segments as f32;
                let phi = u * TAU;
                let n = vec3(
                    -phi.sin() * theta.sin(),
                    theta.cos(),
                    -phi.cos() * theta.sin(),
                );
                let mut vert = Vertex3D::new(n * radius, n, vec2(u, v));
                vert.tangent = vec3(-phi.cos(), 0.0, phi.sin()).extend(1.0);
                data.vertices.push(vert);
            }
        }
        grid_indices(&mut data.indices, rings, segments);
        data
    }

    /// A flat square `size` units across in the XZ plane facing +Y, cut
    /// into `subdivisions` cells on each side.
    pub fn plane(size: f32, subdivisions: u32) -> MeshData {
        let n = subdivisions.max(1);
        let mut data = MeshData::default();
        for z in 0..=n {
            for x in 0..=n {
                let uv = vec2(x as f32, z as f32) / n as f32;
                let p = vec3(uv.x - 0.5, 0.0, uv.y - 0.5) * size;
                data.vertices.push(Vertex3D::new(p, Vec3::Y, uv));
            }
        }
        grid_indices(&mut data.indices, n, n);
        data
    }

    /// A capped cylinder of `radius` and `height` along Y with `segments`
    /// around.
    pub fn cylinder(radius: f32, height: f32, segments: u32) -> MeshData {
        let segments = segments.max(3);
        let h = height * 0.5;
        let mut data = MeshData::default();
        for (y, v) in [(h, 0.0), (-h, 1.0)] {
            for s in 0..=segments {
                let u = s as f32 / segments as f32;
                let a = u * TAU;
                let n = vec3(a.sin(), 0.0, a.cos());
                let mut vert = Vertex3D::new(vec3(n.x * radius, y, n.z * radius), n, vec2(u, v));
                vert.tangent = vec3(a.cos(), 0.0, -a.sin()).extend(1.0);
                data.vertices.push(vert);
            }
        }
        grid_indices(&mut data.indices, 1, segments);
        cap(&mut data, radius, h, segments, true);
        cap(&mut data, radius, -h, segments, false);
        data
    }

    /// A capsule along Y: a cylinder of `radius` whose straight part runs
    /// `half_height` above and below the centre, closed by hemispheres.
    /// `rings` bands cover each hemisphere.
    pub fn capsule(radius: f32, half_height: f32, rings: u32, segments: u32) -> MeshData {
        let (rings, segments) = (rings.max(1), segments.max(3));
        let total = 2.0 * (half_height + radius);
        let mut data = MeshData::default();
        let rows = rings * 2 + 1;
        for r in 0..=rows {
            // Rows 0..=rings cover the top hemisphere, the rest the bottom.
            let (theta, offset) = if r <= rings {
                (r as f32 / rings as f32 * PI * 0.5, half_height)
            } else {
                ((r - 1) as f32 / rings as f32 * PI * 0.5, -half_height)
            };
            for s in 0..=segments {
                let u = s as f32 / segments as f32;
                let phi = u * TAU;
                let n = vec3(
                    phi.sin() * theta.sin(),
                    theta.cos(),
                    phi.cos() * theta.sin(),
                );
                let p = n * radius + Vec3::Y * offset;
                let v = 0.5 - p.y / total;
                let mut vert = Vertex3D::new(p, n, vec2(u, v));
                vert.tangent = vec3(phi.cos(), 0.0, -phi.sin()).extend(1.0);
                data.vertices.push(vert);
            }
        }
        grid_indices(&mut data.indices, rows, segments);
        data
    }

    /// A torus in the XZ plane: a tube of `minor` radius around a circle of
    /// `major` radius, with `rings` steps around the circle and `segments`
    /// around the tube.
    pub fn torus(major: f32, minor: f32, rings: u32, segments: u32) -> MeshData {
        let (rings, segments) = (rings.max(3), segments.max(3));
        let mut data = MeshData::default();
        for r in 0..=rings {
            let u = r as f32 / rings as f32;
            let a = u * TAU;
            let centre = vec3(a.sin(), 0.0, a.cos());
            for s in 0..=segments {
                let v = s as f32 / segments as f32;
                let b = v * TAU;
                let n = centre * b.cos() + Vec3::Y * b.sin();
                let p = centre * major + n * minor;
                let mut vert = Vertex3D::new(p, n, vec2(u, v));
                vert.tangent = vec3(a.cos(), 0.0, -a.sin()).extend(1.0);
                data.vertices.push(vert);
            }
        }
        grid_indices(&mut data.indices, rings, segments);
        data
    }
}

/// Indices for a grid of `rows` by `cols` cells over `(rows + 1) * (cols +
/// 1)` vertices laid out row by row, counter-clockwise when the row step
/// crossed with the column step points outward.
fn grid_indices(out: &mut Vec<u32>, rows: u32, cols: u32) {
    let stride = cols + 1;
    for r in 0..rows {
        for c in 0..cols {
            let a = r * stride + c;
            let b = a + stride;
            out.extend_from_slice(&[a, b, a + 1, a + 1, b, b + 1]);
        }
    }
}

/// Appends a disc closing a cylinder at height `y`, facing up or down.
fn cap(data: &mut MeshData, radius: f32, y: f32, segments: u32, up: bool) {
    let n = if up { Vec3::Y } else { Vec3::NEG_Y };
    let centre = data.vertices.len() as u32;
    let mut vert = Vertex3D::new(vec3(0.0, y, 0.0), n, Vec2::splat(0.5));
    vert.tangent = Vec3::X.extend(1.0);
    data.vertices.push(vert);
    for s in 0..=segments {
        let a = s as f32 / segments as f32 * TAU;
        let (x, z) = (a.sin(), a.cos());
        let uv = vec2(0.5 + x * 0.5, 0.5 + if up { z } else { -z } * 0.5);
        let mut v = Vertex3D::new(vec3(x * radius, y, z * radius), n, uv);
        v.tangent = Vec3::X.extend(1.0);
        data.vertices.push(v);
    }
    for s in 0..segments {
        let (a, b) = (centre + 1 + s, centre + 2 + s);
        if up {
            data.indices.extend_from_slice(&[centre, a, b]);
        } else {
            data.indices.extend_from_slice(&[centre, b, a]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every triangle's geometric normal agrees with its vertex normals,
    /// which checks the winding is counter-clockwise from outside.
    fn check_winding(name: &str, data: &MeshData) {
        assert!(!data.indices.is_empty(), "{name}");
        let mut bad = 0;
        for t in data.indices.as_chunks::<3>().0 {
            let [a, b, c] = t.map(|i| data.vertices[i as usize]);
            let face = (b.position - a.position).cross(c.position - a.position);
            if face.length_squared() < 1e-12 {
                continue;
            }
            if face.dot(a.normal + b.normal + c.normal) <= 0.0 {
                bad += 1;
            }
        }
        assert_eq!(bad, 0, "{name}: {bad} triangles wind the wrong way");
    }

    #[test]
    fn shapes_wind_outward() {
        let shapes = [
            ("cube", MeshData::cube(1.0)),
            ("sphere", MeshData::sphere(1.0, 8, 12)),
            ("plane", MeshData::plane(2.0, 3)),
            ("cylinder", MeshData::cylinder(0.5, 2.0, 10)),
            ("capsule", MeshData::capsule(0.5, 0.5, 4, 10)),
            ("torus", MeshData::torus(1.0, 0.25, 12, 8)),
        ];
        for (name, data) in &shapes {
            check_winding(name, data);
            assert!(
                data.indices
                    .iter()
                    .all(|i| (*i as usize) < data.vertices.len())
            );
        }
        let b = MeshData::capsule(0.5, 1.0, 4, 8).bounds();
        assert!((b.max.y - 1.5).abs() < 1e-5 && (b.min.y + 1.5).abs() < 1e-5);
    }

    #[test]
    fn tangents_follow_u() {
        let mut data = MeshData::plane(1.0, 1);
        data.compute_tangents();
        for v in &data.vertices {
            assert!((v.tangent.truncate() - Vec3::X).length() < 1e-5);
        }
    }
}
