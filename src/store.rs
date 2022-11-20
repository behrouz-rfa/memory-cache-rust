use std::collections::HashMap;
use std::sync::atomic::Ordering;

use parking_lot::Mutex;
use seize::{Collector, Guard, Linked};
use crate::reclaim::{Atomic, Shared};

/// store is the interface fulfilled by all hash map implementations in this
/// file. Some hash map implementations are better suited for certain data
/// distributions than others, so this allows us to abstract that out for use
/// in Ristretto.
///
/// Every store is safe for concurrent usage.
pub trait Store<V> {
    // Get returns the value associated with the key parameter.
    fn Get<'g>(&'g self, key_hash: u64, confilict_hash: u64, guard: &'g Guard<'_>) -> Option< V>;
    // Set adds the key-value pair to the Map or updates the value if it's
    // already present.
    fn Set<'g>(&'g self, key_hash: u64, confilict_hash: u64, v: V, guard: &'g Guard);
    // Del deletes the key-value pair from the Map.
    fn Del<'g>(&'g self, key_hash: u64, confilict_hash: u64, guard: &'g Guard) -> Option<(u64, V)>;
    // Update attempts to update the key with a new value and returns true if
    // successful.
    fn update<'g>(&'g self, key_hash: u64, confilict_hash: u64, v: V, guard: &'g Guard) -> bool;
    // clear clears all contents of the store.
    fn clear<'g>(&'g self, guard: &'g Guard);
}

pub struct StoreItem<V> {
    key: u64,
    confilict: u64,
    value: V,
}

#[derive(Clone)]
pub struct LockeMap<V> where V: Clone{
    data: Atomic<HashMap<u64, StoreItem<V>>>,
}

impl<V:Clone> LockeMap<V> {
    pub(crate) fn init_map<'g>(&self, guard: &'g Guard) -> Shared<'g, HashMap<u64, StoreItem<V>>> {
        loop {
            let mut table = self.data.load(Ordering::SeqCst, guard);
            if !table.is_null() && !unsafe { table.deref() }.is_empty() {
                break table;
            }

            table = Shared::boxed(HashMap::new(), guard.collector().unwrap());
            self.data.store(table, Ordering::SeqCst);

            break table;
        }
    }
}

#[derive(Clone)]
pub struct ShardedMap<V:Clone> {
    shared: Vec<LockeMap<V>>,
}

impl<V> LockeMap<V> where V: Clone{
    fn new() -> Self {
        LockeMap {
            data: Atomic::null(),
        }
    }
}

const NUM_SHARDS: u64 = 256;

impl<V:Clone> ShardedMap<V> {
    /// newStore returns the default store implementation.
    pub(crate) fn new() -> Self {
        let mut sm = ShardedMap {
            shared: Vec::new()
        };
        for i in 0..NUM_SHARDS {
            sm.shared.push(LockeMap::new())
        }
        sm
    }
}


impl<V:Clone> Store<V> for ShardedMap<V> {
    fn Get<'g>(&self, key: u64, conflict: u64, guard: &'g Guard<'_>) -> Option< V> {
        self.shared[(key % NUM_SHARDS) as usize].Get(key, conflict, guard)
    }

    fn Set<'g>(&'g self, key: u64, conflict: u64, v: V, guard: &'g Guard<'_>) {
        eprintln!("256 key {}", key % NUM_SHARDS);
        self.shared[(key % NUM_SHARDS) as usize].Set(key, conflict, v, guard)
    }

    fn Del<'g>(&'g self, key: u64, conflict: u64, guard: &'g Guard) -> Option<(u64, V)> {
        self.shared[(key % NUM_SHARDS) as usize].Del(key, conflict, guard)
    }

    fn update<'g>(&'g self, key: u64, conflict: u64, v: V, guard: &'g Guard) -> bool {
        self.shared[(key % NUM_SHARDS) as usize].update(key, conflict, v, guard)
    }

    fn clear<'g>(&'g self, guard: &'g Guard) {
        for i in 0..self.shared.len() {
            self.shared[i].clear(guard);
        }
    }
}

