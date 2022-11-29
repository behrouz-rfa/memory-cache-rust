use std::collections::HashMap;
use std::marker::PhantomData;
use std::ptr;
use std::sync::atomic::{AtomicIsize, Ordering};
use parking_lot::Mutex;
use seize::Guard;
use crate::bloom::bbloom::Bloom;
use crate::cache::{costAdd, dropGets, Item, keepGets, keyUpdate, Metrics, rejectSets};
use crate::cache::ItemFlag::ItemNew;
use crate::cmsketch::CmSketch;
use crate::reclaim::{Atomic, Shared};
use crate::store::Node;

const LFU_SAMPLE: usize = 5;

pub trait Policy {
    fn push(&self, key: [u64]) -> bool;
    // add attempts to add the key-cost pair to the Policy. It returns a slice
    // of evicted keys and a bool denoting whether or not the key-cost pair
    // was added. If it returns true, the key should be stored in cache.
    fn add<T>(&self, key: u64, cost: i64) -> (Vec<Node<T>>, bool);
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

pub struct DefaultPolicy<T> {
    pub(crate) admit: TinyLFU,

    pub(crate) evict: SampledLFU,
    pub(crate) metrics: Atomic<Metrics>,
    pub(crate) flag: AtomicIsize,
    number_counters: i64,
    lock: Mutex<()>,
    max_cost: i64,
    _merker: PhantomData<T>,
}


impl<T> DefaultPolicy<T> {
    pub(crate) fn new(number_counters: i64, max_cost: i64, metrics: Shared<Metrics>) -> Self {
        let mut p = DefaultPolicy {
            admit: TinyLFU::new(number_counters),

            evict: SampledLFU::new(max_cost, metrics),
            metrics: Atomic::from(metrics),
            flag: AtomicIsize::new(0),
            number_counters,
            lock: Default::default(),
            max_cost,
            _merker: PhantomData,
        };
        ;

        p
    }

