use std::collections::HashMap;
use parking_lot::Mutex;


pub trait Store<T> {
    // Get returns the value associated with the key parameter.
    fn Get(&self, key_hash: u64, confilict_hash: u64) -> (T, bool);
    // Set adds the key-value pair to the Map or updates the value if it's
// already present.
    fn Set(&self, key_hash: u64, confilict_hash: u64, v: T);
    // Del deletes the key-value pair from the Map.
    fn Del(&self, key_hash: u64, confilict_hash: u64) -> (u64, T);
    // Update attempts to update the key with a new value and returns true if
// successful.
    fn update(&self, key_hash: u64, confilict_hash: u64, v: T) -> bool;
    // clear clears all contents of the store.
    fn clear(&self);
}

struct StoreItem<T> {
    key: u64,
    confilict: u64,
    value: T,
}

struct LockeMap<T> {
    data: Mutex<HashMap<u64, StoreItem<T>>>,
}

struct ShardedMap<T> {
    shared: Vec<LockeMap<T>>,
}


impl<T> Store<T> for ShardedMap<T> {
    fn Get(&self, key_hash: u64, confilict_hash: u64) -> (T, bool) {
        todo!()
    }

    fn Set(&self, key_hash: u64, confilict_hash: u64, v: T) {
        todo!()
    }

    fn Del(&self, key_hash: u64, confilict_hash: u64) -> (u64, T) {
        todo!()
    }

    fn update(&self, key_hash: u64, confilict_hash: u64, v: T) -> bool {
        todo!()
    }

    fn clear(&self) {
        todo!()
    }
}

impl<T> Store<T> for LockeMap<T> {
    fn Get(&self, key_hash: u64, confilict_hash: u64) -> (T, bool) {
        todo!()
    }

    fn Set(&self, key_hash: u64, confilict_hash: u64, v: T) {
        todo!()
    }

    fn Del(&self, key_hash: u64, confilict_hash: u64) -> (u64, T) {
        todo!()
    }

    fn update(&self, key_hash: u64, confilict_hash: u64, v: T) -> bool {
        todo!()
    }

    fn clear(&self) {
        todo!()
    }
}