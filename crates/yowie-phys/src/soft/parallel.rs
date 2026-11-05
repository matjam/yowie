//! Optional data parallelism for per-particle passes.

/// Smallest slice length worth splitting across threads.
#[cfg(feature = "rayon")]
const PAR_MIN: usize = 512;

/// Sets `out[i] = f(i)` for every index. With the `rayon` feature, large
/// slices are filled on several threads; the result is identical either way
/// because each element depends only on its own index.
pub(crate) fn fill_with<T: Send>(out: &mut [T], f: impl Fn(usize) -> T + Sync + Send) {
    #[cfg(feature = "rayon")]
    if out.len() >= PAR_MIN {
        use rayon::prelude::*;
        out.par_iter_mut().enumerate().for_each(|(i, o)| *o = f(i));
        return;
    }
    for (i, o) in out.iter_mut().enumerate() {
        *o = f(i);
    }
}

#[cfg(test)]
mod tests {
    use super::fill_with;

    #[test]
    fn fills_every_index() {
        for n in [0, 1, 700, 2000] {
            let mut v = vec![0usize; n];
            fill_with(&mut v, |i| i * 2);
            assert!(v.iter().enumerate().all(|(i, x)| *x == i * 2));
        }
    }
}