    pub fn push<'g>(&mut self, keys: Vec<u64>, guard: &'g Guard) -> bool {
        if keys.len() == 0 {
            return true;
        }


        if self.flag.load(Ordering::SeqCst) == 0 {
            self.flag.store(1, Ordering::SeqCst);
            self.process_items(keys.clone(), guard);
            let metrics = self.metrics.load(Ordering::SeqCst, guard);
            if metrics.is_null() {
                unsafe {
                    metrics.deref().add(keepGets, keys[0], keys.len() as u64, guard)
                };
            }
        } else {
            let metrics = self.metrics.load(Ordering::SeqCst, guard);
            if metrics.is_null() {
                unsafe {
                    metrics.deref().add(dropGets, keys[0], keys.len() as u64, guard)
                };
            }
        }

        /*select! {
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
        }*/
        return true;
        // unsafe {
        //     if !self.metrics.is_null() {
        //         self.metrics.as_mut().unwrap().add(keepGets, keys[0], keys.len() as u64)
        //     }
        // };
        // true
    }
    // pub fn collect_metrics(&mut self, metrics: *mut Metrics, guard: &Guard) {
    //     self.metrics = self.metrics;
    //
    //     let mut evict = self.evict.load(Ordering::SeqCst, guard);
    //     if evict.is_null() {
    //         evict = self.init_evict(self.max_cost, &guard)
    //     }
    //
    //
    //
    //     /* let new_table = Owned::new(SampledLFU::new(evict.max_cost));
    //
    //      self.evict.store(new_table, Ordering::SeqCst)*/
    // }
    pub fn add<'g>(&'g mut self, key: u64, cost: i64, guard: &'g Guard<'_>) -> (Vec<Item<T>>, bool) {
        let l = self.lock.lock();


        // can't add an item bigger than entire cache
        if cost > self.evict.max_cost {
            drop(l);
            return (vec![], false);
        }
        // we don't need to go any further if the item is already in the cache
        if self.evict.update_if_has(key, cost, guard) {
            drop(l);
            // An update does not count as an addition, so return false.
            return (vec![], false);
        }
        let mut room = self.evict.room_left(cost);
        // if we got this far, this key doesn't exist in the cache
        //
        // calculate the remaining room in the cache (usually bytes)
        if room >= 0 {
            drop(l);
            // there's enough room in the cache to store the new item without
            // overflowing, so we can do that now and stop here
            self.evict.add(key, cost);
            return (vec![], true);
        }


        let inc_hits = self.admit.estimate(key);
        // sample is the eviction candidate pool to be filled via random sampling
        //
        // TODO: perhaps we should use a min heap here. Right now our time
        // complexity is N for finding the min. Min heap should bring it down to
        // O(lg N).

        let mut sample = Vec::new();
        let mut victims = Vec::new();
        room = self.evict.room_left(cost);
        while room < 0 {
            room = self.evict.room_left(cost);
            // fill up empty slots in sample
            self.evict.fill_sample(&mut sample);
            let mut min_key: u64 = 0;
            let mut min_hits: i64 = i64::MAX;
            let mut min_id: i64 = 0;
            let mut min_cost: i64 = 0;


            for i in 0..sample.len() {
                let hits = self.admit.estimate(sample[i].key);
                if hits < min_hits {
                    min_key = sample[i].key;
                    min_hits = hits;
                    min_id = i as i64;
                    min_cost = sample[i].cost;
                }
            }
            if inc_hits < min_hits {
                unsafe {
                    let metrics = self.metrics.load(Ordering::SeqCst, guard);
                    if metrics.is_null() {
                        unsafe {
                            metrics.deref().add(rejectSets, key, 1, guard)
                        };
                    }
                }
                return (victims, false);
            }
            self.evict.del(&min_key);
            sample[min_id as usize] = sample[sample.len() - 1];
            victims.push(Item {
                flag: ItemNew,
                key: min_key,
                conflict: 0,
                value: Atomic::null(),
                cost: min_cost,
                expiration: None,
            })
        };
        self.evict.add(key, cost);
        drop(l);
        return (victims, true);
    }

    //TODO lock
    pub fn has(&self, key: u64, guard: &Guard) -> bool {
        self.evict.key_costs.contains_key(&key)
    }

    pub fn del<'g>(&'g mut self, key: &u64, guard: &'g Guard) {
        self.evict.del(key);
    }


    pub fn update<'g>(&'g mut self, key: u64, cost: i64, guard: &'g Guard) {
        self.evict.update_if_has(key, cost, guard);
    }

    pub fn clear<'g>(&'g mut self, guard: &'g Guard) {
        self.admit.clear();
        self.evict.clear();
    }

    pub fn close(&mut self) {
        //self.stop.0.send(true).expect("Chanla close");
    }
    pub fn cost(&self, key: &u64, guard: &Guard) -> i64 {
        match self.evict.key_costs.get(&key) {
            None => -1,
            Some(v) => *v
        }
    }

    pub fn cap(&self) -> i64 {
        self.evict.max_cost - self.evict.used
    }

    fn process_items<'g>(&'g mut self, item: Vec<u64>, guard: &'g Guard) {
        self.admit.push(item);
        self.flag.store(0, Ordering::SeqCst)
        /*        loop {
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
                }*/

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
    pub freq: CmSketch,
    pub door: Bloom,
    pub incrs: i64,
    pub reset_at: i64,
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
    pub(crate) metrics: Atomic<Metrics>,
}

impl SampledLFU {
    fn new(max_cost: i64, shared: Shared<Metrics>) -> Self {
        SampledLFU {
            key_costs: HashMap::new(),
            max_cost,
            used: 0,
            metrics: Atomic::from(shared),
        }
    }

    fn room_left(&self, cost: i64) -> i64 {
        self.max_cost - (self.used + cost)
    }

    fn fill_sample(&self, input: &mut Vec<PolicyPair>) {
        if input.len() >= LFU_SAMPLE {
            return;
        }
        for (key, cost) in self.key_costs.iter() {
            input.push(PolicyPair { key: *key, cost: *cost });
            if input.len() >= LFU_SAMPLE {
                return;
            }
        }
        return;
    }

    fn del(&mut self, key: &u64) {
        match self.key_costs.get(key) {
            None => {}
            Some(v) => {
                self.used -= v;
                self.key_costs.remove(key);
            }
        }
    }

