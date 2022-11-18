use std::sync::atomic::Ordering;
use crossbeam::epoch::Atomic;
use crossbeam_channel::{Receiver, select, Sender};
use serde_json::Value::String;
use crate::bloom::z::KeyHash;
use crate::policy::{DefaultPolicy, Policy};
use crate::ring::{RingBuffer, RingConsumer};
use crate::store::{ShardedMap, Store};

// The following 2 keep track of hits and misses.
pub const hit: MetricType = 0;
pub const miss: MetricType = 1;
// The following 3 keep track of number of keys added, updated and evicted.
const keyAdd: MetricType = 2;
pub const keyUpdate: MetricType = 3;
pub const keyEvict: MetricType = 4;
// The following 2 keep track of cost of keys added and evicted.
pub const costAdd: MetricType = 5;
pub const costEvict: MetricType = 6;
// The following keep track of how many sets were dropped or rejected later.

pub const dropSets: MetricType = 7;
pub const rejectSets: MetricType = 8;
// The following 2 keep track of how many gets were kept and dropped on the
// floor.
pub const dropGets: MetricType = 9;
pub const keepGets: MetricType = 10;
// This should be the final enum. Other enums should be set before this.
pub const doNotUse: MetricType = 11;


/// Config is passed to NewCache for creating new Cache instances.
pub struct Config<T> {
    // NumCounters determines the number of counters (keys) to keep that hold
    // access frequency information. It's generally a good idea to have more
    // counters than the max cache capacity, as this will improve eviction
    // accuracy and subsequent hit ratios.
    //
    // For example, if you expect your cache to hold 1,000,000 items when full,
    // NumCounters should be 10,000,000 (10x). Each counter takes up 4 bits, so
    // keeping 10,000,000 counters would require 5MB of memory.

    numb_counters: i64,
    // max_cost can be considered as the cache capacity, in whatever units you
    // choose to use.
    //
    // For example, if you want the cache to have a max capacity of 100MB, you
    // would set MaxCost to 100,000,000 and pass an item's number of bytes as
    // the `cost` parameter for calls to Set. If new items are accepted, the
    // eviction process will take care of making room for the new item and not
    // overflowing the MaxCost value.
    max_cost: i64,

    // buffer_items determines the size of Get buffers.
    //
    // Unless you have a rare use case, using `64` as the buffer_items value
    // results in good performance.
    buffer_items: usize,
    // metrics determines whether cache statistics are kept during the cache's
    // lifetime. There *is* some overhead to keeping statistics, so you should
    // only set this flag to true when testing or throughput performance isn't a
    // major factor.
    metrics: bool,

    key_to_hash: fn(T) -> (u64, u64),
}


impl<T> Config<T> {
    // OnEvict is called for every eviction and passes the hashed key, value,
    // and cost to the function.
    pub fn on_evict(key: u64, confilict: u64, value: T, cost: usize) {
        todo!()
    }


    pub fn cost(value: T) -> i64 {
        todo!()
    }
}

pub type ItemFlag = u8;

pub const ITEM_NEW: ItemFlag = 0;
pub const ITEM_DELETE: ItemFlag = 1;
pub const ITEM_UPDATE: ItemFlag = 2;

#[derive(Debug, Copy, Clone)]
pub struct Item<T> {
    pub(crate) flag: ItemFlag,
    pub(crate) key: u64,
    pub(crate) conflict: u64,
    pub(crate) value: Option<T>,
    pub(crate) cost: i64,
}


pub struct Metrics {
    pub all: [[u64; 256]; doNotUse],
}

type MetricType = usize;

impl Metrics {
    pub fn new() -> Metrics {
        Metrics {
            all: [[0u64; 256]; doNotUse]
        }
        // for i in 0..doNotUse {
        //     m.all[i] = [0;256]
        // }
        // m
    }
    //TODO fix atomic
    pub fn get(&self, t: MetricType) -> u64 {
        let mut total = 0;

        let valp = self.all[t];
        for i in 0..valp.len() {
            total += valp[i];
        }
        // let gaurd = crossbeam::epoch::pin();
        //
        // for i in 0..self.all.len() {
        //     let s = self.all[i].load(Ordering::SeqCst, &gaurd);
        //     if s.is_null() {
        //         continue;
        //     }
        //
        //     total += unsafe { s.as_ref().unwrap() }
        // }
        total
    }
    pub fn add(&mut self, t: MetricType, hash: u64, delta: u64) {
        let idx = (hash % 5) * 10;
        self.all[t][idx as usize] = delta;
    }
    // Hits is the number of Get calls where a value was found for the corresponding
// key.
    pub fn hits(&self) -> u64 {
        self.get(hit)
    }
    // Misses is the number of Get calls where a value was not found for the
// corresponding key.
    pub fn Misses(&self) -> u64 {
        self.get(miss)
    }

