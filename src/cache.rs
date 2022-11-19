use std::sync::atomic::Ordering;
use std::thread;
use crossbeam::epoch::{Atomic, Guard};
use crossbeam_channel::{Receiver, Sender, select};
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


///Cache is a thread-safe implementation of a hashmap with a TinyLFU admission
/// policy and a Sampled LFU eviction policy. You can use the same Cache instance
/// from as many goroutines as you want.
#[derive(Clone)]
pub struct Cache<K, V> {
    stor: ShardedMap<V>,
    policy: DefaultPolicy<V>,
    get_buf: RingBuffer<V>,
    set_buf: Sender<Item<V>>,
    receiver_buf: Receiver<Item<V>>,
    stop_sender: Sender<bool>,
    stop: Receiver<bool>,
    metrics: Option<Metrics>,
    key_to_hash: fn(K) -> (u64, u64),
    on_evict: Option<fn(u64, u64, V, i64)>,
    cost: Option<fn(V) -> (i64)>,
}


/// Config is passed to NewCache for creating new Cache instances.
pub struct Config<K, V> {
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

    key_to_hash: fn(K) -> (u64, u64),

    on_evict: Option<fn(u64, u64, V, i64)>,
    cost: Option<fn(V) -> i64>,
}

/// Config is passed to NewCache for creating new Cache instances.
impl<K, V> Config<K, V> {
    // OnEvict is called for every eviction and passes the hashed key, value,
    // and cost to the function.
    pub fn cost(value: V) -> i64 {
        todo!()
    }
}

pub type ItemFlag = u8;

pub const ITEM_NEW: ItemFlag = 0;
pub const ITEM_DELETE: ItemFlag = 1;
pub const ITEM_UPDATE: ItemFlag = 2;

/// item is passed to setBuf so items can eventually be added to the
/// cache
#[derive(Debug, Copy, Clone)]
pub struct Item<T> {
    pub(crate) flag: ItemFlag,
    pub(crate) key: u64,
    pub(crate) conflict: u64,
    pub(crate) value: Option<T>,
    pub(crate) cost: i64,
}


#[derive(Clone)]
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


unsafe impl<K: Send, V: Send> Send for Cache<K, V> {}

unsafe impl<K: Sync, V: Sync> Sync for Cache<K, V> {}


impl<K, V> Cache<K, V>
    where V: Clone + Send + 'static,
          K: Clone + Send + 'static
{
    /// NewCache returns a new Cache instance and any
    /// configuration errors, if any.
    pub fn new(c: Config<K, V>) -> Self {
        let mut p = DefaultPolicy::new(c.numb_counters, c.max_cost);
        let (tx, rx) = crossbeam_channel::unbounded();
        let (stop_tx, stop_rx) = crossbeam_channel::unbounded();
        let bf = RingBuffer::new(&mut p, c.buffer_items);
        let mut cache = Cache {
            stor: ShardedMap::new(),
            get_buf: bf,
            policy: p,
            set_buf: tx,
            receiver_buf: rx,
            stop_sender: stop_tx,
            stop: stop_rx,
            metrics: Default::default(),
            key_to_hash: c.key_to_hash,
            on_evict: c.on_evict,
            cost: c.cost,
        };


        let guard = crossbeam::epoch::pin();
        if c.metrics {
            cache.collect_metrics(&guard)
        }
        let mut c = cache.clone();

        // NOTE: benchmarks seem to show that performance decreases the more
        //goroutines we have running cache.processItems(), so 1 should
        // usually be sufficient
        thread::spawn(move || {
            let guard = crossbeam::epoch::pin();
            c.process_items(&guard)
        });
        cache
    }

    fn collect_metrics(&mut self, x: &Guard) {
        self.metrics = Some(Metrics::new());
        if let Some(ref mut m) = self.metrics {
            self.policy.collect_metrics(m, x);
        }
    }
}

