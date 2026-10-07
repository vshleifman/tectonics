//! Loops over cells that may run on every core. With the `parallel` feature
//! they use rayon, and without it they are plain loops. Either way the work is
//! cut into the same fixed chunks of `CHUNK` indices, whatever the thread
//! count, and anything gathered from the chunks is combined in chunk order. So
//! a sum is always added up in the same order, and the world comes out the
//! same on one thread, on many, or from the single-threaded build.

use std::marker::PhantomData;
use std::ops::Range;

#[cfg(feature = "parallel")]
use rayon::prelude::*;

/// Indices per chunk: enough to make each one worth handing to a thread.
pub const CHUNK: usize = 4096;

fn chunk_count(n: usize) -> usize {
    n.div_ceil(CHUNK)
}

fn chunk_range(chunk: usize, n: usize) -> Range<usize> {
    chunk * CHUNK..((chunk + 1) * CHUNK).min(n)
}

/// `f` over each chunk of `0..n`, with the results in chunk order.
pub fn chunks<T: Send>(n: usize, f: impl Fn(Range<usize>) -> T + Sync + Send) -> Vec<T> {
    #[cfg(feature = "parallel")]
    {
        (0..chunk_count(n))
            .into_par_iter()
            .map(|c| f(chunk_range(c, n)))
            .collect()
    }
    #[cfg(not(feature = "parallel"))]
    {
        (0..chunk_count(n)).map(|c| f(chunk_range(c, n))).collect()
    }
}

/// Call `f` on every index in `0..n`, in no particular order.
pub fn for_each(n: usize, f: impl Fn(usize) + Sync + Send) {
    #[cfg(feature = "parallel")]
    {
        (0..chunk_count(n))
            .into_par_iter()
            .for_each(|c| chunk_range(c, n).for_each(&f));
    }
    #[cfg(not(feature = "parallel"))]
    {
        (0..n).for_each(f);
    }
}

/// Call `f` on every index of `out` and the element there.
pub fn for_each_mut<T: Send>(out: &mut [T], f: impl Fn(usize, &mut T) + Sync + Send) {
    #[cfg(feature = "parallel")]
    {
        out.par_chunks_mut(CHUNK).enumerate().for_each(|(c, part)| {
            for (j, x) in part.iter_mut().enumerate() {
                f(c * CHUNK + j, x);
            }
        });
    }
    #[cfg(not(feature = "parallel"))]
    {
        for (i, x) in out.iter_mut().enumerate() {
            f(i, x);
        }
    }
}

/// Set every `out[i]` to `f(i)`.
pub fn fill<T: Send>(out: &mut [T], f: impl Fn(usize) -> T + Sync + Send) {
    for_each_mut(out, |i, x| *x = f(i));
}

/// Set every `out[i]` to `f(i)`, for a few items that each cost a lot: they
/// are split as finely as the pool likes rather than by `CHUNK`. Nothing is
/// combined across items, so the split makes no difference to the result.
pub fn fill_costly<T: Send>(out: &mut [T], f: impl Fn(usize) -> T + Sync + Send) {
    #[cfg(feature = "parallel")]
    out.par_iter_mut().enumerate().for_each(|(i, x)| *x = f(i));
    #[cfg(not(feature = "parallel"))]
    for (i, x) in out.iter_mut().enumerate() {
        *x = f(i);
    }
}

/// Set every `(a[i], b[i])` to `f(i)`; the two must be as long.
pub fn fill_pair<A: Send, B: Send>(
    a: &mut [A],
    b: &mut [B],
    f: impl Fn(usize) -> (A, B) + Sync + Send,
) {
    assert_eq!(a.len(), b.len());
    let set = |c: usize, pa: &mut [A], pb: &mut [B]| {
        for (j, (x, y)) in pa.iter_mut().zip(pb.iter_mut()).enumerate() {
            (*x, *y) = f(c * CHUNK + j);
        }
    };
    #[cfg(feature = "parallel")]
    {
        a.par_chunks_mut(CHUNK)
            .zip(b.par_chunks_mut(CHUNK))
            .enumerate()
            .for_each(|(c, (pa, pb))| set(c, pa, pb));
    }
    #[cfg(not(feature = "parallel"))]
    {
        for (c, (pa, pb)) in a.chunks_mut(CHUNK).zip(b.chunks_mut(CHUNK)).enumerate() {
            set(c, pa, pb);
        }
    }
}

