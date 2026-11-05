use std::cmp::Ordering;

/// A queue entry ordered so that `BinaryHeap` pops the lowest `f` first.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Node {
    pub f: f32,
    pub i: u32,
}

impl PartialEq for Node {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for Node {}

impl PartialOrd for Node {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Node {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .f
            .total_cmp(&self.f)
            .then_with(|| other.i.cmp(&self.i))
    }
}
