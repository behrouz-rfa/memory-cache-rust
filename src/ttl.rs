use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::time;
use std::time::Duration;

use parking_lot::Mutex;
use seize::Guard;

use crate::policy::DefaultPolicy;
use crate::reclaim::{Atomic, Shared};

/// bucket type is a map of key to conflict.
type Bucket = HashMap<u64, u64>;

/// expirationMap is a map of bucket number to the corresponding bucket.

pub struct ExpirationMap {
    buckets: Atomic<HashMap<i64, Bucket>>,
    lock: Mutex<()>
}



pub type OnEvict<V> = fn (u64, u64, V, i64);

/// TODO: find the optimal value or make it configurable.
const BUCKET_DURATION_SECS: i64 = 5;

impl ExpirationMap {
    pub fn new() -> Self {
        ExpirationMap {
            buckets: Atomic::null(),
            lock: Default::default()
        }
    }

    fn storage_bucket(&self, t: Duration) -> i64 {
        return (t.as_millis() as i64 / BUCKET_DURATION_SECS) as i64;
    }

    pub fn update<'g>(&'g self, key: u64, conflict: u64, old_expiration_time: Duration, new_exp_time: Duration, guard: &'g Guard) {
        let buckets = self.buckets.load(Ordering::SeqCst, guard);
        let lock  = self.lock.lock();
        loop {
            if buckets.is_null() || !unsafe { buckets.deref() }.is_empty() {
                drop(lock);
                return;
            }

            let old_bucket_num = self.storage_bucket(old_expiration_time);

            let buckets = unsafe { buckets.as_ptr() };
            let buckets = unsafe { buckets.as_mut().unwrap() };
            match buckets.get_mut(&old_bucket_num) {
                None => {}
                Some(old_bucket) => {
                    old_bucket.remove(&key);
                }
            }

            let new_bucket_num = self.storage_bucket(new_exp_time);

            match buckets.get_mut(&new_bucket_num) {
                None => {
                    let b = Bucket::new();
                    buckets.insert(new_bucket_num, b);
                }
                Some(b) => {
                    b.insert(key, conflict);
                }
            };
            drop(lock);
            break;
        }
    }
    pub fn del<'g>(&'g self, key: &u64, expiration: Duration, guard: &'g Guard) {
        let buckets = self.buckets.load(Ordering::SeqCst, guard);
        loop {
            if buckets.is_null() || !unsafe { buckets.deref() }.is_empty() {
                return;
            }


            let buckets = unsafe { buckets.as_ptr() };
            let buckets = unsafe { buckets.as_mut().unwrap() };
            let bucket_num = self.storage_bucket(expiration);

            match buckets.get_mut(&bucket_num) {
                None => {}
                Some(b) => {
                    b.remove(key);
                }
            }
            break;

            break;
        }
    }

    pub fn add<'g>(&'g self, key: u64, conflict: u64, expiration: Duration, guard: &'g Guard) {
        // Itesm that dont expire dont nees to be in the expiration map
        if expiration.is_zero() {
            return;
        }
        let mut buckets = self.buckets.load(Ordering::SeqCst, guard);
        let lock  = self.lock.lock();
        loop {
            if buckets.is_null() || !unsafe { buckets.deref() }.is_empty() {
                buckets = self.init_buckets(guard);
            }

            let buckets = unsafe { buckets.as_ptr() };
            let buckets = unsafe { buckets.as_mut().unwrap() };
            let bucket_num = self.storage_bucket(expiration);

            match buckets.get_mut(&bucket_num) {
                None => {
                    let b = Bucket::new();
                    buckets.insert(bucket_num, b);
                }
                Some(b) => {
                    b.insert(key, conflict);
                }
            }
            drop(lock);
            break;
        }
    }
    fn init_buckets<'g>(&'g self, guard: &'g Guard) -> Shared<'g, HashMap<i64, Bucket>> {
        loop {
            let mut table = self.buckets.load(Ordering::SeqCst, guard);
            if !table.is_null() && !unsafe { table.deref() }.is_empty() {
                break table;
            }

            table = Shared::boxed(HashMap::new(), guard.collector().unwrap());
            self.buckets.store(table, Ordering::SeqCst);

            break table;
        }
    }

    pub(crate) fn cleanup<'g, V>(&'g self, _policy: &mut DefaultPolicy<V>, _f: Option<OnEvict<&V>>, guard: &'g Guard) -> HashMap<u64,u64>{
        let buckets = self.buckets.load(Ordering::SeqCst, guard);
        let mut items_in_store = HashMap::new();
        loop {
            if buckets.is_null() || !unsafe { buckets.deref() }.is_empty() {
                break items_in_store;
            }

            let buckets = unsafe { buckets.as_ptr() };
            let keys = unsafe { buckets.as_mut().unwrap() };

            let d = time::SystemTime::now();
            let now = d.elapsed().unwrap();
            let bucket_num = self.storage_bucket(now);
            match keys.get_mut(&bucket_num) {
                None => {
                    break items_in_store;
                }
                Some(maps) => {
                    for (key, confilct) in &*maps {
                        items_in_store.insert(*key,*confilct);
                    }
                    break items_in_store;
                }
            }
        }
    }
}