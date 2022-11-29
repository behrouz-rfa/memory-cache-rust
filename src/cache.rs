use std::hash::{BuildHasher, BuildHasherDefault, Hash, Hasher};
use std::marker::PhantomData;
use std::ops::{Add, Deref};
use std::sync::atomic::{AtomicIsize, Ordering};
use std::{ptr, thread, time};
use std::any::TypeId;
use std::fmt::{Debug, Formatter};
use std::process::id;
use std::time::Duration;
use seahash::hash;
use seize::{Collector, Guard, Linked};
use crate::bloom::{hasher, haskey};
use crate::reclaim::{Atomic, Shared};
use crate::store::{Node, Store};
use xxhash_rust::const_xxh3::xxh3_64 as const_xxh3;
use xxhash_rust::xxh3::xxh3_64;
use crate::bloom::haskey::key_to_hash;
use crate::cache::ItemFlag::{ItemDelete, ItemNew, ItemUpdate};
use crate::policy::{DefaultPolicy, Policy};
use crate::ring::{RingBuffer, RingStripe};


/// number shared element on store
pub const NUM_SHARDS: usize = 256;

pub enum ItemFlag {
    ItemNew,
    ItemDelete,
    ItemUpdate,
}
macro_rules! load_factor {
    ($n: expr) => {
        // ¾ n = n - n/4 = n - (n >> 2)
        $n - ($n >> 2)
    };
}

pub struct Item<V> {
    pub flag: ItemFlag,
    pub key: u64,
    pub conflict: u64,
    pub(crate) value: Atomic<V>,
    pub cost: i64,
    pub expiration: Option<Duration>,
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

impl<K, V> Default for Config<K, V> {
    fn default() -> Self {
        Config {
            numb_counters: 1e7 as i64, // maximum cost of cache (1GB).
            max_cost: 1 << 30,// maximum cost of cache (1GB).
            buffer_items: 64,// number of keys per Get buffer.
            metrics: true,
            key_to_hash: |x| { (0, 0) },
            on_evict: None,
            cost: None,
        }
    }
}


/// Cache is a thread-safe implementation of a hashmap with a TinyLFU admission
/// policy and a Sampled LFU eviction policy. You can use the same Cache instance
/// from as many goroutines as you want.
pub struct Cache<K, V, S = crate::DefaultHashBuilder> {
    pub(crate) store: Atomic<Store<V>>,
    pub(crate) policy: Atomic<DefaultPolicy<V>>,
    pub(crate) get_buf: Atomic<RingBuffer<V>>,
    collector: Collector,
    // key_to_hash: fn(&K) -> (u64, u64),

    /// Table initialization and resizing control.  When negative, the
    /// table is being initialized or resized: -1 for initialization,
    /// else -(1 + the number of active resizing threads).  Otherwise,
    /// when table is null, holds the initial table size to use upon
    /// creation, or 0 for default. After initialization, holds the
    /// next element count value upon which to resize the table.
    size_ctl: AtomicIsize,
    size_metrics_ctl: AtomicIsize,
    size_buf_ctl: AtomicIsize,
    build_hasher: S,
    pub on_evict: Option<fn(u64, u64, &V, i64)>,
    cost: Option<fn(&V) -> (i64)>,

    _marker: PhantomData<K>,

    pub numb_counters: i64,
    pub buffer_items: usize,
    // max_cost can be considered as the cache capacity, in whatever units you
    // choose to use.
    //
    // For example, if you want the cache to have a max capacity of 100MB, you
    // would set MaxCost to 100,000,000 and pass an item's number of bytes as
    // the `cost` parameter for calls to Set. If new items are accepted, the
    // eviction process will take care of making room for the new item and not
    // overflowing the MaxCost value.
    pub max_cost: i64,

    pub(crate) metrics: Atomic<Metrics>,
}

impl<K, V, S> Debug for Cache<K, V, S> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Cache")
            .field(&self.numb_counters)
            .finish()
    }
}

