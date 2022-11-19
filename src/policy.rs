use std::collections::HashMap;
use std::{ptr, thread};
use std::marker::PhantomData;
use std::sync::atomic::Ordering;
use crossbeam::epoch::{Atomic, Guard, Owned};
use parking_lot::Mutex;
use crate::bloom::bbloom::Bloom;
use crate::cache::{Item, ITEM_NEW, keepGets, keyUpdate, rejectSets};
use crate::cmsketch::CmSketch;
use crate::Metrics;

use crossbeam_channel::{select, unbounded, Sender, Receiver, TryRecvError};
use crate::ring::RingConsumer;

const lfuSample: usize = 5;

pub trait Policy {
    fn push(&self, key: [u64]) -> bool;
    // add attempts to add the key-cost pair to the Policy. It returns a slice
    // of evicted keys and a bool denoting whether or not the key-cost pair
    // was added. If it returns true, the key should be stored in cache.
    fn add<T>(&self, key: u64, cost: i64) -> (Vec<Item<T>>, bool);
    // Has returns true if the key exists in the Policy.
    fn has(&self, key: u64) -> bool;
    // Del deletes the key from the Policy.
    fn del(&self, key: u64);
    // Cap returns the available capacity.
    fn cap(&self) -> i64;
    // Close stops all goroutines and closes all channels.
    fn close(&self);
    // Update updates the cost value for the key.
    fn update(&self, key: u64, cost: i64);
    // Cost returns the cost value of a key or -1 if missing.
    fn cost(&self, key: u64) -> i64;
    // Optionally, set stats object to track how policy is performing.
    fn collect_metrics(&self, metrics: &mut Metrics);
    // Clear zeroes out all counters and clears hashmaps.
    fn clear(&self);
}

#[derive(Clone)]
pub struct DefaultPolicy<T> {
    pub admit: Atomic<TinyLFU>,
    pub item_ch: (Sender<Vec<u64>>, Receiver<Vec<u64>>),
    pub stop: (Sender<bool>, Receiver<bool>),
    pub evict: Atomic<SampledLFU>,
    pub metrics: *mut Metrics,
    _merker: PhantomData<T>,
}


impl<T> DefaultPolicy<T> {
    pub fn new(number_counters: i64, max_cost: i64) -> Self {
        let mut p = DefaultPolicy {
            admit: Atomic::new(TinyLFU::new(number_counters)),
            item_ch: unbounded::<Vec<u64>>(),
            stop: unbounded::<bool>(),
            evict: Atomic::new(SampledLFU::new(max_cost)),
            metrics: ptr::null_mut(),
            _merker: PhantomData,
        };


        // p.processItems();

        p
    }

