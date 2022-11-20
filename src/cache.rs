use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::{ptr, thread};

use crossbeam_channel::{Receiver, Sender, select};
use seize::{Collector, Guard};
use serde_json::Value::String;
use crate::bloom::hasher::KeyHash;
use crate::policy::{DefaultPolicy, Policy};
use crate::reclaim::Atomic;
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
pub struct Cache< K, V: Clone> {
    pub store: ShardedMap<V>,
    pub policy: DefaultPolicy<V>,
    get_buf: RingBuffer< V>,
    pub set_buf: Sender<Item<V>>,
    receiver_buf: Receiver<Item<V>>,
    stop_sender: Sender<bool>,
    stop: Receiver<bool>,
    metrics: *mut Metrics,
    key_to_hash: fn(&K) -> (u64, u64),
    on_evict: Option<fn(u64, u64, V, i64)>,
    cost: Option<fn(V) -> (i64)>,

    /// Collector that all `Guard` references used for operations on this map must be tied to. It
    /// is important that they all assocate with the _same_ `Collector`, otherwise you end up with
    /// unsoundness as described in https://github.com/jonhoo/flurry/issues/46. Specifically, a
    /// user can do:
    ///
    ///
    ///
    /// We avoid that by checking that every external guard that is passed in is associated with
    /// the `Collector` that was specified when the map was created (which may be the global
    /// collector).
    collector: Collector,
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

    pub numb_counters: i64,
    // max_cost can be considered as the cache capacity, in whatever units you
    // choose to use.
    //
    // For example, if you want the cache to have a max capacity of 100MB, you
    // would set MaxCost to 100,000,000 and pass an item's number of bytes as
    // the `cost` parameter for calls to Set. If new items are accepted, the
    // eviction process will take care of making room for the new item and not
    // overflowing the MaxCost value.
    pub max_cost: i64,

    // buffer_items determines the size of Get buffers.
    //
    // Unless you have a rare use case, using `64` as the buffer_items value
    // results in good performance.
    pub buffer_items: usize,
    // metrics determines whether cache statistics are kept during the cache's
    // lifetime. There *is* some overhead to keeping statistics, so you should
    // only set this flag to true when testing or throughput performance isn't a
    // major factor.
    pub metrics: bool,

    pub key_to_hash: fn(&K) -> (u64, u64),

    pub on_evict: Option<fn(u64, u64, V, i64)>,
    pub cost: Option<fn(V) -> i64>,
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
    pub flag: ItemFlag,
    pub key: u64,
    pub conflict: u64,
    pub value: Option<T>,
    pub cost: i64,
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


unsafe impl<K: Send, V: Send+Clone> Send for Cache< K, V> {}

unsafe impl<K: Sync, V: Sync+Clone> Sync for Cache<K, V> {}


impl<K, V> Cache<K, V>
    where V: Clone + Send + Sync + 'static,
          K: Clone + Send + Sync + 'static
{
    /// NewCache returns a new Cache instance and any
    /// configuration errors, if any.
    pub fn new(c: Config<K, V>) -> Self {
        let mut p = DefaultPolicy::new(c.numb_counters, c.max_cost);
        let (tx, rx) = crossbeam_channel::unbounded();
        let (stop_tx, stop_rx) = crossbeam_channel::unbounded();
    /*    let binding = Collector::new();
        let binding = binding.enter();*/
       let bf = RingBuffer::new(&mut p, c.buffer_items);
        let mut cache = Cache {
            store: ShardedMap::new(),
            get_buf: bf,
            policy: p,
            set_buf: tx,
            receiver_buf: rx,
            stop_sender: stop_tx,
            stop: stop_rx,
            metrics: ptr::null_mut(),
            key_to_hash: c.key_to_hash,
            on_evict: c.on_evict,
            cost: c.cost,
            collector:  Collector::new(),
        };
        cache


        // if c.metrics {
        //  //   cache.collect_metrics(&binding);
        // }
        //
        //
        // cache
    }
    /// Pin a `Guard` for use with this map.
    ///
    /// Keep in mind that for as long as you hold onto this `Guard`, you are preventing the
    /// collection of garbage generated by the map.
    pub fn guard(&self) -> Guard<'_> {
        self.collector.enter()
    }
    fn collect_metrics(&mut self, x: &Guard) {
        self.metrics = &mut Metrics::new() as *mut _;
        if !self.metrics.is_null() {
            self.policy.collect_metrics(self.metrics, x);
        }
    }
}