impl<K, V, S> Clone for Cache<K, V, S>
    where
        K: Sync + Send + Clone + Hash + Ord,
        V: Sync + Send + Clone,
        S: BuildHasher + Clone,
{
    fn clone(&self) -> Cache<K, V, S> {
        Self {
            store: self.store.clone(),
            policy: Atomic::from(self.policy.load(Ordering::SeqCst, &self.guard())),
            get_buf: Atomic::from(self.get_buf.load(Ordering::SeqCst, &self.guard())),
            collector: self.collector.clone(),
            size_ctl: AtomicIsize::from(self.size_ctl.load(Ordering::SeqCst)),
            size_metrics_ctl: AtomicIsize::from(self.size_metrics_ctl.load(Ordering::SeqCst)),
            size_buf_ctl: AtomicIsize::from(self.size_buf_ctl.load(Ordering::SeqCst)),
            build_hasher: self.build_hasher.clone(),
            on_evict: None,
            cost: None,

            _marker: Default::default(),

            numb_counters: self.numb_counters,
            buffer_items: self.buffer_items,
            max_cost: self.max_cost,
            metrics: Atomic::from(self.metrics.load(Ordering::SeqCst, &self.guard())),
        }
    }
}

impl<K, V> Cache<K, V, crate::DefaultHashBuilder> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_config(c: Config<K,V>) -> Self {
        Self::with_hasher(crate::DefaultHashBuilder::default(), c)
    }
}

impl<K, V, S> Default for Cache<K, V, S>
    where
        S: Default,
{
    fn default() -> Self {
        Self::with_hasher(S::default(), Default::default())
    }
}

impl<K, V, S> Cache<K, V, S>

{
    pub fn with_hasher(hash_builder: S, c: Config<K, V>) -> Self {
        let c = Cache {
            store: Atomic::null(),
            policy: Atomic::null(),
            get_buf: Atomic::null(),
            collector: Collector::new(),
            size_ctl: AtomicIsize::new(0),
            size_metrics_ctl: AtomicIsize::new(0),
            size_buf_ctl: AtomicIsize::new(0),
            build_hasher: hash_builder,
            on_evict: None,
            cost: None,
            buffer_items: c.buffer_items,
            _marker: Default::default(),

            numb_counters: c.numb_counters,
            max_cost: c.max_cost,
            metrics: Atomic::null(),
        };

        c
    }

