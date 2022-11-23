use std::collections::HashMap;
use std::ops::Deref;
use std::sync::atomic::Ordering;
use std::thread;
use std::time::{Duration, Instant};


use parking_lot::Mutex;
use seize::{Collector, Guard, Linked};
use crate::cache::{Item, ItemFlag, NUM_SHARDS, PutResult};
use crate::reclaim;
use crate::reclaim::{Atomic, CompareExchangeError, Shared};
use crate::ttl::ExpirationMap;

pub struct Node<V> {
    pub key: u64,
    pub conflict: u64,
    pub(crate) value: Atomic<V>,
    pub expiration: Option<Duration>,

}

impl<V> Node<V> {
    pub(crate) fn new<AV>(key: u64, conflict: u64, value: AV, expiration: Option<Duration>) -> Self
        where AV: Into<Atomic<V>>,
    {
        Node {
            key,
            conflict,
            value: value.into(),
            expiration,
        }
    }
}

impl<V> Clone for Node<V> {
    fn clone(&self) -> Self {
        Self {
            key: self.key,
            conflict: self.conflict,
            value:self.value.clone(),
            expiration: self.expiration
        }
    }
}

pub(crate) struct Store<V> {
    pub data: Vec<HashMap<u64, Node<V>>>,
    em: ExpirationMap,
    lock: Mutex<()>,
}


impl<V> Store<V> {
    pub fn new() -> Self {
        Self::from(Vec::with_capacity(NUM_SHARDS))
    }
    pub fn from(mut data: Vec<HashMap<u64, Node<V>>>) -> Self {
        for i in 0..NUM_SHARDS {
            data.push(HashMap::new());
        }

        Self {
            data: data,
            em: ExpirationMap::new(),
            lock: Default::default(),
        }
    }
    pub(crate) fn clear<'g>(&'g self, guard: &'g Guard) {
        // self.data
    }
    pub(crate) fn is_empty(&self) -> bool {
        self.data.is_empty()
    }


    pub(crate) fn bini(&self, hash: u64) -> usize {
        (hash % NUM_SHARDS as u64) as usize
    }


 /*   pub(crate) fn bin<'g>(&'g self, i: usize, guard: &'g Guard<'_>) -> Shared<'g, HashMap<u64, Node<V>>> {
        self.data[i].load(Ordering::Acquire, guard)
    }


    pub(crate) fn cas_bin<'g>(
        &'g self,
        i: usize,
        current: Shared<'_, HashMap<u64, Node<V>>>,
        new: Shared<'g, HashMap<u64, Node<V>>>,
        guard: &'g Guard<'_>,
    ) -> Result<Shared<'g, HashMap<u64, Node<V>>>, reclaim::CompareExchangeError<'g, HashMap<u64, Node<V>>>> {
        self.data[i].compare_exchange(current, new, Ordering::AcqRel, Ordering::Acquire, guard)
    }*/
    pub(crate) fn expiration<'g>(&'g mut self, key: &u64, guard: &'g Guard<'_>) -> Option<Duration> {
        let index = self.bini(*key);

        return match   self.data[index].get(key) {
            None => None,
            Some(v) => {
                v.expiration
            }
        };
    }

    pub fn get<'g>(&'g self, key_hash: u64, confilict_hash: u64, guard: &'g Guard<'_>) -> Option<&'g V> {
        let index = self.bini(key_hash);



        return match self.data[index].get(&key_hash) {
            None => None,
            Some(v) => {
                if confilict_hash != 0 && confilict_hash != v.conflict {
                    return None;
                }
                let now = Instant::now();
                if v.expiration.is_some() && v.expiration.unwrap().as_millis() > now.elapsed().as_millis() {
                    None
                } else {
                    let v = v.value.load(Ordering::SeqCst, guard);
                    assert!(!v.is_null());
                    return Some((unsafe { v.as_ref().unwrap().deref() }));
                }
            }
        };
    }

    pub(crate) fn set<'g>(&'g mut self, item: Node<V>, guard: &'g Guard<'_>) {
        let lock = self.lock.lock();




        let index = self.bini(item.key);

        match self.data[index].get(&item.key) {
            None => {
                if item.expiration.is_some() {
                    self.em.add(item.key, item.conflict, item.expiration.unwrap(), guard);
                }
                self.data[index].insert(item.key,item);
               drop(lock);
                return;
            }
            Some(v) if v.conflict != item.conflict && item.conflict != 0 => {
                drop(lock);
                return;
            }
            Some(v) => {
                if v.expiration.is_some() {
                    self.em.update(item.key, item.conflict, v.expiration.unwrap(), item.expiration.unwrap(), guard);
                }

                self.data[index].insert(item.key,item);
                drop(lock);
                return;
            }
        }
        drop(lock);
        return;
    }

    pub(crate) fn update<'g>(&'g mut self, item: Item<V>, guard: &'g Guard<'_>) -> bool {
        let index = self.bini(item.key);


        return match self.data[index].get_mut(&item.key) {
            None => {
                false
            }
            Some(v) if v.conflict != item.conflict && item.conflict != 0 => {
                false
            }
            Some(v) => {
                if v.expiration.is_some() {
                    //todo
                    self.em.update(item.key, item.conflict, v.expiration.unwrap(), item.expiration.unwrap(), guard);
                }
                self.data[index].insert(item.key, Node {
                    key: item.key,
                    conflict: item.conflict,
                    value: item.value,
                    expiration: item.expiration,

                });

                true
            }
        };
    }

    pub(crate) fn del<'g>(&'g mut self, key_hash: &u64, conflict: &u64, guard: &'g Guard<'_>) -> Option<(u64, &'g V)> {
        let index = self.bini(*key_hash);


        return match self.data[index].get_mut(key_hash) {
            None => {
                None
            }
            Some(v) if v.conflict != *conflict && *conflict != 0 => {
                None
            }
            Some(v) => {
                self.em.del(&v.key, v.expiration.unwrap(), guard);
                if let Some(item) = self.data[index].remove(key_hash) {
                    let v = item.value.load(Ordering::SeqCst, guard);
                    assert!(!v.is_null());
                    return Some((item.conflict, unsafe { v.as_ref().unwrap().deref() }));
                }
                None
            }
        };
    }
}


