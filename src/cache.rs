use crossbeam_channel::{Receiver, select, Sender};
use crate::bloom::z::KeyHash;
use crate::policy::{DefaultPolicy, Policy};
use crate::ring::{RingBuffer, RingConsumer};
use crate::store::{ShardedMap, Store};

// The following 2 keep track of hits and misses.
pub const hit: i32 = 0;
pub const miss: i32 = 1;
// The following 3 keep track of number of keys added, updated and evicted.
const keyAdd: i32 = 2;
pub const keyUpdate: i32 = 3;
pub const keyEvict: i32 = 4;
// The following 2 keep track of cost of keys added and evicted.
pub const costAdd: i32 = 5;
pub const costEvict: i32 = 6;
// The following keep track of how many sets were dropped or rejected later.

pub const dropSets: i32 = 7;
pub const rejectSets: i32 = 8;
// The following 2 keep track of how many gets were kept and dropped on the
// floor.
pub const dropGets: i32 = 9;
pub const keepGets: i32 = 10;
// This should be the final enum. Other enums should be set before this.
pub const doNotUse: i32 = 11;


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

#[derive(Default, Clone)]
pub struct Metrics {
    pub all: Vec<u64>,
}

type MetricType = i32;

impl Metrics {
    pub fn new() -> Metrics {
        let mut m = Metrics::default();
        for i in 0..doNotUse {
            m.all = vec![0u64; 256];
            //ToDO
            //  for j := range slice {
            //      slice[j] = new(uint64)
            //  }
        }
        m
    }
    pub fn add(&mut self, t: MetricType, hash: u64, delta: i64) {
        todo!()
    }
}


pub struct Cache<T> {
    stor: ShardedMap<T>,
    policy: DefaultPolicy,
    getBuf: RingBuffer,
    set_buf: Sender<Item<T>>,
    receiver_buf: Receiver<Item<T>>,
    metrics: Metrics,
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
        self.metrics = Metrics::new();
        self.policy.collect_metrics(&mut self.metrics);
    }
}

impl<T:Clone> Cache<T>
{
    fn set(&mut self, key: T, value: T, cost: i64) -> bool {
        let (key_hash, confilict_hash) = (self.key_to_hash)(key);
        let mut item = Item {
            flag: ITEM_NEW,
            key: key_hash,
            conflict: confilict_hash,
            value:Some(value.clone()),
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
               self.metrics.add(dropSets, key_hash, 1);
                false
            },
        }
    }
    fn get(&mut self, key: T) -> Option<&T> {
        let (key_hash, confilict_hash) = (self.key_to_hash)(key);
        self.getBuf.push(key_hash);
        let result = self.stor.Get(key_hash, confilict_hash);

        return match result {
            None => {
                self.metrics.add(hit, key_hash, 1);
                None
            },
            Some(v) => {
                self.metrics.add(miss, key_hash, 1);
                Some(v)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn test() {}
}