    /// Pin a `Guard` for use with this map.
    ///
    /// Keep in mind that for as long as you hold onto this `Guard`, you are preventing the
    /// collection of garbage generated by the map.
    pub fn guard(&self) -> Guard<'_> {
        self.collector.enter()
    }

    fn check_guard(&self, guard: &Guard<'_>) {
        if let Some(c) = guard.collector() {
            assert!(Collector::ptr_eq(c, &self.collector))
        }
    }


    fn init_metrics<'g>(&'g self, guard: &'g Guard<'_>) -> Shared<'g, Metrics> {
        loop {
            let table = self.metrics.load(Ordering::SeqCst, guard);
            // safety: we loaded the table while the thread was marked as active.
            // table won't be deallocated until the guard is dropped at the earliest.
            if !table.is_null() {
                break table;
            }

            //try to allocate the table
            let mut sc = self.size_metrics_ctl.load(Ordering::SeqCst);
            if sc < 0 {
                // we lost the initialization race; just spin
                std::thread::yield_now();
                continue;
            }

            if self
                .size_metrics_ctl
                .compare_exchange(sc, -1, Ordering::SeqCst, Ordering::Relaxed)
                .is_ok() {
                // we get to do it!
                let mut table = self.metrics.load(Ordering::SeqCst, guard);

                // safety: we loaded the table while the thread was marked as active.
                // table won't be deallocated until the guard is dropped at the earliest.
                if table.is_null() {
                    let n = if sc > 0 {
                        sc as usize
                    } else {
                        doNotUse
                    };
                    table = Shared::boxed(Metrics::new(n, &self.collector), &self.collector);
                    self.metrics.store(table, Ordering::SeqCst);
                    sc = load_factor!(n as isize);
                }
                self.size_metrics_ctl.store(sc, Ordering::SeqCst);
                break table;
            }
        }
    }
    fn init_ringbuf<'g>(&'g self, guard: &'g Guard<'_>) -> Shared<'g, RingBuffer<V>> {
        loop {
            let table = self.get_buf.load(Ordering::SeqCst, guard);
            // safety: we loaded the table while the thread was marked as active.
            // table won't be deallocated until the guard is dropped at the earliest.
            if !table.is_null() {
                break table;
            }

            //try to allocate the table
            let mut sc = self.size_buf_ctl.load(Ordering::SeqCst);
            if sc < 0 {
                // we lost the initialization race; just spin
                std::thread::yield_now();
                continue;
            }

            if self
                .size_buf_ctl
                .compare_exchange(sc, -1, Ordering::SeqCst, Ordering::Relaxed)
                .is_ok() {
                // we get to do it!
                let mut table = self.get_buf.load(Ordering::SeqCst, guard);

                // safety: we loaded the table while the thread was marked as active.
                // table won't be deallocated until the guard is dropped at the earliest.
                if table.is_null() {
                    let n = if sc > 0 {
                        sc as usize
                    } else {
                        doNotUse
                    };
                    let p = self.policy.load(Ordering::SeqCst, guard);

                    table = Shared::boxed(RingBuffer::new(p, self.buffer_items), &self.collector);
                    self.get_buf.store(table, Ordering::SeqCst);
                    sc = load_factor!(n as isize);
                }
                self.size_buf_ctl.store(sc, Ordering::SeqCst);
                break table;
            }
        }
    }

    fn init_store<'g>(&'g self, guard: &'g Guard<'_>) -> Shared<'g, Store<V>> {
        loop {
            let table = self.store.load(Ordering::SeqCst, guard);
            // safety: we loaded the table while the thread was marked as active.
            // table won't be deallocated until the guard is dropped at the earliest.
            if !table.is_null() && !unsafe { table.deref() }.is_empty() {
                break table;
            }

            //try to allocate the table
            let mut sc = self.size_ctl.load(Ordering::SeqCst);
            if sc < 0 {
                // we lost the initialization race; just spin
                std::thread::yield_now();
                continue;
            }

            if self
                .size_ctl
                .compare_exchange(sc, -1, Ordering::SeqCst, Ordering::Relaxed)
                .is_ok() {
                // we get to do it!
                let mut table = self.store.load(Ordering::SeqCst, guard);

                // safety: we loaded the table while the thread was marked as active.
                // table won't be deallocated until the guard is dropped at the earliest.
                if table.is_null() || unsafe { table.deref() }.is_empty() {
                    let n = if sc > 0 {
                        sc as usize
                    } else {
                        NUM_SHARDS
                    };
                    table = Shared::boxed(Store::new(), &self.collector);
                    self.store.store(table, Ordering::SeqCst);
                    sc = load_factor!(n as isize);
                }
                self.size_ctl.store(sc, Ordering::SeqCst);
                break table;
            }
        }
    }

    fn init_policy<'g>(&'g self, guard: &'g Guard<'_>) -> Shared<'g, DefaultPolicy<V>> {
        loop {
            let mut table = self.policy.load(Ordering::SeqCst, guard);
            // safety: we loaded the table while the thread was marked as active.
            // table won't be deallocated until the guard is dropped at the earliest.
            if !table.is_null() {
                break table;
            }

            //try to allocate the table
            let metrics = self.init_metrics(guard);

            let mut p = DefaultPolicy::new(self.numb_counters, self.max_cost, metrics);


            table = Shared::boxed(p, &self.collector);
            self.policy.store(table, Ordering::SeqCst);
            let ring_buf = self.init_ringbuf(guard);
            break table;
        }
    }
}