/// `src` copied into `dst`, which must be as long.
pub fn copy<T: Copy + Send + Sync>(dst: &mut [T], src: &[T]) {
    assert_eq!(dst.len(), src.len());
    fill(dst, |i| src[i]);
}

/// Sum of `f(i)` over `0..n`, added up within each chunk and then over the
/// chunks, in order.
pub fn sum(n: usize, f: impl Fn(usize) -> f64 + Sync + Send) -> f64 {
    chunks(n, |r| r.map(&f).sum::<f64>()).into_iter().sum()
}

/// The `Some` results of `f` over `0..n`, in index order.
pub fn filter_map<T: Send>(n: usize, f: impl Fn(usize) -> Option<T> + Sync + Send) -> Vec<T> {
    concat(chunks(n, |r| r.filter_map(&f).collect()))
}

/// The `Some` results of `f` over the items of `of`, in order.
pub fn filter_map_slice<S: Sync, T: Send>(
    of: &[S],
    f: impl Fn(&S) -> Option<T> + Sync + Send,
) -> Vec<T> {
    concat(chunks(of.len(), |r| of[r].iter().filter_map(&f).collect()))
}

/// Per-chunk lists joined end to end.
pub fn concat<T>(parts: Vec<Vec<T>>) -> Vec<T> {
    let mut out = Vec::with_capacity(parts.iter().map(Vec::len).sum());
    for part in parts {
        out.extend(part);
    }
    out
}

/// Sort `v`. Its keys must be distinct, so the order is the same however the
/// sort is split up.
pub fn sort_unstable<T: Ord + Send>(v: &mut [T]) {
    #[cfg(feature = "parallel")]
    v.par_sort_unstable();
    #[cfg(not(feature = "parallel"))]
    v.sort_unstable();
}

/// A slice a parallel loop writes through by index, for a pass that sets
/// several per-cell fields at once, which a plain `for_each_mut` over one
/// field cannot. Holding it borrows the slice mutably, so the rest of the
/// pass can only reach the slice through it.
pub struct Disjoint<'a, T> {
    ptr: *mut T,
    len: usize,
    _slice: PhantomData<&'a mut [T]>,
}

// Safety: shared only by loops whose iterations each touch their own indices
// (see `Disjoint::get` and `Disjoint::set`), so no element is reached from
// two threads at once.
unsafe impl<T: Send> Send for Disjoint<'_, T> {}
unsafe impl<T: Send> Sync for Disjoint<'_, T> {}

impl<'a, T: Copy> Disjoint<'a, T> {
    pub fn new(slice: &'a mut [T]) -> Disjoint<'a, T> {
        Disjoint {
            ptr: slice.as_mut_ptr(),
            len: slice.len(),
            _slice: PhantomData,
        }
    }

    /// # Safety
    /// No other iteration of the loop may write index `i`.
    #[inline]
    pub unsafe fn get(&self, i: usize) -> T {
        assert!(i < self.len);
        unsafe { *self.ptr.add(i) }
    }

    /// # Safety
    /// Index `i` must belong to this iteration of the loop alone: no other
    /// iteration may read or write it.
    #[inline]
    pub unsafe fn set(&self, i: usize, value: T) {
        assert!(i < self.len);
        unsafe { *self.ptr.add(i) = value }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunked_results_come_back_in_index_order() {
        let n = 3 * CHUNK + 17;
        let odd = filter_map(n, |i| (i % 2 == 1).then_some(i));
        assert!(odd.windows(2).all(|w| w[0] < w[1]));
        assert_eq!(odd.len(), n / 2);
        let mut out = vec![0usize; n];
        fill(&mut out, |i| i * 2);
        assert!(out.iter().enumerate().all(|(i, &x)| x == i * 2));
        let total = sum(n, |i| i as f64);
        assert_eq!(total, (n * (n - 1) / 2) as f64);
    }
}
