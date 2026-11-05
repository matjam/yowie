//! A bounding volume hierarchy over boxes: median splits along the longest
//! axis of the centres, leaves of up to eight, stored flat in depth-first
//! order with each node's subtree end for skipping.

use crate::scene::bounds::{Aabb, Frustum};

/// The most items in a leaf.
pub(crate) const LEAF_SIZE: usize = 8;

/// One node: its box, and either its items (a leaf) or its children
/// (the left child follows it).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Node {
    pub(crate) bounds: Aabb,
    /// First entry of `Bvh::items` (leaves only).
    pub(crate) start: u32,
    /// Items in the leaf; 0 for an inner node.
    pub(crate) count: u32,
    /// The index past this node's subtree.
    pub(crate) skip: u32,
}

/// The hierarchy: nodes, and item indices in leaf order.
#[derive(Clone, Debug, Default)]
pub(crate) struct Bvh {
    pub(crate) nodes: Vec<Node>,
    pub(crate) items: Vec<u32>,
}

impl Bvh {
    /// Builds the hierarchy over `boxes`.
    pub(crate) fn build(boxes: &[Aabb]) -> Self {
        let mut bvh = Bvh {
            nodes: Vec::with_capacity(boxes.len() / LEAF_SIZE * 2 + 1),
            items: (0..boxes.len() as u32).collect(),
        };
        if !boxes.is_empty() {
            let n = boxes.len();
            bvh.split(boxes, 0, n);
        }
        bvh
    }

    fn split(&mut self, boxes: &[Aabb], lo: usize, hi: usize) {
        let at = self.nodes.len();
        let bounds = self.items[lo..hi]
            .iter()
            .fold(Aabb::EMPTY, |b, &i| b.union(boxes[i as usize]));
        self.nodes.push(Node {
            bounds,
            start: lo as u32,
            count: 0,
            skip: 0,
        });
        if hi - lo <= LEAF_SIZE {
            self.nodes[at].count = (hi - lo) as u32;
        } else {
            let centres = self.items[lo..hi]
                .iter()
                .fold(Aabb::EMPTY, |b, &i| b.grown(boxes[i as usize].center()));
            let size = centres.max - centres.min;
            let axis = if size.x >= size.y && size.x >= size.z {
                0
            } else if size.y >= size.z {
                1
            } else {
                2
            };
            let mid = lo + (hi - lo) / 2;
            self.items[lo..hi].select_nth_unstable_by(mid - lo, |a, b| {
                let ca = boxes[*a as usize].center()[axis];
                let cb = boxes[*b as usize].center()[axis];
                ca.total_cmp(&cb)
            });
            self.split(boxes, lo, mid);
            self.split(boxes, mid, hi);
        }
        self.nodes[at].skip = self.nodes.len() as u32;
    }

    /// Calls `visit` with each item in a leaf whose box touches `frustum`.
    /// Returns the nodes tested.
    pub(crate) fn walk(&self, frustum: &Frustum, mut visit: impl FnMut(u32)) -> u32 {
        let mut i = 0;
        let mut tests = 0;
        while i < self.nodes.len() {
            let n = &self.nodes[i];
            tests += 1;
            if !frustum.intersects_aabb(&n.bounds) {
                i = n.skip as usize;
                continue;
            }
            if n.count > 0 {
                for &item in &self.items[n.start as usize..(n.start + n.count) as usize] {
                    visit(item);
                }
                i = n.skip as usize;
            } else {
                i += 1;
            }
        }
        tests
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use yowie_core::Vec3;
    use yowie_core::glam::camera::rh::{proj::directx, view::look_at_mat4};

    fn grid(n: usize) -> Vec<Aabb> {
        (0..n)
            .map(|i| {
                let c = Vec3::new((i % 50) as f32 * 2.0, 0.0, (i / 50) as f32 * 2.0);
                Aabb::new(c - Vec3::splat(0.5), c + Vec3::splat(0.5))
            })
            .collect()
    }

    #[test]
    fn leaves_partition_the_items_and_nodes_hold_them() {
        let boxes = grid(1000);
        let bvh = Bvh::build(&boxes);
        let mut seen = vec![0u32; boxes.len()];
        for n in &bvh.nodes {
            if n.count > 0 {
                assert!(n.count as usize <= LEAF_SIZE);
                for &i in &bvh.items[n.start as usize..(n.start + n.count) as usize] {
                    seen[i as usize] += 1;
                    let b = boxes[i as usize];
                    assert!(n.bounds.min.cmple(b.min).all() && n.bounds.max.cmpge(b.max).all());
                }
            }
        }
        assert!(seen.iter().all(|&s| s == 1), "every item in one leaf");
        assert_eq!(bvh.nodes[0].skip as usize, bvh.nodes.len());
    }

    #[test]
    fn walking_finds_what_the_frustum_sees_with_few_tests() {
        let boxes = grid(2500);
        let bvh = Bvh::build(&boxes);
        let proj = directx::perspective(0.6, 1.0, 200.0, 0.1);
        let view = look_at_mat4(
            Vec3::new(10.0, 5.0, -10.0),
            Vec3::new(10.0, 0.0, 10.0),
            Vec3::Y,
        );
        let f = Frustum::from_view_proj(&(proj * view));
        let mut found = Vec::new();
        let tests = bvh.walk(&f, |i| found.push(i));
        let want: Vec<u32> = (0..boxes.len() as u32)
            .filter(|&i| f.intersects_aabb(&boxes[i as usize]))
            .collect();
        for w in &want {
            assert!(found.contains(w), "item {w} visible but not found");
        }
        assert!(
            found.len() < boxes.len() / 2,
            "{} of {}",
            found.len(),
            boxes.len()
        );
        assert!((tests as usize) < boxes.len() / 4, "{tests} tests");
        assert_eq!(Bvh::build(&[]).walk(&f, |_| {}), 0);
    }
}