impl<K:Send, V: Clone + Send> Cache<K, V>
{
    /// Set attempts to add the key-value item to the cache. If it returns false,
    /// then the Set was dropped and the key-value item isn't added to the cache. If
    /// it returns true, there's still a chance it could be dropped by the policy if
    /// its determined that the key-value item isn't worth keeping, but otherwise the
    /// item will be added and other items will be evicted in order to make room.
    ///
    /// To dynamically evaluate the items cost using the Config.Coster function, set
    /// the cost parameter to 0 and Coster will be ran when needed in order to find
    /// the items true cost.
    fn set(&mut self, key: K, value: V, cost: i64, guard: &Guard) -> bool {
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
        if self.stor.update(key_hash, confilict_hash, value.clone(), guard) {
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
    /// Del deletes the key-value item from the cache if it exists.
    fn del(&mut self, key: K) {
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

    /// Get returns the value (if any) and a boolean representing whether the
    /// value was found or not. The value can be nil and the boolean can be true at
    /// the same time.
    fn get<'a>(&mut self, key: K, guard: &'a Guard) -> Option<&'a V> {
        let (key_hash, confilict_hash) = (self.key_to_hash)(key);
        self.get_buf.push(key_hash);
        let result = self.stor.Get(key_hash, confilict_hash, guard);

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
    /// Close stops all goroutines and closes all channels.
    fn close(&mut self, guard: &Guard) {
        // block until processItems  is returned
        self.stop_sender.try_send(true);
        self.policy.close()
    }
    /// Clear empties the hashmap and zeroes all policy counters. Note that this is
    /// not an atomic operation (but that shouldn't be a problem as it's assumed that
    /// Set/Get calls won't be occurring until after this).
    fn clear(&mut self, guard: &Guard) {
        // block until processItems  is returned
        self.stop_sender.try_send(true);
        let guard = crossbeam::epoch::pin();
        self.policy.clear(&guard);
        self.stor.clear(&guard);
        if let Some(ref mut metrics) = self.metrics {
            metrics.clear();
        }
        let (tx, rx) = crossbeam_channel::unbounded();
        self.set_buf = tx;
        self.receiver_buf = rx;

        //TODO fix thead after clear
       /* thread::spawn( || {
            let guard = crossbeam::epoch::pin();
            self.process_items(&guard);
        });*/
    }

    fn process_items(&mut self, guard: &Guard) {
        loop {
            select! {
               recv(self.receiver_buf) -> item => {
                    match item {
                        Ok(mut i)=> {
                                //todo cost fn
                            if i.cost ==0 && self.cost.is_some() && i.flag == ITEM_DELETE {
                                    i.cost = (self.cost.unwrap())(i.value.clone().unwrap())
                            }
                            match i.flag {
                                ITEM_NEW=>{
                                    let  (mut victims, added ) = self.policy.add(i.key,i.conflict as i64,guard);
                                    if added {
                                        self.stor.Set(i.key,i.conflict,i.value.unwrap(),guard);
                                        if let Some(ref mut metrics) = self.metrics {
                                            metrics.add(keyAdd,i.key,1);
                                            metrics.add(costAdd,i.key,i.cost as u64);
                                        }
                                    }
                                    for i in 0..victims.len() {
                                        let mut delVal = self.stor.Del(victims[i].key,0,guard);
                                        match delVal {
                                            Some((mut c,mut v))=>{
                                                 victims[i].value = Some(v.clone());
                                                victims[i].conflict = c;
                                               if self.on_evict.is_some() {
                                                    (self.on_evict.unwrap())(victims[i].key,victims[i].conflict,victims[i].value.clone().unwrap(),victims[i].cost)
                                                }

                                                if let Some(ref mut metrics) = self.metrics {
                                                    metrics.add(keyEvict,victims[i].key,1);
                                                    metrics.add(costEvict,victims[i].key,victims[i].cost as u64);
                                                }
                                                    }
                                            None=>{continue}
                                            }

                                    }


                                },
                                ITEM_UPDATE=>{
                                    self.policy.update(i.key,i.cost,guard)
                                },
                                ITEM_DELETE=>{
                                    self.policy.del(i.key,guard);
                                    self.stor.Del(i.key,i.conflict,guard);
                                }
                                _=>{continue}

                            }

                        },
                        Err(_)=>continue,
                    }
                },
                recv(self.stop) ->item=> {return;}
           }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::thread;
    use std::time::Duration;
    use crate::bloom::z;
    use crate::bloom::z::{key_to_hash, KeyHash, value_to_int};
    use crate::cache::{Cache, Config};

    #[test]
    fn TestCacheKeyToHash() {
        let mut key_to_hash_count = 0;
        let mut cache = Cache::new(
            Config {
                numb_counters: 100,
                max_cost: 10,
                buffer_items: 64,
                metrics: false,
                key_to_hash: key_to_hash,
                on_evict: None,
                cost: None,
            }
        );

        let guard = crossbeam::epoch::pin();
        if cache.set("key", "value", 1, &guard) {
            thread::sleep(Duration::from_millis(2000));
            println!("{:?}", cache.get("key", &guard));
        }
        // cache.set(1, 1, 1, &guard);
    }
}