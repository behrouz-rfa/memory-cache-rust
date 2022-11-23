use std::any::{Any, TypeId};
use crate::bloom::rutil::{mem_hash, mem_hash_byte};
use xxhash_rust::const_xxh3::xxh3_64 as const_xxh3;
use xxhash_rust::xxh3::xxh3_64;

use std::mem::size_of;
use std::ops::Deref;


pub type KeyHash<T> = Box<dyn FnMut(T) -> (u64, i64)>;



pub fn key_to_hash<T: Copy + 'static + std::fmt::Display>(key: & T) -> (u64, u64)
    where

{
    let key = *key.deref();
    if equals::<T, String>() {
        let v = cast_ref::<_, String>(&key).unwrap();
        let raw = v.as_bytes();
        return (mem_hash(raw), const_xxh3(raw) as u64);
    }


    // if equals::<T, Vec<u8>>() {
    //     let value = unsafe { std::mem::transmute::<T, Vec<u8>>(key) };
    //
    //     return (mem_hash(&value), const_xxh3(&value) as i64);
    // }


    if equals::<T, u64>() {
        let value = cast_ref::<_, u64>(&key).unwrap();
        return (*value, 0);
    }

    if equals::<T, usize>() {
        let value = cast_ref::<_, usize>(&key).unwrap();
        return (*value as u64, 0);
    }


    if equals::<T, i64>() {
        let value = cast_ref::<_, i64>(&key).unwrap();
        return (*value as u64, 0);
    }


    if equals::<T, i32>() {
        let value = cast_ref::<_, i32>(&key).unwrap();

        return (*value as u64, 0);
    }

    if equals::<T, u8>() {
        let value = cast_ref::<_, u8>(&key).unwrap();
        return (*value as u64, 0);
    }
    if equals::<T, u16>() {
        let value = cast_ref::<_, u16>(&key).unwrap();

        return (*value as u64, 0);
    }
    if equals::<T, u32>() {
        let value = cast_ref::<_, u32>(&key).unwrap();

        return (*value as u64, 0);
    }
    if equals::<T, i16>() {
        let value = cast_ref::<_, i16>(&key).unwrap();

        return (*value as u64, 0);
    }

    let f = format!("{}",key);

    match cast_ref::<_, String>(&f) {
        None => {
            panic!("not suported");
        }
        Some(v) => {
            let raw = v.as_bytes();
            return (mem_hash(raw), const_xxh3(raw) as u64);
        }
    }


    // panic!("! Key type not supported")


    // if TypeId::of::<T>() == TypeId::of::<[u8]>() {
    //     let raw = (key) as [u8];
    //     return (mem_hash_byte(&raw), const_xxh3(&raw) as i64);
    // }
    // return (mem_hash(&mut key.into_bytes()), const_xxh3(&* key.into_bytes()) as i64);
}


pub fn value_to_int<T: 'static>(key: T) -> i64 {
    if equals::<T, u64>() {
        let value = cast_ref::<_, u64>(&key).unwrap();
        return *value as i64;
    }

    if equals::<T, usize>() {
        let value = cast_ref::<_, usize>(&key).unwrap();
        return *value as i64;
    }


    if equals::<T, i64>() {
        let value = cast_ref::<_, i64>(&key).unwrap();
        return *value as i64;
    }


    if equals::<T, i32>() {
        let value = cast_ref::<_, i32>(&key).unwrap();

        return *value as i64;
    }

    if equals::<T, u8>() {
        let value = cast_ref::<_, u8>(&key).unwrap();
        return *value as i64;
    }
    if equals::<T, u16>() {
        let value = cast_ref::<_, u16>(&key).unwrap();

        return *value as i64;
    }
    if equals::<T, u32>() {
        let value = cast_ref::<_, u32>(&key).unwrap();

        return *value as i64;
    }
    if equals::<T, i16>() {
        let value = cast_ref::<_, i16>(&key).unwrap();

        return *value as i64;
    }
    panic!("! value type not supported")
}

pub fn equals<U: 'static, V: 'static>() -> bool {
    TypeId::of::<U>() == TypeId::of::<V>() && size_of::<U>() == size_of::<V>()
}

pub fn cast_ref<U: 'static, V: 'static>(u: &U) -> Option<&V> {
    if equals::<U, V>() {
        Some(unsafe { std::mem::transmute::<&U, &V>(u) })
    } else {
        None
    }
}

pub fn cast_mut<U: 'static, V: 'static>(u: &mut U) -> Option<&mut V> {
    if equals::<U, V>() {
        Some(unsafe { std::mem::transmute::<&mut U, &mut V>(u) })
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash() {
        let a ="22";
        let d = key_to_hash(&a);
        ;
        println!("{:?}", d);
    }
}