use std::vec;
use syncpool::prelude::*;
use crate::policy::{DefaultPolicy, Policy};

pub type RingConsumer = Box<dyn Fn(Vec<u64>) -> bool>;

/// ringStripe is a singular ring buffer that is not concurrent safe.
#[derive(Clone)]
pub struct RingStripe<T> {
    pub data: Vec<u64>,
    pub capa: usize,
    pub cons: *mut DefaultPolicy<T>,
}


impl<T> RingStripe<T> {
    fn initializer(mut self: Box<Self>) -> Box<Self> {
        // self.data = vec![0; capa];
        // self.capa = capa;
        self
    }
    fn new(capa: usize, p: *mut DefaultPolicy<T>) -> Self {
        RingStripe {
            data: vec![0; capa],
            capa,
            cons: p,
        }
    }
    /// Push appends an item in the ring buffer and drains (copies items and
    /// sends to Consumer) if full.
    fn push(&mut self, item: u64) {
        self.data.push(item);
        if self.data.len() >= self.capa {
            unsafe {
                if let Some(cons) = self.cons.as_mut() {
                    if cons.push(self.data.clone()) {
                        self.data = vec![0u64; self.capa];
                    } else {
                        self.data = vec![];
                    }
                } else {
                    self.data = vec![];
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
pub struct RingBuffer<T> {
    pool: SyncPool<RingStripe<T>>,
}

impl<T> Clone for RingBuffer<T> {
    fn clone(&self) -> Self {
        Self {
            pool: SyncPool::with_packer(RingStripe::initializer)
        }
    }
}

impl<T> RingBuffer<T> {
    /// newRingBuffer returns a striped ring buffer. The Consumer in ringConfig will
    /// be called when individual stripes are full and need to drain their elements.
    pub fn new(f: *mut DefaultPolicy<T>, capa: usize) -> Self
    {
        // LOSSY buffers use a very simple sync.Pool for concurrently reusing
        // stripes. We do lose some stripes due to GC (unheld items in sync.Pool
        // are cleared), but the performance gains generally outweigh the small
        // percentage of elements lost. The performance primarily comes from
        // low-level runtime functions used in the standard library that aren't
        // available to us (such as runtime_procPin()).
        let mut p = SyncPool::with_packer(RingStripe::initializer);
        let mut g = p.get();
        let mut g = p.get();
        g.capa = capa;
        g.data = vec![0; capa];
        g.cons = f;
        p.put(g);
        RingBuffer {
            pool: p
        }
    }
    /// Push adds an element to one of the internal stripes and possibly drains if
    /// the stripe becomes full.
    pub fn push(&mut self, item: u64) {
        let mut g = self.pool.get();
        g.push(item);
        self.pool.put(g);
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