impl<K: Send, V: Clone + Send> Cache< K, V>
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
    pub fn set<'g>(&'g self, key: K, value: V, cost: i64, guard: &'g Guard) -> bool {
        let (key_hash, confilict_hash) = (self.key_to_hash)(&key);
        let mut item = Item {
            flag: ITEM_NEW,
            key: key_hash,
            conflict: confilict_hash,
            value: Some(value.clone()),
            cost,
        };
        // attempt to immediately update hashmap value and set flag to update so the
        // cost is eventually updated
        if self.store.update(key_hash, confilict_hash, value.clone(), guard) {
            item.flag = ITEM_UPDATE;
        }

        self.process_items(item, guard);
        true
        /*  select! {
              send(self.set_buf, item)->res => true,
              default => {
                  if let Some(ref mut m) = self.metrics {
                   m.add(dropSets, key_hash, 1);
                }
                  false
              },
          }*/
    }
    /// Del deletes the key-value item from the cache if it exists.
    pub fn del<'g>(&'g self, key: K, guard: &'g Guard) {
        let (key_hash, confilict_hash) = (self.key_to_hash)(&key);
        let item = Item {
            flag: ITEM_DELETE,
            key: key_hash,
            conflict: confilict_hash,
            value: None,
            cost: 0,
        };

        self.process_items(item, guard);
        // self.set_buf.send(item);
    }

    /// Get returns the value (if any) and a boolean representing whether the
    /// value was found or not. The value can be nil and the boolean can be true at
    /// the same time.
    pub fn get<'g>(&'g self, key: &K, guard: &'g Guard) -> Option< V> {
        let (key_hash, confilict_hash) = (self.key_to_hash)(key);
        self.get_buf.push(key_hash, guard);
        let result = self.store.Get(key_hash, confilict_hash, guard);

        return match result {
            None => {

                if !self.metrics.is_null() {
                    unsafe { self.metrics.as_mut().unwrap().add(hit, key_hash, 1) };
                }

                None
            }
            Some(ref v) => {
                if !self.metrics.is_null() {
                    unsafe { self.metrics.as_mut().unwrap().add(miss, key_hash, 1) };
                }

                result

            }
        };
    }
    /// Close stops all goroutines and closes all channels.
    pub fn close(&mut self, guard: &Guard) {
        // block until processItems  is returned
        self.stop_sender.try_send(true);
        self.policy.close()
    }
    /// Clear empties the hashmap and zeroes all policy counters. Note that this is
    /// not an atomic operation (but that shouldn't be a problem as it's assumed that
    /// Set/Get calls won't be occurring until after this).
    pub fn clear(&mut self, guard: &Guard) {
        // block until processItems  is returned
        self.stop_sender.try_send(true);
        self.policy.clear(guard);
        self.store.clear(guard);
        if !self.metrics.is_null() {
            unsafe { self.metrics.as_mut().unwrap().clear() };
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

    pub fn process_items<'g>(&'g self, mut i: Item<V>, guard: &'g Guard) {


            //todo cost fn
            if i.cost == 0 && self.cost.is_some() && i.flag != ITEM_DELETE {
                i.cost = (self.cost.unwrap())(i.value.clone().unwrap())
            }
            match i.flag {
                ITEM_NEW => {
                    let (mut victims, added) = self.policy.add(i.key, i.cost, guard);
                    if added {
                        self.store.Set(i.key, i.conflict, i.value.unwrap(), guard);
                        if !self.metrics.is_null() {
                            unsafe {
                                self.metrics.as_mut().unwrap().add(keyAdd, i.key, 1);
                                self.metrics.as_mut().unwrap().add(costAdd, i.key, i.cost as u64);
                            };
                        }

                    }
                    for i in 0..victims.len() {
                        let mut delVal = self.store.Del(victims[i].key, 0, guard);
                        match delVal {
                            Some((mut c, mut v)) => {
                                victims[i].value = Some(v.clone());
                                victims[i].conflict = c;
                                if self.on_evict.is_some() {
                                    (self.on_evict.unwrap())(victims[i].key, victims[i].conflict, victims[i].value.clone().unwrap(), victims[i].cost)
                                }
                                if !self.metrics.is_null() {
                                    unsafe {
                                        self.metrics.as_mut().unwrap().add(keyEvict, victims[i].key, 1);
                                        self.metrics.as_mut().unwrap().add(costEvict, victims[i].key, victims[i].cost as u64);

                                    };
                                }

                            }
                            None => { continue; }
                        }
                    }

                }
                ITEM_UPDATE => {
                    self.policy.update(i.key, i.cost, guard);

                }
                ITEM_DELETE => {
                    self.policy.del(i.key, guard);
                    self.store.Del(i.key, i.conflict, guard);

                }
                _ => { }
            }

    }
    fn check_guard(&self, guard: &Guard<'_>) {
        if let Some(c) = guard.collector() {
            assert!(Collector::ptr_eq(c, &self.collector));
        }
    }

  /*  fn process_itemsw(&mut self, guard: &Guard) {
        loop {
            select! {
               recv(self.receiver_buf) -> item => {
                    match item {
                        Ok(mut i)=> {
                                //todo cost fn
                            if i.cost ==0 && self.cost.is_some() && i.flag != ITEM_DELETE {
                                    i.cost = (self.cost.unwrap())(i.value.clone().unwrap())
                            }
                            match i.flag {
                                ITEM_NEW=>{

                                    let  (mut victims, added ) = self.policy.add(i.key,i.cost ,guard);
                                    if added {
                                        self.store.Set(i.key,i.conflict,i.value.unwrap(),guard);

                                          if !self.metrics.is_null() {
                                    unsafe {
                                        self.metrics.as_mut().unwrap().add(keyEvict, victims[i].key, 1);
                                        self.metrics.as_mut().unwrap().add(costEvict, victims[i].key, victims[i].cost as u64);

                                    };
                                }
                                        if let Some(ref mut metrics) = self.metrics {
                                            metrics.add(keyAdd,i.key,1);
                                            metrics.add(costAdd,i.key,i.cost as u64);
                                        }
                                    }
                                    for i in 0..victims.len() {

                                        let mut delVal = self.store.Del(victims[i].key,0,guard);
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
                                    self.store.Del(i.key,i.conflict,guard);
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
    }*/
}

#[cfg(test)]
mod tests {
    use std::thread;
    use std::time::Duration;
    use crate::bloom::hasher;
    use crate::bloom::hasher::{key_to_hash, KeyHash, value_to_int};
    use crate::cache::{Cache, Config};

    #[test]
    fn test_cache_key_to_hash() {
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

        let guard = cache.guard();
        cache.set("key", "value", 1, &guard);
        thread::sleep(Duration::from_millis(10));
        let v = cache.get(&"key", &guard);
        assert_eq!(v, Some(&"value"));

        cache.del("key",&guard);
        thread::sleep(Duration::from_millis(10));
        let v = cache.get(&"key", &guard);
        assert_eq!(v, None);
        // cache.set(1, 1, 1, &guard);
    }
}