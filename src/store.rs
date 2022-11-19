use std::collections::HashMap;
use std::sync::atomic::Ordering;
use crossbeam::epoch::{Atomic, Guard};
use parking_lot::Mutex;

/// store is the interface fulfilled by all hash map implementations in this
/// file. Some hash map implementations are better suited for certain data
/// distributions than others, so this allows us to abstract that out for use
/// in Ristretto.
///
/// Every store is safe for concurrent usage.
pub trait Store<V> {
    // Get returns the value associated with the key parameter.
    fn Get<'g>(&self, key_hash: u64, confilict_hash: u64, guard: &'g Guard) -> Option<&'g V>;
    // Set adds the key-value pair to the Map or updates the value if it's
    // already present.
    fn Set(&mut self, key_hash: u64, confilict_hash: u64, v: V, guard: & Guard);
    // Del deletes the key-value pair from the Map.
    fn Del<'g>(&mut self, key_hash: u64, confilict_hash: u64, guard: &'g Guard) -> Option<(u64, V)>;
    // Update attempts to update the key with a new value and returns true if
    // successful.
    fn update<'g>(&mut self, key_hash: u64, confilict_hash: u64, v: V, guard: &'g Guard) -> bool;
    // clear clears all contents of the store.
    fn clear<'g>(&mut self, guard: &'g Guard);
}

struct StoreItem<V> {
    key: u64,
    confilict: u64,
    value: V,
}

#[derive(Clone)]
pub struct LockeMap<V> {
    data: Atomic<HashMap<u64, StoreItem<V>>>,
}

#[derive(Clone)]
pub struct ShardedMap<V> {
    shared: Vec<LockeMap<V>>,
}

impl<V> LockeMap<V> {
    fn new() -> Self {
        LockeMap {
            data: Atomic::new(HashMap::default()),
        }
    }
}

const NUM_SHARDS: u64 = 256;
impl<V> ShardedMap<V> {
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



impl<V> Store<V> for ShardedMap<V> {
    fn Get<'g>(&self, key: u64, conflict: u64, guard: &'g Guard) -> Option<&'g V> {
        self.shared[(key & NUM_SHARDS) as usize].Get(key, conflict, guard)
    }

    fn Set(&mut self, key: u64, conflict: u64, v: V, guard: &Guard) {
        self.shared[(key & NUM_SHARDS) as usize].Set(key, conflict, v, guard)
    }

    fn Del<'g>(&mut self, key: u64, conflict: u64, guard: &'g Guard) -> Option<(u64, V)> {
        self.shared[(key & NUM_SHARDS) as usize].Del(key, conflict, guard)
    }

    fn update<'g>(&mut self, key: u64, conflict: u64, v: V, guard: &'g Guard) -> bool {
        self.shared[(key & NUM_SHARDS) as usize].update(key, conflict, v, guard)
    }

    fn clear<'g>(&mut self, guard: &'g Guard) {
        for i in 0..self.shared.len() {
            self.shared[i].clear(guard);
        }
    }
}

impl<V> Store<V> for LockeMap<V> {
    fn Get<'g>(&self, key_hash: u64, confilict_hash: u64, guard: &'g Guard) -> Option<&'g V> {
        let data = self.data.load(Ordering::SeqCst, guard);
        if data.is_null() {
            return None;
        }
        let data = unsafe { data.deref() };
        match data.get(&key_hash) {
            None => None,
            Some(v) => {
                if confilict_hash != 0 && confilict_hash != v.confilict {
                    None
                } else {
                    Some(&v.value)
                }
            }
        }
    }

    fn Set<'g>(&mut self, key_hash: u64, conflict: u64, value: V, guard: &'g Guard) {
        let mut data = self.data.load(Ordering::SeqCst, guard);
        if data.is_null() {
            return ;
        }
        let data = unsafe { data.deref_mut() };
        match data.get(&key_hash) {
            None => {
                data.insert(key_hash, StoreItem {
                    key: key_hash,
                    confilict: conflict,
                    value,
                });
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
            }
        }
    }

    fn Del<'g>(&mut self, key_hash: u64, conflict: u64, guard: &'g Guard) -> Option<(u64, V)> {
        let mut data = self.data.load(Ordering::SeqCst, guard);
        if data.is_null() {
            return None;
        }
        let data = unsafe { data.deref_mut() };
        return match data.get(&key_hash) {
            None => {
                None
            }
            Some(v) => {
                if conflict != 0 && conflict != v.confilict {
                    None
                } else {
                    let store_item = data.remove(&key_hash);
                    if let Some(item) = store_item {
                        return Some((item.confilict, item.value));
                    }
                    None
                }
            }
        };
    }

    fn update<'g>(&mut self, key_hash: u64, conflict: u64, value: V, guard: &'g Guard) -> bool {
        let mut data = self.data.load(Ordering::SeqCst, guard);
        if data.is_null() {
            return false;
        }
        let data = unsafe { data.deref_mut() };
        match data.get(&key_hash) {
            None => {
                return false;
            }
            Some(v) if v.confilict != conflict && conflict != 0 => {
                return false;
            }
            Some(v) => {
                data.insert(key_hash, StoreItem {
                    key: key_hash,
                    confilict: conflict,
                    value,
                });

                true
            }
        }
    }

    fn clear<'g>(&mut self, guard: &'g Guard) {
        let mut data = self.data.load(Ordering::SeqCst, guard);
        if data.is_null() {
            return ;
        }
        let data = unsafe { data.deref_mut() };
        self.data = Atomic::null();
    }
}