    pub fn KeysAdded(&self) -> u64 {
        self.get(keyAdd)
    }
    pub fn KeysUpdated(&self) -> u64 {
        self.get(keyUpdate)
    }
    pub fn KeysEvicted(&self) -> u64 {
        self.get(keyEvict)
    }
    pub fn CostAdded(&self) -> u64 {
        self.get(costAdd)
    }
    pub fn CostEvicted(&self) -> u64 {
        self.get(costEvict)
    }
    pub fn SetsDropped(&self) -> u64 {
        self.get(dropSets)
    }

    pub fn SetsRejected(&self) -> u64 {
        self.get(rejectSets)
    }

    pub fn GetsDropped(&self) -> u64 {
        self.get(dropGets)
    }
    pub fn GetsKept(&self) -> u64 {
        self.get(keepGets)
    }
    pub fn ratio(&self) -> f64 {
        let hits = self.get(hit);
        let misses = self.get(miss);
        if hits == 0 && misses == 0 {
            return 0.0;
        }
        (hits / (misses + hits)) as f64
    }

    pub fn clear(&mut self) {
        self.all = [[0u64; 256]; doNotUse]
    }

    pub fn string(&self) -> std::string::String {
        let mut values = "".to_owned();
        for i in 0..doNotUse {
            values.push_str(&format!("{}: {} ", self.stringFor(i), self.get(i)));
        }

        values.push_str(&format!("gets-total: {} ", self.get(hit) + self.get(miss)));
        values.push_str(&format!("gets-total: {:#02} ", self.ratio()));

        values
    }

    pub fn stringFor(&self, t: MetricType) -> &str {
        match t {
            hit => "hit",
            miss => "miss",
            keyAdd => "keys-added",
            keyUpdate => "keys-updated",
            keyEvict => "keys-evicted",
            costAdd => "cost-added",
            costEvict => "cost-evicted",
            dropSets => "sets-dropped",
            rejectSets => "sets-rejected",
            dropGets => "gets-dropped",
            keepGets => "gets-kept",
            _ => { "unidentified" }
        }
    }
}


pub struct Cache<T> {
    stor: ShardedMap<T>,
    policy: DefaultPolicy,
    getBuf: RingBuffer,
    set_buf: Sender<Item<T>>,
    receiver_buf: Receiver<Item<T>>,
    metrics: Option<Metrics>,
    key_to_hash: fn(T) -> (u64, u64),
}


impl<T> Cache<T> {
    pub fn new(c: Config<T>) -> Self {
        let mut p = DefaultPolicy::new(c.numb_counters, c.max_cost);
        let (tx, rx) = crossbeam_channel::unbounded();
        let mut cache = Cache {
            stor: ShardedMap::new(),
            getBuf: RingBuffer::new(&mut p, c.buffer_items),
            policy: p,
            set_buf: tx,
            receiver_buf: rx,
            metrics: Default::default(),
            key_to_hash: c.key_to_hash,
        };


        if c.metrics {
            cache.collect_metrics()
        }
        cache
    }

    fn collect_metrics(&mut self) {
        self.metrics = Some(Metrics::new());
        if let Some(ref mut m) = self.metrics {
            self.policy.collect_metrics(m);
        }
    }
}

impl<T: Clone> Cache<T>
{
    fn set(&mut self, key: T, value: T, cost: i64) -> bool {
        let (key_hash, confilict_hash) = (self.key_to_hash)(key);
        let mut item = Item {
            flag: ITEM_NEW,
            key: key_hash,
            conflict: confilict_hash,
            value: Some(value.clone()),
            cost,
        };
        // attempt to immediately update hashmap value and set flag to update so the
        // cost is eventually updated
        if (self.stor.update(key_hash, confilict_hash, value.clone())) {
            item.flag = ITEM_UPDATE;
        }
        select! {
            send(self.set_buf, item)->res => true,
            default => {
                if let Some(ref mut m) = self.metrics {
                 m.add(dropSets, key_hash, 1);
              }


                false
            },
        }
    }
    fn del(&mut self, key: T) {
        let (key_hash, confilict_hash) = (self.key_to_hash)(key);
        let item = Item {
            flag: ITEM_DELETE,
            key: key_hash,
            conflict: confilict_hash,
            value: None,
            cost: 0,
        };

        self.set_buf.send(item);
    }
    fn get(&mut self, key: T) -> Option<&T> {
        let (key_hash, confilict_hash) = (self.key_to_hash)(key);
        self.getBuf.push(key_hash);
        let result = self.stor.Get(key_hash, confilict_hash);

        return match result {
            None => {
                if let Some(ref mut m) = self.metrics {
                    m.add(hit, key_hash, 1);
                }

                None
            }
            Some(v) => {
                if let Some(ref mut m) = self.metrics {
                    m.add(miss, key_hash, 1);
                }

                Some(v)
            }
        };
    }
}

#[cfg(test)]
mod tests {
    use crate::cache::Cache;

    #[test]
    fn TestCacheKeyToHash() {
        // let mut key_to_hash_count = 0;
        // let mut cache = Cache::new()
    }
}