impl<V, K, S> Cache<K, V, S>
    where K: Hash + Ord,
          S: BuildHasher,
{
    pub fn hash<Q: ?Sized + Hash + 'static>(&self, key: &Q) -> (u64, u64) {
        let t = TypeId::of::<&Q>();
        if t == TypeId::of::<&i64>() {
            let v = key as *const Q as *const u8;
            let v = unsafe { v.as_ref().unwrap() };
            return (*v as u64, 0);
        }
        if t == TypeId::of::<&i32>() {
            let v = key as *const Q as *const u8;
            let v = unsafe { v.as_ref().unwrap() };
            return (*v as u64, 0);
        }

        if t == TypeId::of::<&u64>() {
            let v = key as *const Q as *const u8;
            let v = unsafe { v.as_ref().unwrap() };
            return (*v as u64, 0);
        }


        if t == TypeId::of::<&u32>() {
            let v = key as *const Q as *const u8;
            let v = unsafe { v.as_ref().unwrap() };
            return (*v as u64, 0);
        }

        if t == TypeId::of::<&u8>() {
            let v = key as *const Q as *const u8;
            let v = unsafe { v.as_ref().unwrap() };
            return (*v as u64, 0);
        }

        let mut h = self.build_hasher.build_hasher();
        key.hash(&mut h);

        let slice = unsafe {
            std::slice::from_raw_parts(key as *const Q as *const u8, std::mem::size_of_val(key))
        };

        let t = TypeId::of::<Q>();
        if t == TypeId::of::<i64>() {}

        (h.finish(), const_xxh3(slice))
    }


    /// Get returns the value (if any) and a boolean representing whether the
    /// value was found or not. The value can be nil and the boolean can be true at
    /// the same time.
    pub fn get<'g, Q: ?Sized + Hash + 'static>(&'g self, key: &Q, guard: &'g Guard) -> Option<&'g V> {
        let (key_hash, conflict) = self.hash(key);

        let buf = self.get_buf.load(Ordering::SeqCst, guard);
        if buf.is_null() {
            return None;
        }
        unsafe{ buf.deref() }.push(key_hash,guard);

        let mut store = self.store.load(Ordering::SeqCst, guard);

        // let mut old_value = None;

        if store.is_null() {
            return None;
        }

        let result = unsafe { store.deref() }.get(key_hash, conflict, guard);
        return match result {
            None => {
                let metrics = self.metrics.load(Ordering::SeqCst, guard);
                if metrics.is_null() {
                    unsafe { metrics.deref().add(hit, key_hash, 1, guard) };
                }

                None
            }
            Some(ref v) => {
                let metrics = self.metrics.load(Ordering::SeqCst, guard);
                if metrics.is_null() {
                    unsafe { metrics.deref().add(miss, key_hash, 1, guard) };
                }

                result
            }
        };
    }
}