    fn add(&mut self, key: u64, cost: i64) {
        //eprintln!("{}", cost);
        self.key_costs.insert(key, cost);
        self.used += cost;
    }
    fn update_if_has(&mut self, key: u64, cost: i64, guard: &Guard) -> bool {
        match self.key_costs.get(&key) {
            None => false,
            Some(v) => {
                let metrics = self.metrics.load(Ordering::SeqCst, guard);
                unsafe {
                    if !metrics.is_null() {
                        metrics.deref().add(keyUpdate, key, 1, guard)
                    }
                }
                if metrics.is_null() {
                    panic!("metric is null")
                }
                if *v > cost {
                    let diff = *v - cost;
                    unsafe { metrics.deref().add(costAdd, key, (diff - 1) as u64, guard) }
                } else if cost > *v {
                    let diff = *v - cost;
                    unsafe { metrics.deref().add(costAdd, key, diff as u64, guard) }
                }
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
    use std::sync::atomic::Ordering;
    use seize::Collector;
    use crate::cache::{doNotUse, Metrics};
    use crate::policy::{DefaultPolicy, SampledLFU};
    use crate::reclaim::{Atomic, Shared};

    #[test]
    fn test_policy_policy_push() {
        let metrics: Atomic<Metrics> = Atomic::null();
        let collector = Collector::new();
        let table = Shared::boxed(Metrics::new(doNotUse, &collector), &collector);
        metrics.store(table, Ordering::SeqCst);

        let guard = collector.enter();
        let shard_metric = metrics.load(Ordering::SeqCst, &guard);
        let mut p = DefaultPolicy::<i32>::new(100, 10, shard_metric);
        let mut keep_count = 0;
        for i in 0..10 {
            if p.push(vec![1, 2, 3, 4, 5], &guard) {
                keep_count += 1;
            }
        }
        assert_ne!(keep_count, 0)
    }

    #[test]
    fn test_policy_policy_add() {
        let metrics: Atomic<Metrics> = Atomic::null();
        let collector = Collector::new();
        let table = Shared::boxed(Metrics::new(doNotUse, &collector), &collector);
        metrics.store(table, Ordering::SeqCst);

        let guard = collector.enter();
        let shard_metric = metrics.load(Ordering::SeqCst, &guard);
        let mut p = DefaultPolicy::<i32>::new(1000, 100, shard_metric);
        let v = p.add(1, 101, &guard);
        assert!(v.0.len() == 0 || v.1, "can't add an item bigger than entire cache");

        p.add(1, 1, &guard);
        p.admit.increment(1);
        p.admit.increment(2);
        p.admit.increment(3);

        let v = p.add(1, 1, &guard);
        assert_eq!(v.0.len(), 0);
        assert_eq!(v.1, false);

        let v = p.add(2, 20, &guard);
        assert_eq!(v.0.len(), 0);
        assert_eq!(v.1, true);

        let v = p.add(3, 90, &guard);
        assert!(v.0.len()>0);
        assert_eq!(v.1, true);

        let v = p.add(4, 20, &guard);
        assert_eq!(v.0.len(), 0);
        assert_eq!(v.1, false);
    }


    #[test]
    fn test_policy_del() {
        let metrics: Atomic<Metrics> = Atomic::null();
        let collector = Collector::new();
        let table = Shared::boxed(Metrics::new(doNotUse, &collector), &collector);
        metrics.store(table, Ordering::SeqCst);

        let guard = collector.enter();
        let shard_metric = metrics.load(Ordering::SeqCst, &guard);
        let mut p = DefaultPolicy::<i32>::new(1000, 100, shard_metric);

        p.add(1, 1, &guard);
        p.del(&1,&guard);
        p.del(&2,&guard);

        assert_eq!(p.has(1,&guard),false);
        assert_eq!(p.has(2,&guard),false);
    }
    #[test]
    fn test_policy_cap() {
        let metrics: Atomic<Metrics> = Atomic::null();
        let collector = Collector::new();
        let table = Shared::boxed(Metrics::new(doNotUse, &collector), &collector);
        metrics.store(table, Ordering::SeqCst);

        let guard = collector.enter();
        let shard_metric = metrics.load(Ordering::SeqCst, &guard);
        let mut p = DefaultPolicy::<i32>::new(100, 10, shard_metric);

        p.add(1, 1, &guard);

        assert_eq!(p.cap(),9);

    }


    #[test]
    fn test_policy_update() {
        let metrics: Atomic<Metrics> = Atomic::null();
        let collector = Collector::new();
        let table = Shared::boxed(Metrics::new(doNotUse, &collector), &collector);
        metrics.store(table, Ordering::SeqCst);

        let guard = collector.enter();
        let shard_metric = metrics.load(Ordering::SeqCst, &guard);
        let mut p = DefaultPolicy::<i32>::new(100, 10, shard_metric);

        p.add(1, 1, &guard);
        p.add(1, 2, &guard);

        assert_eq!(p.evict.key_costs.get(&1),Some(&2));

    }

    #[test]
    fn test_policy_cost() {
        let metrics: Atomic<Metrics> = Atomic::null();
        let collector = Collector::new();
        let table = Shared::boxed(Metrics::new(doNotUse, &collector), &collector);
        metrics.store(table, Ordering::SeqCst);

        let guard = collector.enter();
        let shard_metric = metrics.load(Ordering::SeqCst, &guard);
        let mut p = DefaultPolicy::<i32>::new(100, 10, shard_metric);

        p.add(1, 1, &guard);


        assert_eq!(p.cost(&1,&guard),1);
        assert_eq!(p.cost(&2,&guard),-1);

    }


    #[test]
    fn test_policy_clear() {
        let metrics: Atomic<Metrics> = Atomic::null();
        let collector = Collector::new();
        let table = Shared::boxed(Metrics::new(doNotUse, &collector), &collector);
        metrics.store(table, Ordering::SeqCst);

        let guard = collector.enter();
        let shard_metric = metrics.load(Ordering::SeqCst, &guard);
        let mut p = DefaultPolicy::<i32>::new(100, 10, shard_metric);

        p.add(1, 1, &guard);
        p.add(2, 2, &guard);
        p.add(3, 3, &guard);
        p.clear(&guard);


        assert_eq!(p.has(1,&guard),false);
        assert_eq!(p.has(2,&guard),false);
        assert_eq!(p.has(2,&guard),false);


    }
    #[test]
    fn test_lfu_add(){

        let metrics: Atomic<Metrics> = Atomic::null();
        let collector = Collector::new();
        let table = Shared::boxed(Metrics::new(doNotUse, &collector), &collector);
        metrics.store(table, Ordering::SeqCst);
        let guard = collector.enter();
        let shard_metric = metrics.load(Ordering::SeqCst, &guard);

        let mut lfu = SampledLFU::new(4,shard_metric);
        lfu.add(1, 1);
        lfu.add(2, 2);
        lfu.add(3, 1);
        assert_eq!(lfu.used,4);
        assert_eq!(lfu.key_costs.get(&2),Some(&2));
    }

    #[test]
    fn test_lfu_del(){

        let metrics: Atomic<Metrics> = Atomic::null();
        let collector = Collector::new();
        let table = Shared::boxed(Metrics::new(doNotUse, &collector), &collector);
        metrics.store(table, Ordering::SeqCst);
        let guard = collector.enter();
        let shard_metric = metrics.load(Ordering::SeqCst, &guard);

        let mut lfu = SampledLFU::new(4,shard_metric);
        lfu.add(1, 1);
        lfu.add(2, 2);
        lfu.del(&2);
        assert_eq!(lfu.used,1);
        assert_eq!(lfu.key_costs.get(&2),None);
    }


    #[test]
    fn test_lfu_update(){

        let metrics: Atomic<Metrics> = Atomic::null();
        let collector = Collector::new();
        let table = Shared::boxed(Metrics::new(doNotUse, &collector), &collector);
        metrics.store(table, Ordering::SeqCst);
        let guard = collector.enter();
        let shard_metric = metrics.load(Ordering::SeqCst, &guard);

        let mut lfu = SampledLFU::new(4,shard_metric);
        lfu.add(1, 1);

        assert_eq!( lfu.update_if_has(1,2,&guard),true);
        assert_eq!(lfu.used,2);
        assert_eq!( lfu.update_if_has(2,2,&guard),false);
    }

    #[test]
    fn test_lfu_clear(){

        let metrics: Atomic<Metrics> = Atomic::null();
        let collector = Collector::new();
        let table = Shared::boxed(Metrics::new(doNotUse, &collector), &collector);
        metrics.store(table, Ordering::SeqCst);
        let guard = collector.enter();
        let shard_metric = metrics.load(Ordering::SeqCst, &guard);

        let mut lfu = SampledLFU::new(4,shard_metric);
        lfu.add(1, 1);
        lfu.add(2, 2);
        lfu.add(3, 3);
        lfu.clear();

        assert_eq!(lfu.used,0);
        assert_eq!(lfu.key_costs.len(),0);

    }
}