impl<V:Clone> Store<V> for LockeMap<V> {
    fn Get<'g>(&'g self, key_hash: u64, confilict_hash: u64, guard: &'g Guard<'_>) -> Option<V> {
        let data = self.data.load(Ordering::SeqCst, guard);

        if data.is_null() {
            return None;
        }
        let data = unsafe { data.as_ref() };

        return match data.unwrap().get(&key_hash) {
            None => None,
            Some(v) => {
                if confilict_hash != 0 && confilict_hash != v.confilict {
                    None
                } else {
                    Some(v.value.clone())
                }
            }
        };
    }

    fn Set<'g>(&'g self, key_hash: u64, conflict: u64, value: V, guard: &'g Guard) {
        let mut table = self.data.load(Ordering::SeqCst, guard);
        loop {
            if table.is_null() || unsafe { table.deref() }.is_empty() {
                table = self.init_map(guard);
            }
            let mut data = unsafe { table.as_ptr() };
            unsafe {
                match data.as_ref() {
                    None => {
                        todo!();
                        return;
                    }
                    Some(_) => {
                        let data = data.as_mut().unwrap();
                        match data.get(&key_hash) {
                            None => {
                                data.insert(key_hash, StoreItem {
                                    key: key_hash,
                                    confilict: conflict,
                                    value,
                                });
                                eprintln!("data {}", data.len());
                                return;
                            }
                            Some(v) if v.confilict != conflict && conflict != 0 => {
                                return;
                            }
                            Some(v) => {
                                data.insert(key_hash, StoreItem {
                                    key: key_hash,
                                    confilict: conflict,
                                    value,
                                });
                                eprintln!("data {}", data.len());
                                return;
                            }
                        }
                    }
                }
            }
        }
    }

    fn Del<'g>(&'g self, key_hash: u64, conflict: u64, guard: &'g Guard) -> Option<(u64, V)> {
        let mut table = self.data.load(Ordering::SeqCst, guard);
        loop {
            if table.is_null() || unsafe { table.deref() }.is_empty() {
                table = self.init_map(guard);
                continue;
            }


            let mut data = unsafe { table.as_ptr() };
            unsafe {
                match data.as_ref() {
                    None => {
                        todo!();
                        return None;
                    }
                    Some(_) => {
                        let data = data.as_mut().unwrap();
                        match data.get(&key_hash) {
                            None => {
                                return None;
                            }
                            Some(v) => {
                                if conflict != 0 && conflict != v.confilict {
                                    return None;
                                } else {
                                    let store_item = data.remove(&key_hash);
                                    if let Some(item) = store_item {
                                        return Some((item.confilict, item.value));
                                    }
                                    return None;
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    fn update<'g>(&'g self, key_hash: u64, conflict: u64, value: V, guard: &'g Guard) -> bool {
        let mut table = self.data.load(Ordering::SeqCst, guard);
        loop {
            if table.is_null() || unsafe { table.deref() }.is_empty() {
                table = self.init_map(guard);
                return false;
            }


            let mut data = unsafe { table.as_ptr() };
            unsafe {
                match data.as_ref() {
                    None => {
                        todo!();
                        return false;
                    }
                    Some(_) => {
                        let data = data.as_mut().unwrap();
                        return match data.get(&key_hash) {
                            None => {
                                false
                            }
                            Some(v) if v.confilict != conflict && conflict != 0 => {
                                false
                            }
                            Some(v) => {
                                data.insert(key_hash, StoreItem {
                                    key: key_hash,
                                    confilict: conflict,
                                    value,
                                });

                                true
                            }
                        };
                    }
                }
            }
        }
    }

    fn clear<'g>(&'g self, guard: &'g Guard) {
        let mut table = self.data.load(Ordering::SeqCst, guard);

        if table.is_null() || unsafe { table.deref() }.is_empty() {
            table = self.init_map(guard);
            return;
            ;
        }
        let mut data = unsafe { table.as_ptr() };
        let data = unsafe { data.as_mut().unwrap() };
        data.clear();
        // data
        // self.data = Atomic::null();
    }
}