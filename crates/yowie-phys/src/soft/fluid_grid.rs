//! Smoothing kernels and the neighbour grid of the 2D fluid.

use std::f32::consts::PI;

use yowie_core::Vec2;

/// 2D poly6 kernel, normalised over the disc of radius `h`.
pub(crate) fn poly6(r: f32, h: f32) -> f32 {
    if r >= h {
        return 0.0;
    }
    let d = h * h - r * r;
    4.0 / (PI * h.powi(8)) * d * d * d
}

/// Magnitude of the gradient of the 2D spiky kernel at distance `r`. Scale
/// the unit vector from the neighbour to the particle by it.
pub(crate) fn spiky_grad(r: f32, h: f32) -> f32 {
    if r >= h || r <= 0.0 {
        return 0.0;
    }
    let d = h - r;
    -30.0 / (PI * h.powi(5)) * d * d
}

/// Density of a square lattice of the given spacing, the density a fluid
/// is solved toward.
pub(crate) fn rest_density(radius: f32, spacing: f32) -> f32 {
    let reach = (radius / spacing).ceil() as i32;
    let mut rho = 0.0;
    for y in -reach..=reach {
        for x in -reach..=reach {
            rho += poly6(Vec2::new(x as f32, y as f32).length() * spacing, radius);
        }
    }
    rho
}

/// Neighbour lists of all particles within `radius` of each other, built
/// with a spatial hash.
#[derive(Debug, Clone, Default)]
pub(crate) struct Neighbours {
    start: Vec<u32>,
    list: Vec<u32>,
    bucket_start: Vec<u32>,
    items: Vec<u32>,
}

fn cell(p: Vec2, size: f32) -> (i32, i32) {
    ((p.x / size).floor() as i32, (p.y / size).floor() as i32)
}

fn bucket(c: (i32, i32), mask: u32) -> usize {
    let h = (c.0 as u32).wrapping_mul(73_856_093) ^ (c.1 as u32).wrapping_mul(19_349_663);
    (h & mask) as usize
}

impl Neighbours {
    /// Rebuilds the lists for `pos`.
    pub fn build(&mut self, pos: &[Vec2], radius: f32) {
        let n = pos.len();
        let size = (n.next_power_of_two().max(16)) as u32;
        let mask = size - 1;
        self.bucket_start.clear();
        self.bucket_start.resize(size as usize + 1, 0);
        for p in pos {
            self.bucket_start[bucket(cell(*p, radius), mask) + 1] += 1;
        }
        for i in 0..size as usize {
            self.bucket_start[i + 1] += self.bucket_start[i];
        }
        let mut fill = self.bucket_start.clone();
        self.items.clear();
        self.items.resize(n, 0);
        for (i, p) in pos.iter().enumerate() {
            let b = bucket(cell(*p, radius), mask);
            self.items[fill[b] as usize] = i as u32;
            fill[b] += 1;
        }
        self.start.clear();
        self.list.clear();
        let r2 = radius * radius;
        for (i, p) in pos.iter().enumerate() {
            self.start.push(self.list.len() as u32);
            let (cx, cy) = cell(*p, radius);
            let mut visited = [usize::MAX; 9];
            let mut count = 0;
            for dy in -1..=1 {
                for dx in -1..=1 {
                    let b = bucket((cx + dx, cy + dy), mask);
                    if visited[..count].contains(&b) {
                        continue;
                    }
                    visited[count] = b;
                    count += 1;
                    let range = self.bucket_start[b] as usize..self.bucket_start[b + 1] as usize;
                    for &j in &self.items[range] {
                        if j as usize != i && (pos[j as usize] - *p).length_squared() < r2 {
                            self.list.push(j);
                        }
                    }
                }
            }
        }
        self.start.push(self.list.len() as u32);
    }

    /// The neighbours of particle `i`.
    pub fn of(&self, i: usize) -> &[u32] {
        &self.list[self.start[i] as usize..self.start[i + 1] as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neighbours_match_brute_force() {
        let pos: Vec<Vec2> = (0..200)
            .map(|i| Vec2::new((i * 37 % 101) as f32 * 0.03, (i * 53 % 97) as f32 * 0.03))
            .collect();
        let radius = 0.2;
        let mut n = Neighbours::default();
        n.build(&pos, radius);
        for i in 0..pos.len() {
            let mut got: Vec<u32> = n.of(i).to_vec();
            got.sort_unstable();
            let want: Vec<u32> = (0..pos.len() as u32)
                .filter(|&j| j as usize != i && (pos[j as usize] - pos[i]).length() < radius)
                .collect();
            assert_eq!(got, want, "particle {i}");
        }
    }

    #[test]
    fn kernels_vanish_outside_radius() {
        assert_eq!(poly6(1.0, 1.0), 0.0);
        assert_eq!(spiky_grad(1.5, 1.0), 0.0);
        assert!(poly6(0.0, 1.0) > poly6(0.5, 1.0));
        assert!(spiky_grad(0.5, 1.0) < 0.0);
        assert!(rest_density(0.25, 0.1) > 0.0);
    }
}