impl<V, K, S> Cache<K, V, S>
    where
        K: Sync + Send + Clone + Hash + Ord + 'static,
        V: Sync + Send,
        S: BuildHasher,
{
    /*    fn init_metrics2<'g>(&'g self, guard: &'g Guard<'_>) -> Shared<'g, Metrics> {
            loop {
                let mut metrics = self.metrics.load(Ordering::SeqCst, guard);
                if !metrics.is_null() {
                    break metrics;
                }

                metrics = Shared::boxed(Metrics::new(, &self.collector), &self.collector);
                self.metrics.store(metrics, Ordering::SeqCst);
                break metrics;
            }
        }*/



    /// Set attempts to add the key-value item to the cache. If it returns false,
    /// then the Set was dropped and the key-value item isn't added to the cache. If
    /// it returns true, there's still a chance it could be dropped by the policy if
    /// its determined that the key-value item isn't worth keeping, but otherwise the
    /// item will be added and other items will be evicted in order to make room.
    ///
    /// To dynamically evaluate the items cost using the Config.Coster function, set
    /// the cost parameter to 0 and Coster will be ran when needed in order to find
    /// the items true cost.
    pub fn set<'g>(&'g self, key: K, value: V, cost: i64, guard: &'g Guard<'_>) -> bool {
        self.check_guard(guard);
        self.set_with_ttl(key, value, cost, Duration::from_millis(0), guard)
    }


    /// SetWithTTL works like Set but adds a key-value pair to the cache that will expire
    /// after the specified TTL (time to live) has passed. A zero value means the value never
    /// expires, which is identical to calling Set. A negative value is a no-op and the value
    /// is discarded.
    pub fn set_with_ttl<'g>(&'g self, key: K, value: V, cost: i64, ttl: Duration, guard: &'g Guard) -> bool {
        let mut expiration: Option<Duration> = None;
        if ttl.as_millis() < 0 {
            return false;
        } else if ttl.is_zero() {
            expiration = Some(ttl)
        } else if ttl.as_millis() > 0 {
            expiration = Some(ttl)
        } else {
            expiration = Some(time::SystemTime::now().elapsed().unwrap().checked_add(ttl).unwrap())
        }
        let (key_hash, conflict) = self.hash(&key);

        let mut store = self.store.load(Ordering::SeqCst, guard);
        let value = Shared::boxed(value, &self.collector);
        // let mut old_value = None;
        loop {
            if store.is_null() {
                store = self.init_store(guard);
                continue;
            }

            let store = unsafe { store.as_ptr() };
            let store = unsafe { store.as_mut().unwrap() };

            let mut item = Item {
                flag: ItemNew,
                key: key_hash,
                conflict: conflict,
                value: value.into(),
                cost,
                expiration,
            };
            let mut item2 = Item {
                flag: ItemNew,
                key: key_hash,
                conflict: conflict,
                value: value.into(),
                cost,
                expiration,
            };
            // cost is eventually updated. The expiration must also be immediately updated
            // to prevent items from being prematurely removed from the map.
            if store.update(item, guard) {
                item2.flag = ItemUpdate
            };

            let node = Node::new(key_hash, conflict, value, expiration);


            match item2.flag {
                ItemNew | ItemUpdate => unsafe {
                    if item2.cost == 0 && self.cost.is_some() {
                        item2.cost = (self.cost.unwrap())(item2.value.load(Ordering::SeqCst, guard).deref());
                    }
                }
                _ => {}
            }

            let mut policy = self.policy.load(Ordering::SeqCst, guard);
            if policy.is_null() {
                policy = self.init_policy(guard);
                continue;
            }

            match item2.flag {
                ItemNew => {
                    let (mut victims, added) = unsafe {
                        let policy = policy.as_ptr();
                        policy.as_mut().unwrap().add(item2.key, item2.cost, guard)
                    };

                    if added {
                        store.set(node, guard);
                    }


                    for i in 0..victims.len() {
                        let mut delVal = store.del(&victims[i].key, &0, guard);
                        match delVal {
                            Some((mut c, mut v)) => {
                                // victims[i].value = Some(v.clone());
                                // victims[i].conflict = c;

                                if self.on_evict.is_some() {
                                    let v = victims[i].value.load(Ordering::SeqCst, guard);

                                    (self.on_evict.unwrap())(victims[i].key, victims[i].conflict, unsafe { v.deref().deref().deref() }, victims[i].cost)
                                }
                                // if !self.metrics.is_null() {
                                //     unsafe {
                                //         self.metrics.as_mut().unwrap().add(keyEvict, victims[i].key, 1);
                                //         self.metrics.as_mut().unwrap().add(costEvict, victims[i].key, victims[i].cost as u64);
                                //     };
                                // }
                            }
                            None => { continue; }
                        }
                    }
                    break true;
                }
                ItemDelete => {
                    unsafe {
                        let policy = policy.as_ptr();
                        policy.as_mut().unwrap().del(&item2.key, guard)
                    }
                    store.del(&item2.key, &item2.conflict, guard);
                }
                ItemUpdate => {
                    unsafe {
                        let policy = policy.as_ptr();
                        policy.as_mut().unwrap().update(item2.key, item2.cost, guard);
                    }
                    // unsafe { policy.deref() }.update(item2.key, item2.cost, guard);
                }
            }


            // self.process_items(node, item2, cost, guard);

            break true;
        }
    }


    /// Del deletes the key-value item from the cache if it exists.
    pub fn del<'g, Q: ?Sized + Hash + 'static>(&'g self, key: &Q, guard: &'g Guard) {
        let (key_hash, conflict) = self.hash(key);
        let item = Item {
            flag: ItemDelete,
            key: key_hash,
            conflict: conflict,
            value: Atomic::null(),
            cost: 0,
            expiration: None,
        };

        let node = Node {
            key: 0,
            conflict: 0,
            value: Atomic::null(),
            expiration: None,

        };


        self.process_items(node, item, 0, guard);
        // self.set_buf.send(item);
    }


    /// Clear empties the hashmap and zeroes all policy counters. Note that this is
    /// not an atomic operation (but that shouldn't be a problem as it's assumed that
    /// Set/Get calls won't be occurring until after this).
    pub fn clear<'g>(&'g self, guard: &'g Guard) {
        // block until processItems  is returned
        let store = self.store.load(Ordering::SeqCst, guard);
        let policy = self.policy.load(Ordering::SeqCst, guard);
        let metrics = self.metrics.load(Ordering::SeqCst, guard);


        unsafe {
            if !policy.is_null() {
                let policy = policy.as_ptr();
                policy.as_mut().unwrap().clear(guard);
            }
        }
        if !store.is_null() {
            unsafe {
                let  p = store.as_ptr();
                p.as_mut().unwrap().clear(guard);
            };
        }
        if !metrics.is_null() {
            unsafe { metrics.deref() }.clear(guard);
        }

        /* let (tx, rx) = crossbeam_channel::unbounded();
         self.set_buf = tx;
         self.receiver_buf = rx;*/

        //TODO fix thead after clear
        /* thread::spawn( || {
             let guard = crossbeam::epoch::pin();
             self.process_items(&guard);
         });*/
    }

    pub fn process_items<'g>(&'g self, node: Node<V>, mut item: Item<V>, cost: i64, guard: &'g Guard) {
        let mut cost = cost;
        match item.flag {
            ItemNew | ItemUpdate => unsafe {
                if item.cost == 0 && self.cost.is_some() {
                    item.cost = (self.cost.unwrap())(item.value.load(Ordering::SeqCst, guard).deref());
                }
            }
            _ => {}
        }

        match item.flag {
            ItemNew => {
                let mut policy = self.policy.load(Ordering::SeqCst, guard);
                loop {
                    if policy.is_null() {
                        policy = self.init_policy(guard);
                        continue;
                    }
                    let (mut victims, added) = unsafe {
                        let p = policy.as_ptr();
                        let p = p.as_mut().unwrap();
                        p.add(item.key, item.cost, guard)
                    };

                    let store = self.store.load(Ordering::SeqCst, guard);
                    if added {
                        let store = unsafe { store.as_ptr() };
                        let store = unsafe { store.as_mut().unwrap() };
                        store.set(node, guard);
                        break;
                    }

                    for i in 0..victims.len() {
                        let store = unsafe { store.as_ptr() };
                        let store = unsafe { store.as_mut().unwrap() };
                        let mut delVal = store.del(&victims[i].key, &0, guard);
                        match delVal {
                            Some((mut c, mut v)) => {
                                // victims[i].value = Some(v.clone());
                                // victims[i].conflict = c;

                                if self.on_evict.is_some() {
                                    let v = victims[i].value.load(Ordering::SeqCst, guard);

                                    (self.on_evict.unwrap())(victims[i].key, victims[i].conflict, unsafe { v.deref().deref().deref() }, victims[i].cost)
                                }
                                // if !self.metrics.is_null() {
                                //     unsafe {
                                //         self.metrics.as_mut().unwrap().add(keyEvict, victims[i].key, 1);
                                //         self.metrics.as_mut().unwrap().add(costEvict, victims[i].key, victims[i].cost as u64);
                                //     };
                                // }
                            }
                            None => { continue; }
                        }
                    }
                }
            }
            ItemDelete => {
                let mut policy = self.policy.load(Ordering::SeqCst, guard);
                if policy.is_null() {
                    return;
                }

                let store = self.store.load(Ordering::SeqCst, guard);
                unsafe {
                    let p = policy.as_ptr();
                    let p = p.as_mut().unwrap();
                    p.del(&item.key, guard)
                }

                let store = unsafe { store.as_ptr() };
                let store = unsafe { store.as_mut().unwrap() };
                store.del(&item.key, &item.conflict, guard);
            }
            ItemFlag::ItemUpdate => {
                let mut policy = self.policy.load(Ordering::SeqCst, guard);
                if policy.is_null() {
                    return;
                }
                unsafe {
                    let p = policy.as_ptr();
                    let p = p.as_mut().unwrap();
                    p.update(item.key, item.cost, guard);
                }
            }
        }
    }
}


