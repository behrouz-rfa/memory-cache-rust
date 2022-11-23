use std::sync::atomic::Ordering;
use std::vec;
use seize::{Collector, Guard};
use syncpool::prelude::*;
use crate::policy::{DefaultPolicy, Policy};
use crate::reclaim::{Atomic, Shared};

pub type RingConsumer = Box<dyn Fn(Vec<u64>) -> bool>;

/// ringStripe is a singular ring buffer that is not concurrent safe.
#[derive(Clone)]
pub struct RingStripe<T> {
    pub(crate) data: Atomic<Vec<u64>>,
    pub capa: usize,
    pub(crate)  cons: Atomic<DefaultPolicy<T>>,

}


impl<T> RingStripe<T> {
    fn new(capa: usize, p: Shared<DefaultPolicy<T>>) -> Self {
        RingStripe {
            data: Atomic::null(),
            capa,
            cons: Atomic::from(p),

        }
    }
    /// Push appends an item in the ring buffer and drains (copies items and
    /// sends to Consumer) if full.
    fn push<'g>(&'g self, item: u64, guard: &'g Guard) {
        let mut data = self.data.load(Ordering::SeqCst, guard);
        if data.is_null() {
            data = Shared::boxed(vec![0; self.capa], guard.collector().unwrap());
            self.data.store(data, Ordering::SeqCst);
        }
        let data = unsafe { data.as_ptr() };
        let data = unsafe { data.as_mut().unwrap() };

        data.push(item);
        if data.len() >= self.capa {
            unsafe {
                let p = self.cons.load(Ordering::SeqCst, guard);
                let p = unsafe {p.as_ptr()};
                let p = unsafe {p.as_mut().unwrap()};
                let mut data = self.data.load(Ordering::SeqCst, guard);
                if data.is_null() || !unsafe { data.deref() }.is_empty() {
                    data = Shared::boxed(Vec::with_capacity(self.capa), guard.collector().unwrap());
                    self.data.store(data, Ordering::SeqCst);
                }
                let data = data.as_ptr();
                if p.push(data.as_mut().unwrap().clone(), guard) {
                    let empty = Shared::boxed(vec![0; self.capa], guard.collector().unwrap());
                    self.data.store(empty, Ordering::SeqCst);
                } else {
                    let empty = Shared::boxed(vec![0; self.capa], guard.collector().unwrap());
                    self.data.store(empty, Ordering::SeqCst);
                }
            }
        }
    }
}

/// ringBuffer stores multiple buffers (stripes) and distributes Pushed items
/// between them to lower contention.
///
/// This implements the "batching" process described in the BP-Wrapper paper
/// (section III part A).
#[derive(Clone)]
pub struct RingBuffer<T> {
    pool: RingStripe<T>,
}
//
// impl<'g,T> Clone for RingBuffer<'g,T> {
//     fn clone(&self) -> Self {
//         Self {
//             pool:self.pool,
//             guard: self.guard
//         }
//     }
// }

impl<T> RingBuffer<T> {
    /// newRingBuffer returns a striped ring buffer. The Consumer in ringConfig will
    /// be called when individual stripes are full and need to drain their elements.
    pub(crate)  fn new(f: Shared<DefaultPolicy<T>>, capa: usize) -> Self
    {
        // LOSSY buffers use a very simple sync.Pool for concurrently reusing
        // stripes. We do lose some stripes due to GC (unheld items in sync.Pool
        // are cleared), but the performance gains generally outweigh the small
        // percentage of elements lost. The performance primarily comes from
        // low-level runtime functions used in the standard library that aren't
        // available to us (such as runtime_procPin()).

        RingBuffer {
            pool: RingStripe::new(capa, f),
        }
    }
    /// Push adds an element to one of the internal stripes and possibly drains if
    /// the stripe becomes full.
    pub fn push<'g>(&'g self, item: u64, guard: &'g Guard) {
        self.pool.push(item, guard);
        // self.pool.put(g);
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ring_drain() {
        // let r := RingBuffer::new()
    }
}


