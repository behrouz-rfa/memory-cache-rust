use std::collections::HashMap;
use parking_lot::Mutex;


pub trait Store<T> {
    // Get returns the value associated with the key parameter.
    fn Get(&self, key_hash: u64, confilict_hash: u64) -> Option<&T>;
    // Set adds the key-value pair to the Map or updates the value if it's
// already present.
    fn Set(&mut self, key_hash: u64, confilict_hash: u64, v: T);
    // Del deletes the key-value pair from the Map.
    fn Del(&mut self, key_hash: u64, confilict_hash: u64) -> (u64, T);
    // Update attempts to update the key with a new value and returns true if
// successful.
    fn update(&mut self, key_hash: u64, confilict_hash: u64, v: T) -> bool;
    // clear clears all contents of the store.
    fn clear(&self);
}

struct StoreItem<T> {
    key: u64,
    confilict: u64,
    value: T,
}

pub struct LockeMap<T> {
    l: Mutex<()>,
    data: HashMap<u64, StoreItem<T>>,
}

pub struct ShardedMap<T> {
    shared: Vec<LockeMap<T>>,
}

impl<T> LockeMap<T> {
    fn new() -> Self {
        LockeMap {
            l: Default::default(),
            data: HashMap::default(),
        }
    }
}

impl<T> ShardedMap<T> {
    pub(crate) fn new() -> Self {
        let mut sm = ShardedMap {
            shared: Vec::new()
        };
        for i in 0..sm.shared.len() {
            sm.shared[i] = LockeMap::new()
        }
        sm
    }
}

const NUM_SHARDS: u64 = 256;

impl<T> Store<T> for ShardedMap<T> {
    fn Get(&self, key: u64, conflict: u64) -> Option<&T> {
        self.shared[(key & NUM_SHARDS) as usize].Get(key, conflict)
    }

    fn Set(&mut self, key: u64, conflict: u64, v: T) {
        self.shared[(key & NUM_SHARDS) as usize].Set(key, conflict, v)
    }

    fn Del(&mut self, key: u64, conflict: u64) -> (u64, T) {
        self.shared[(key & NUM_SHARDS) as usize].Del(key, conflict)
    }

    fn update(&mut self, key: u64, conflict: u64, v: T) -> bool {
        self.shared[(key & NUM_SHARDS) as usize].update(key, conflict, v)
    }

    fn clear(&self) {
        todo!()
    }
}

impl<T> Store<T> for LockeMap<T> {
    fn Get(&self, key_hash: u64, confilict_hash: u64) -> Option<&T> {
        let mut l = self.l.lock();
        match self.data.get(&key_hash) {
            None => None,
            Some(v) => {
                if confilict_hash != 0 && confilict_hash != v.confilict {
                    drop(l);
                    None
                } else {
                    drop(l);
                    Some(&v.value)
                }
            }
        }
    }

    fn Set(&mut self, key_hash: u64, conflict: u64, value: T) {
        let l = self.l.lock();
        match self.data.get(&key_hash) {
            None => {
                self.data.insert(key_hash, StoreItem {
                    key: key_hash,
                    confilict: conflict,
                    value
                });
                drop(l);

            }
            Some(v) if v.confilict != conflict && conflict != 0 => {
                drop(l);
                return ;
            }
            Some(v)=> {
                self.data.insert(key_hash, StoreItem {
                    key: key_hash,
                    confilict: conflict,
                    value
                });
                drop(l);
            }
        }
    }

    fn Del(&mut self, key_hash: u64, conflict: u64) -> (u64, T) {
        todo!()
    }

    fn update(&mut self, key_hash: u64, conflict: u64, value: T) -> bool {
        let l = self.l.lock();
        match self.data.get(&key_hash) {
            None => {
                drop(l);
                return false;
            }
            Some(v) if v.confilict != conflict && conflict != 0 => {
                drop(l);
                return false
            }
            Some(v)=> {
                self.data.insert(key_hash, StoreItem {
                    key: key_hash,
                    confilict: conflict,
                    value
                });
                drop(l);
                true
            }
        }
    }

    fn clear(&self) {
        todo!()
    }
}