type MetricType = usize;

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

pub struct Metrics {
    pub(crate) all: Box<[Atomic<[u64; 256]>]>,

}

impl Metrics {
    fn new(n: usize, collector: &Collector) -> Self {
        let data = vec![Atomic::from(Shared::boxed([0u64; 256], collector)); n];
        Metrics {
            all: data.into_boxed_slice(),
        }
    }
    pub(crate) fn get<'g>(&'g self, t: MetricType, guard: &'g Guard) -> u64 {
        let all = self.all[t].load(Ordering::SeqCst, guard);
        if all.is_null() {
            return 0;
        }

        let data = unsafe { all.as_ptr() };
        let data = unsafe { data.as_mut().unwrap() };
        let mut total = 0;
        for i in 0..data.len() {
            total += data[i];
        }
        total
    }
    pub(crate) fn SetsDropped<'g>(&'g self, guard: &'g Guard) -> u64 {
        self.get(dropSets, guard)
    }
    pub(crate) fn add<'g>(&self, t: MetricType, hash: u64, delta: u64, guard: &'g Guard) {
        let idx = (hash % 5) * 10;
        let all = self.all[t].load(Ordering::SeqCst, guard);
        if all.is_null() {
            panic!("metric all is null");
        }
        let data = unsafe { all.as_ptr() };
        let data = unsafe { data.as_mut().unwrap() };

        let _ = data[idx as usize].checked_add(delta);
        // unsafe {all.deref().deref().deref()[idx as usize] = delta};
    }

    pub fn clear<'g>(&self, guard: &'g Guard) {
        let data = vec![Atomic::from(Shared::boxed([0u64; 256], guard.collector().unwrap())); doNotUse];
        // self.all.as_mut() = &mut *data.into_boxed_slice();
    }
}

