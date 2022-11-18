use std::vec;
use syncpool::prelude::*;
use crate::policy::{DefaultPolicy, Policy};

pub type RingConsumer = Box<dyn Fn(Vec<u64>) -> bool>;

pub struct RingStripe {
    pub data: Vec<u64>,
    pub capa: usize,
    cons: *mut DefaultPolicy,
}


impl RingStripe {
    fn initializer(mut self: Box<Self>) -> Box<Self> {
        // self.data = vec![0; capa];
        // self.capa = capa;
        self
    }
    fn new(capa: usize, p: *mut DefaultPolicy) -> Self {
        RingStripe {
            data: vec![0; capa],
            capa,
            cons: p,
        }
    }

    fn push(&mut self, item: u64) {
        self.data.push(item);
        if self.data.len() >= self.capa {
            unsafe {
                if self.cons.as_mut().unwrap().push(self.data.clone()) {
                    self.data = vec![0u64; self.capa];
                } else {
                    self.data = vec![];
                }
            }
        }
    }
}

pub struct RingBuffer {
    pool: SyncPool<RingStripe>,
}

impl RingBuffer {
    pub fn new(f: *mut DefaultPolicy, capa: usize) -> Self
    {
        let mut p = SyncPool::with_packer(RingStripe::initializer);
        p.get().data = vec![0; capa];
        p.get().capa = capa;
        p.get().cons = f;
        RingBuffer {
            pool: p
        }
    }
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


