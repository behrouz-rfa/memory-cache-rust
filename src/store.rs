use std::collections::HashMap;
use std::ops::{Deref, DerefMut};
use std::sync::atomic::Ordering;
use std::thread;
use std::time::{Duration, Instant};


use parking_lot::Mutex;
use seize::{Collector, Guard, Linked};
use crate::cache::{Item, ItemFlag, NUM_SHARDS, PutResult};
use crate::policy::DefaultPolicy;
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
            value: self.value.clone(),
            expiration: self.expiration,
        }
    }
}

pub(crate) struct Store<V> {
    pub data: Vec<HashMap<u64, Node<V>>>,
    em: ExpirationMap,
    lock: Mutex<()>,
}

impl<V> Clone for Store<V> {
    fn clone(&self) -> Self {
        let mut store = Store::new();
        for map in &self.data {
            store.data.push(map.clone())
        }
        store
    }
}

impl<V> Deref for Store<V> {
    type Target = ();

    fn deref(&self) -> &Self::Target {
        todo!()
    }
}

impl<V> DerefMut for Store<V> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self
    }
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
    pub(crate) fn clear<'g>(&'g mut self, guard: &'g Guard) {
        self.data = Vec::with_capacity(NUM_SHARDS);
        for i in 0..NUM_SHARDS {
            self.data.push(HashMap::new());
        }
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

        return match self.data[index].get(key) {
            None => None,
            Some(v) => {
                v.expiration
            }
        };
    }

    pub fn get<'g>(&'g self, key_hash: u64, confilict_hash: u64, guard: &'g Guard<'_>) -> Option<&'g V> {
        let lock = self.lock.lock();
        let index = self.bini(key_hash);

        return match self.data[index].get(&key_hash) {
            None => {
                drop(lock);
                None
            },
            Some(v) => {
                if confilict_hash != 0 && confilict_hash != v.conflict {
                    drop(lock);
                    return None;
                }
                let now = Instant::now();
                if v.expiration.is_some() && v.expiration.unwrap().as_millis() > now.elapsed().as_millis() {
                    drop(lock);
                    None
                } else {
                    let item = v.value.load(Ordering::SeqCst, guard);
                    if let Some(v) = unsafe { item.as_ref() } {
                        let v = &**v;
                        drop(lock);
                        return Some(v);
                    }
                    drop(lock);
                    return None;
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

                self.data[index].insert(item.key, item);
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

                self.data[index].insert(item.key, item);
                drop(lock);
                return;
            }
        }

    }

    pub(crate) fn update<'g>(&'g mut self, item: &Item<V>, guard: &'g Guard<'_>) -> bool {
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
                    value: item.value.clone(),
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
                if v.expiration.is_some() {
                    self.em.del(&v.key, v.expiration.unwrap(), guard);
                }
                if let Some(item) = self.data[index].remove(key_hash) {
                    let v = item.value.load(Ordering::SeqCst, guard);
                    assert!(!v.is_null());
                    return Some((item.conflict, unsafe { v.as_ref().unwrap().deref() }));
                }
                None
            }
        };
    }

    pub(crate) fn clean_up<'g>(&'g mut self, policy: &mut DefaultPolicy<V>, guard: &'g Guard<'_>) {
        let maps = self.em.cleanup(policy, None, guard);
        for (key, conflict) in maps {
            match self.expiration(&key,
                                  guard) {
                None => { continue; }
                Some(v) => {
                    let cost = policy.cost(&key, guard);
                    policy.del(&key, guard);
                    let value = self.del(&key, &conflict, guard);
                    //ToDO for evict
                    // if f.is_some(){
                    // //ToDO for evict
                    // // f.unwrap()(*key,*confilct,value.unwrap().1,cost)
                    // }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::Ordering;
    use std::thread;
    use seize::Collector;
    use crate::bloom::haskey::key_to_hash;
    use crate::cache::Item;
    use crate::cache::ItemFlag::ItemNew;
    use crate::reclaim::{Atomic, Shared};
    use crate::store::{Node, Store};

    const ITER: u64 = 32 * 1024;

    #[test]
    fn test_set_get() {
        let collector = Collector::new();
        let guard = collector.enter();
        let mut s = Store::new();

        for i in 0..20 {
            let (key, confilict) = key_to_hash(&i);
            let value = Shared::boxed(i + 2, &collector);
            let node = Node::new(key, confilict, value, None);

            s.set(node, &guard);
            let v = s.get(key, confilict, &guard);
            assert_eq!(v, Some(&(i + 2)))
        }
    }

    #[test]
    fn test_set_del() {
        let collector = Collector::new();
        let guard = collector.enter();
        let mut s = Store::new();

        for i in 0..20 {
            let (key, confilict) = key_to_hash(&i);
            let value = Shared::boxed(i + 2, &collector);
            let node = Node::new(key, confilict, value, None);

            s.set(node, &guard);
            let d = s.del(&key, &confilict, &guard);
            assert_eq!(d.unwrap().1, &(i + 2));

            let v = s.get(key, confilict, &guard);
            assert_eq!(v, None);
        }
    }


    #[test]
    fn test_set_clear() {
        let collector = Collector::new();
        let guard = collector.enter();
        let mut s = Store::new();

        for i in 0..20 {
            let (key, confilict) = key_to_hash(&i);
            let value = Shared::boxed(i + 2, &collector);
            let node = Node::new(key, confilict, value, None);

            s.set(node, &guard);
        }
        s.clear(&guard);

        for i in 0..20 {
            let (key, confilict) = key_to_hash(&i);
            let v = s.get(key, confilict, &guard);
            assert_eq!(v, None)
        }
    }

    #[test]
    fn test_set_update() {
        let collector = Collector::new();
        let guard = collector.enter();
        let mut s = Store::new();

        for i in 0..20 {
            let (key, confilict) = key_to_hash(&i);
            let value = Shared::boxed(i + 2, &collector);
            let node = Node::new(key, confilict, value, None);

            s.set(node, &guard);
            let v = s.get(key, confilict, &guard);
            assert_eq!(v, Some(&(i + 2)))
        }

        for i in 0..20 {
            let (key, conflict) = key_to_hash(&i);
            let value = Shared::boxed(i + 4, &collector);
            let item = Item {
                flag: ItemNew,
                key: key,
                conflict: conflict,
                value: value.into(),
                cost: 0,
                expiration: None,
            };
            s.update(&item, &guard);
            let v = s.get(key, conflict, &guard);
            assert_eq!(v, Some(&(i + 4)))
        }
    }

    #[test]
    fn test_set_collision() {
        let collector = Collector::new();
        let guard = collector.enter();
        let mut s = Store::new();
        let value = Shared::boxed(1, &collector);

        let node = Node::new(1, 0, value, None);
        s.data.get_mut(1).unwrap().insert(1, node);
        let v = s.get(1, 1, &guard);
        assert_eq!(v, None);


        let value = Shared::boxed(2, &collector);
        let node = Node::new(1, 1, value, None);
        s.set(node, &guard);
        let v = s.get(1, 0, &guard);
        assert_ne!(v, Some(&2));
        let item = Item {
            flag: ItemNew,
            key: 1,
            conflict: 1,
            value: value.into(),
            cost: 0,
            expiration: None,
        };
        assert_eq!(s.update(&item, &guard), false);

        s.del(&1, &1, &guard);
        let v = s.get(1, 0, &guard);
        assert_eq!(v, Some(&1));
    }
}


