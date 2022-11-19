use std::vec;
use syncpool::prelude::*;
use crate::policy::{DefaultPolicy, Policy};

pub type RingConsumer = Box<dyn Fn(Vec<u64>) -> bool>;

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

    fn push(&mut self, item: u64) {
        self.data.push(item);
        if self.data.len() >= self.capa {
            unsafe {
                if let Some(cons) = self.cons.as_mut(){
                    if cons.push(self.data.clone()) {
                        self.data = vec![0u64; self.capa];
                    } else {
                        self.data = vec![];
                    }
                }else {
                    self.data = vec![];
                }

            }
        }
    }
}

pub struct RingBuffer<T> {
    pool: SyncPool<RingStripe<T>>,
}

impl<T> Clone  for RingBuffer<T> {
    fn clone(& self) -> Self {

        Self {
            pool:SyncPool::with_packer(RingStripe::initializer)
        }
    }
}

impl<T> RingBuffer<T> {
    pub fn new(f: *mut DefaultPolicy<T>, capa: usize) -> Self
    {
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


