use std::thread;
use std::time::Duration;
use memory_cache_rust::bloom::hasher::key_to_hash;
use memory_cache_rust::cache::{Cache, Config};

#[test]
fn test_cache() {
    let mut key_to_hash_count = 0;
    let mut cache = Cache::new(
        Config {
            numb_counters: 100,
            max_cost: 10,
            buffer_items: 64,
            metrics: false,
            key_to_hash: key_to_hash,
            on_evict: None,
            cost: None,
        }
    );

    let guard = crossbeam::epoch::pin();
    cache.set("key", "value", 1, &guard);
    thread::sleep(Duration::from_millis(10));
    let v =  cache.get("key",&guard);
    assert_eq!(v,Some(&"value"));

    cache.del("key");
    thread::sleep(Duration::from_millis(10));
    let v =  cache.get("key",&guard);
    assert_eq!(v,None);
}