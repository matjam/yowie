use crate::Rng;

impl Rng {
    /// Returns a uniformly chosen element, or `None` for an empty slice.
    pub fn choose<'a, T>(&mut self, items: &'a [T]) -> Option<&'a T> {
        if items.is_empty() {
            return None;
        }
        items.get(self.below(items.len() as u64) as usize)
    }

    /// Returns a uniformly chosen element by mutable reference.
    pub fn choose_mut<'a, T>(&mut self, items: &'a mut [T]) -> Option<&'a mut T> {
        if items.is_empty() {
            return None;
        }
        let i = self.below(items.len() as u64) as usize;
        items.get_mut(i)
    }

    /// Returns the index of an element picked with probability
    /// proportional to its weight. Weights that are zero, negative or NaN
    /// are never picked. Returns `None` when no weight is positive.
    pub fn weighted_index(&mut self, weights: &[f32]) -> Option<usize> {
        let positive = |w: f32| w > 0.0 && w.is_finite();
        let total: f32 = weights.iter().copied().filter(|&w| positive(w)).sum();
        if total <= 0.0 {
            return None;
        }
        let mut x = self.float() * total;
        for (i, &w) in weights.iter().enumerate() {
            if !positive(w) {
                continue;
            }
            if x < w {
                return Some(i);
            }
            x -= w;
        }
        // Rounding left a remainder: fall back to the last positive weight.
        weights.iter().rposition(|&w| positive(w))
    }

    /// Returns an element picked with probability proportional to
    /// `weight(item)`. See [`Rng::weighted_index`] for the weight rules.
    pub fn choose_weighted<'a, T>(
        &mut self,
        items: &'a [T],
        weight: impl Fn(&T) -> f32,
    ) -> Option<&'a T> {
        let weights: Vec<f32> = items.iter().map(weight).collect();
        self.weighted_index(&weights).and_then(|i| items.get(i))
    }

    /// Shuffles a slice in place (Fisher-Yates).
    pub fn shuffle<T>(&mut self, items: &mut [T]) {
        for i in (1..items.len()).rev() {
            let j = self.below(i as u64 + 1) as usize;
            items.swap(i, j);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn choose_handles_empty_and_single() {
        let mut r = Rng::new(1);
        assert_eq!(r.choose::<u8>(&[]), None);
        assert_eq!(r.choose(&[5]), Some(&5));
        let mut v = [1, 2, 3];
        let picked = *r.choose_mut(&mut v).unwrap();
        assert!((1..=3).contains(&picked));
    }

    #[test]
    fn weighted_index_cases() {
        let mut r = Rng::new(2);
        let none: [&[f32]; 4] = [&[], &[0.0, 0.0], &[-1.0], &[f32::NAN]];
        for w in none {
            assert_eq!(r.weighted_index(w), None);
        }
        for _ in 0..100 {
            assert_eq!(r.weighted_index(&[0.0, 3.0, -2.0]), Some(1));
        }
        let mut counts = [0u32; 2];
        for _ in 0..10_000 {
            counts[r.weighted_index(&[1.0, 3.0]).unwrap()] += 1;
        }
        assert!((2200..2800).contains(&counts[0]), "{counts:?}");
    }

    #[test]
    fn choose_weighted_uses_weights() {
        let mut r = Rng::new(3);
        let items = [("a", 0.0), ("b", 1.0)];
        for _ in 0..50 {
            assert_eq!(r.choose_weighted(&items, |i| i.1).unwrap().0, "b");
        }
    }

    #[test]
    fn shuffle_permutes() {
        let mut r = Rng::new(4);
        let mut v: Vec<u32> = (0..20).collect();
        r.shuffle(&mut v);
        assert_ne!(v, (0..20).collect::<Vec<_>>());
        v.sort();
        assert_eq!(v, (0..20).collect::<Vec<_>>());
        let mut empty: [u8; 0] = [];
        r.shuffle(&mut empty);
    }
}