    pub fn push(&mut self, keys: Vec<u64>) -> bool {
        if keys.len() == 0 {
            return true;
        }


        select! {
            send(self.item_ch.0,keys.clone())->res =>{
                if !self.metrics.is_null() {
                    unsafe{self.metrics.as_mut().unwrap().add(keepGets,keys[0],keys.len() as u64)};
                    return true;
                }

            },
            default=>{
              if !self.metrics.is_null() {
                    unsafe {self.metrics.as_mut().unwrap().add(keepGets,keys[0],keys.len() as u64)};
                    return false;
                }

            }
        }
        return true;
        // unsafe {
        //     if !self.metrics.is_null() {
        //         self.metrics.as_mut().unwrap().add(keepGets, keys[0], keys.len() as u64)
        //     }
        // };
        // true
    }
    pub fn collect_metrics(&mut self, metrics: &mut Metrics, guard: &Guard) {
        self.metrics = metrics;

        let evict = self.evict.load(Ordering::SeqCst, guard);
        if evict.is_null() {
            return;
        }

        let evict = unsafe { evict.deref() };
        let new_table = Owned::new(SampledLFU::new(evict.max_cost));

        self.evict.store(new_table, Ordering::SeqCst)
    }
    pub fn add(&mut self, key: u64, cost: i64, guard: &Guard) -> (Vec<Item<T>>, bool) {
        let mut evict = self.evict.load(Ordering::SeqCst, guard);
        if evict.is_null() {
            return (vec![], false);
        }
        let evict = unsafe { evict.deref_mut() };

        // can't add an item bigger than entire cache
        if cost > evict.max_cost {
            return (vec![], false);
        }
        // we don't need to go any further if the item is already in the cache
        if evict.update_if_has(key, cost) {
            return (vec![], true);
        }
        let mut room = evict.room_left(cost);
        // if we got this far, this key doesn't exist in the cache
        //
        // calculate the remaining room in the cache (usually bytes)
        if room >= 0 {
            // there's enough room in the cache to store the new item without
            // overflowing, so we can do that now and stop here
            evict.add(key, cost);
            return (vec![], true);
        }
        let mut admit = self.admit.load(Ordering::SeqCst, guard);
        if admit.is_null() {
            return (vec![], false);
        }
        let admit = unsafe { admit.deref_mut() };
        let inc_hits = admit.estimate(key);
        // sample is the eviction candidate pool to be filled via random sampling
        //
        // TODO: perhaps we should use a min heap here. Right now our time
        // complexity is N for finding the min. Min heap should bring it down to
        // O(lg N).

        let mut sample = Vec::new();
        let mut victims = Vec::new();

        while room < 0 {
            room = evict.room_left(cost);
            // fill up empty slots in sample
            evict.fill_sample(&mut sample);
            let mut minKey: u64 = 0;
            let mut minHits: i64 = i64::MAX;
            let mut minId: i64 = 0;
            let mut minCost: i64 = 0;

            for (i, pair) in sample.iter().enumerate() {
                let hits = admit.estimate(pair.key);
                if hits < minHits {
                    minKey = pair.key;
                    minHits = hits;
                    minId = i as i64;
                    minCost = pair.cost;
                }
            }
            if inc_hits < minHits {
                unsafe { self.metrics.as_mut().unwrap().add(rejectSets, key, 1) };
                return (victims, false);
            }
            evict.del(minKey);
            sample[minId as usize] = sample[sample.len() - 1];
            victims.push(Item {
                flag: ITEM_NEW,
                key: minKey,
                conflict: 0,
                value: None,
                cost: minCost,
            })
        };
        evict.add(key, cost);
        (victims, true)
    }

    //TODO lock
    pub fn has(&self, key: u64, guard: &Guard) -> bool {
        let evict = self.evict.load(Ordering::SeqCst, guard);
        if evict.is_null() {
            return false;
        }
        let evict = unsafe { evict.deref() };
        evict.key_costs.contains_key(&key)
    }

    pub fn del(&mut self, key: u64, guard: &Guard) {
        let mut evict = self.evict.load(Ordering::SeqCst, guard);
        if evict.is_null() {
            return;
        }
        let evict = unsafe { evict.deref_mut() };
        evict.del(key);
        return;
    }


    pub fn update(&mut self, key: u64, cost: i64, guard: &Guard) {
        let mut evict = self.evict.load(Ordering::SeqCst, guard);
        if evict.is_null() {
            return;
        }
        let evict = unsafe { evict.deref_mut() };
        evict.update_if_has(key, cost);
        return;
    }

    pub fn clear(&mut self, guard: &Guard) {
        let mut evict = self.evict.load(Ordering::SeqCst, guard);
        if evict.is_null() {
            return;
        }
        let evict = unsafe { evict.deref_mut() };

        let mut admit = self.admit.load(Ordering::SeqCst, guard);
        if admit.is_null() {
            return;
        }
        let admit = unsafe { admit.deref_mut() };
        admit.clear();
        evict.clear();
        return;
    }

    pub fn close(&mut self) {
        self.stop.0.send(true).expect("Chanla close");
    }
    pub fn cost(&mut self, key: u64, cost: i64, guard: &Guard) -> i64 {
        let evict = self.evict.load(Ordering::SeqCst, guard);
        if evict.is_null() {
            return -1;
        }
        let evict = unsafe { evict.deref() };
        match evict.key_costs.get(&key) {
            None => -1,
            Some(v) => *v
        }
    }

    pub fn cap(&self, key: u64, guard: &Guard) -> i64 {
        let mut evict = self.evict.load(Ordering::SeqCst, guard);
        if evict.is_null() {
            return -1;
        }
        let evict = unsafe { evict.deref_mut() };
        evict.max_cost - evict.used
    }

    fn processItems(&mut self, guard: &Guard) {
        loop {
            select! {
               recv(self.item_ch.1) -> item => {
                    if let Ok(item) = item {
                        let mut admit = self.admit.load(Ordering::SeqCst,guard);
                        if admit.is_null() {
                            return;
                        }
                        let admit = unsafe{admit.deref_mut()};
                        admit.push(item)
                    }
               },
                  recv(self.stop.1) -> item => {
                    return;
                }
            }
        }

     /*   let msg = self.item_ch.1.try_recv();
        {
            match msg {
                Ok(r) => {
                    let mut admit = self.admit.load(Ordering::SeqCst, guard);
                    if admit.is_null() {
                        return;
                    }
                    let admit = unsafe { admit.deref_mut() };

                    admit.push(r);
                }
                Err(_) => {}
            }
        }*/
    }
}

