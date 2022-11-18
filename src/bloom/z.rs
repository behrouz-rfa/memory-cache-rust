use std::any::{Any, TypeId};
use crate::bloom::rutil::{MemHash, MemHashByte};
use xxhash_rust::const_xxh3::xxh3_64 as const_xxh3;
use xxhash_rust::xxh3::xxh3_64;

pub type KeyHash = Box<dyn FnMut() -> (u64, i64)>;

pub fn key_to_hash<T>(key: String) -> KeyHash {
    Box::new(move || {
        let raw = key.as_bytes();
        return (MemHash(raw), const_xxh3(raw) as i64);
    })

    // if TypeId::of::<T>() == TypeId::of::<[u8]>() {
    //     let raw = (key) as [u8];
    //     return (MemHashByte(&raw), const_xxh3(&raw) as i64);
    // }
    // return (MemHash(&mut key.into_bytes()), const_xxh3(&* key.into_bytes()) as i64);
}