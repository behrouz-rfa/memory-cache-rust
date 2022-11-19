use std::thread;
use std::time::Duration;
use memory_cache_rust::bloom::hasher::{key_to_hash, cast_mut, value_to_int};
use memory_cache_rust::cache::{Cache, Config, Item, ITEM_DELETE, ITEM_NEW, ITEM_UPDATE};
use memory_cache_rust::store::Store;

#[test]
fn test_cache() {
    let mut key_to_hash_count = 0;
    let mut cache = Cache::new(
        Config {
            numb_counters: 1e7 as i64, // maximum cost of cache (1GB).
            max_cost: 1 << 30,// maximum cost of cache (1GB).
            buffer_items: 64,// number of keys per Get buffer.
            metrics: false,
            key_to_hash: key_to_hash,
            on_evict: None,
            cost: None,
        }
    );

    let guard = crossbeam::epoch::pin();
    cache.set("key", "value", 1, &guard);
    thread::sleep(Duration::from_millis(10));
    let v = cache.get("key", &guard);
    assert_eq!(v, Some(&"value"));

    cache.del("key");
    thread::sleep(Duration::from_millis(10));
    let v = cache.get("key", &guard);
    assert_eq!(v, None);
}

fn evicted(key: u64, conflict: u64, value: i64, cost: i64) {
    println!("evicted")
}

#[test]
fn test_cache_process_items() {
    let mut key_to_hash_count = 0;
    let mut cache = Cache::new(
        Config {
            numb_counters: 1e7 as i64, // maximum cost of cache (1GB).
            max_cost: 1 << 30,// maximum cost of cache (1GB).
            buffer_items: 64,// number of keys per Get buffer.
            metrics: false,
            key_to_hash: key_to_hash,
            on_evict: Some(evicted),
            cost: Some(value_to_int),
        }
    );

    let guard = crossbeam::epoch::pin();

    let mut key = 0;
    let mut conflict = 0;


   /* (key, conflict) = key_to_hash(1);
    cache.set_buf.send(Item {
        flag: ITEM_NEW,
        key,
        conflict,
        value: Some(1),
        cost: 0,
    });

    thread::sleep(Duration::from_millis(100));
    assert_eq!(cache.policy.has(key, &guard), true);
    assert_eq!(cache.policy.cost(key, &guard), 1);

    (key, conflict) = key_to_hash(1);

    cache.set_buf.send(Item {
        flag: ITEM_UPDATE,
        key,
        conflict,
        value: Some(2),
        cost: 0,
    });
    thread::sleep(Duration::from_millis(100));
    assert_eq!(cache.policy.cost(key, &guard), 2);

    (key, conflict) = key_to_hash(1);
    cache.set_buf.send(Item {
        flag: ITEM_DELETE,
        key,
        conflict,
        value: None,
        cost: 0,
    });

    thread::sleep(Duration::from_millis(100));
    (key, conflict) = key_to_hash(1);
    let v = cache.store.Get(key,conflict,&guard);
    assert_eq!(v,None);
    assert_eq!(cache.policy.has(key, &guard), false);
*/

    (key, conflict) = key_to_hash(2);
    cache.set_buf.send(Item {
        flag: ITEM_NEW,
        key,
        conflict,
        value: Some(2),
        cost: 3,
    });

    (key, conflict) = key_to_hash(3);
    cache.set_buf.send(Item {
        flag: ITEM_NEW,
        key,
        conflict,
        value: Some(2),
        cost: 3,
    });
    thread::sleep(Duration::from_millis(100));
    (key, conflict) = key_to_hash(4);
    cache.set_buf.send(Item {
        flag: ITEM_NEW,
        key,
        conflict,
        value: Some(3),
        cost: 3,
    });
    thread::sleep(Duration::from_millis(100));
    (key, conflict) = key_to_hash(5);
    cache.set_buf.send(Item {
        flag: ITEM_NEW,
        key,
        conflict,
        value: Some(3),
        cost: 5,
    });
    thread::sleep(Duration::from_millis(100));

    println!("{:?}",cache.get(2,&guard));
    cache.set(1, 1, 1, &guard);
}