#[derive(Eq, PartialEq, Debug)]
pub enum PutResult<'a, T> {
    Inserted {
        new: &'a T,
    },
    Replaced {
        old: &'a T,
        new: &'a T,
    },
    Exists {
        current: &'a T,
        not_inserted: Box<Linked<T>>,
    },
}


#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::Ordering;
    use std::thread;
    use std::time::Duration;

    use rayon;
    use rayon::prelude::*;
    use crate::cache::{Cache, Config, Item, NUM_SHARDS};
    use crate::cache::ItemFlag::ItemUpdate;
    use crate::reclaim::{Atomic, Shared};
    use crate::store::Node;

    const ITER: u64 = 32 * 1024;

    #[test]
    fn test_cache_key_to_hash() {
        let mut key_to_hash_count = 0;
        let mut cache = Cache::new();

        let guard = cache.guard();
        cache.set(1, 2, 1, &guard);
        cache.set(2, 2, 1, &guard);
        println!("{:?}", cache.get(&1, &guard));
        println!("{:?}", cache.get(&2, &guard));
    }

    #[test]
    fn test_cache_key_to_hash_thread() {
        let mut key_to_hash_count = 0;
        let mut cache = Cache::new();

        let arcc = Arc::new(cache);
        let c1 = Arc::clone(&arcc);
        let c2 = Arc::clone(&arcc);
        let c3 = Arc::clone(&arcc);

        let t1 = thread::spawn(move || {
            let guard = c1.guard();
            for i in 0..100000 {
                c1.set(i, i + 7, 1, &guard);
            }
        });

        let t2 = thread::spawn(move || {
            let guard = c2.guard();
            for i in 0..100000 {
                c2.set(i, i + 7, 1, &guard);
            }
        });

        let t3 = thread::spawn(move || {
            let guard = c3.guard();
            for i in 0..100000 {
                c3.set(i, i + 7, 1, &guard);
            }
        });
        let c41 = Arc::clone(&arcc);
        let t4 = thread::spawn(move || {
            thread::sleep(Duration::from_millis(1000));
            let guard = c41.guard();
            for i in 0..300 {
               println!("{:?}", c41.get(&i, &guard))
            }
        });

        t1.join();
        t2.join();
        t3.join();
        t4.join();
        let c4 = Arc::clone(&arcc);
        let guard = c4.guard();
        c4.set(1, 2, 1, &guard);
        c4.set(2, 2, 1, &guard);
        println!("{:?}", c4.get(&1, &guard));
        println!("{:?}", c4.get(&2, &guard));
    }


    #[test]
    fn test_cache_with_ttl2() {
        let mut key_to_hash_count = 0;
        let cache = Cache::new();

        (0..ITER).into_par_iter().for_each(|i| {
            let guard = cache.guard();
            cache.set(i, i + 7, 1, &guard);
        });
    }

    #[test]
    fn test_cache_with_ttl() {
        let mut key_to_hash_count = 0;
        let cache = Cache::new();

        let guard = cache.guard();

        let key = 1;
        let value = 1;
        let cost = 1;
        let ttl = Duration::from_millis(0);

        loop {
            if !cache.set_with_ttl(1, 1, 1, Duration::from_millis(0), &guard) {
                thread::sleep(Duration::from_millis(10));
                continue;
            }
            thread::sleep(Duration::from_millis(50));
            match cache.get(&key, &guard) {
                None => {
                    assert!(false)
                }
                Some(v) => {
                    assert_eq!(v, &value)
                }
            }
            break;
        }


        cache.set(1, 2, 2, &guard);
        let (key_hash, confilictha) = cache.hash(&1);
        let store = cache.store.load(Ordering::SeqCst, &guard);
        assert_eq!(store.is_null(), false);
        let some = unsafe { store.deref() }.get(key_hash, confilictha, &guard);
        assert_eq!(some.is_some(), true);
        assert_eq!(some.unwrap(), &2);


        thread::sleep(Duration::from_millis(10));

        for i in 0..1000 {
            let (key_hash, conflict) = cache.hash(&1);
            cache.process_items(Node {
                key: 0,
                conflict,
                value: Atomic::null(),
                expiration: None,
            }, Item {
                flag: ItemUpdate,
                key: key_hash,
                conflict: conflict,
                value: Atomic::from(Shared::boxed(1, &cache.collector)),
                cost: 1,
                expiration: Some(Duration::from_millis(200u64)),
            }, 1, &guard);
        }
        thread::sleep(Duration::from_millis(50));
        let v = cache.set(2, 2, 1, &guard);
        assert_eq!(v, true);
        let metrics = cache.metrics.load(Ordering::SeqCst, &guard);
        assert_eq!(metrics.is_null(), false);
        unsafe { assert_eq!(unsafe { metrics.deref() }.SetsDropped(&guard), 0) }
    }
}