pub struct TinyLFU {
    freq: CmSketch,
    door: Bloom,
    incrs: i64,
    reset_at: i64,
}


impl TinyLFU {
    pub fn new(num_counter: i64) -> Self {
        TinyLFU {
            freq: CmSketch::new(num_counter),
            door: Bloom::new(num_counter as f64, 0.01),
            incrs: 0,
            reset_at: num_counter,
        }
    }

    pub fn push(&mut self, keys: Vec<u64>) {
        for (i, key) in keys.iter().enumerate() {
            self.increment(*key)
        }
    }

    pub fn estimate(&mut self, key: u64) -> i64 {
        let mut hits = self.freq.estimate(key);
        if self.door.has(key) {
            hits += 1;
        }
        hits
    }

    pub fn increment(&mut self, key: u64) {
        // flip doorkeeper bit if not already
        if self.door.add_if_not_has(key) {
            // increment count-min counter if doorkeeper bit is already set.
            self.freq.increment(key);
        }
        self.incrs += 1;
        if self.incrs >= self.reset_at {
            self.reset()
        }
    }

    fn clear(&mut self) {
        // Zero out incrs.
        self.incrs = 0;
        // clears doorkeeper bits
        self.door.clear();
        // halves count-min counters
        self.freq.clear();
    }
    fn reset(&mut self) {
        // Zero out incrs.
        self.incrs = 0;
        // clears doorkeeper bits
        self.door.clear();
        // halves count-min counters
        self.freq.clear();
    }
}

pub struct SampledLFU {
    pub key_costs: HashMap<u64, i64>,
    pub max_cost: i64,
    pub used: i64,
    pub metrics: *mut Metrics,
}

impl SampledLFU {
    fn new(max_cost: i64) -> Self {
        SampledLFU {
            key_costs: Default::default(),
            max_cost,
            used: 0,
            metrics: ptr::null_mut(),
        }
    }

    fn room_left(&self, cost: i64) -> i64 {
        self.max_cost - (self.used + cost)
    }

    fn fill_sample(&self, input: &mut Vec<PolicyPair>) {
        if input.len() >= lfuSample {
            return;
        }
        for (key, cost) in self.key_costs.iter() {
            input.push(PolicyPair { key: *key, cost: *cost });
            if input.len() >= lfuSample {
                return;
            }
        }
        return;
    }

    fn del(&mut self, key: u64) {
        match self.key_costs.get(&key) {
            None => {}
            Some(v) => {
                self.used -= v;
                self.key_costs.remove(&key);
            }
        }
    }

    fn add(&mut self, key: u64, cost: i64) {
        self.key_costs.insert(key, cost);
        self.used += cost;
    }
    fn update_if_has(&mut self, key: u64, cost: i64) -> bool {
        match self.key_costs.get(&key) {
            None => false,
            Some(v) => {
                unsafe { self.metrics.as_mut().unwrap().add(keyUpdate, key, 1) }
                self.used += cost - v;
                self.key_costs.insert(key, cost);
                true
            }
        }
    }

    fn clear(&mut self) {
        self.used = 0;
        self.key_costs = HashMap::default();
    }
}

#[derive(Clone, Copy)]
struct PolicyPair {
    key: u64,
    cost: i64,
}


#[cfg(test)]
mod tests {
    use std::{thread, time};
    use crate::Metrics;
    use crate::policy::DefaultPolicy;

    const wait: time::Duration = time::Duration::from_millis(10);

    #[test]
    fn test_policy_metrics() {
        let mut p = DefaultPolicy::<u64>::new(100, 10);
        let guard = crossbeam::epoch::pin();
        p.collect_metrics(&mut Metrics::new(), &guard);
        unsafe { assert_eq!(p.metrics.as_mut().unwrap().all.len(), 256) };
    }

    #[test]
    fn test_policy_push() {
        let mut p = DefaultPolicy::<u64>::new(100, 10);
        assert_eq!(p.push(vec![]), true);

        let mut keepCount = 0;
        for i in 0..10 {
            if p.push(vec![1, 2, 3, 4, 5]) {
                keepCount += 1;
            }
        }
        assert_eq!(keepCount, 0)
    }
}