use std::ops::Add;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::thread;
use std::time::{Duration, Instant};
use crossbeam::epoch::{Atomic, Shared};

use memory_cache_rust::bloom::haskey::key_to_hash;

use rayon;
use rayon::prelude::*;
use memory_cache_rust::cache::{Cache, Item};
use memory_cache_rust::cache::ItemFlag::ItemUpdate;

const ITER: u64 = 32 * 1024;

#[test]
fn test_cache_key_to_hash() {
    let mut key_to_hash_count = 0;
    let mut cache = Cache::new();

    let guard = cache.guard();
    cache.set(1, 2, 1, &guard);
    cache.set(2, 2, 1, &guard);
    println!("{:?}", cache.get(&1, &guard));
    println!("{:?}", cache.get(&2, &guard));
}

#[test]
fn test_cache_key_to_hash_thread() {
    let mut key_to_hash_count = 0;


    let cache = Arc::new(Cache::new());

    let handles: Vec<_> = (0..10).map(|_| {
        let map1 = cache.clone();
        thread::spawn(move || {
            let guard = map1.guard();
            for i in 0..ITER {
                map1.set(i, i + 7, 0, &guard);
            }
        })
    }).collect();


    for h in handles {
        h.join().unwrap()
    }


    let c4 = Arc::clone(&cache);
    let guard = c4.guard();
    for i in 0..ITER {
       assert_eq!( c4.get(&i, &guard),Some(&i));
    }

}


#[test]
fn test_cache_with_ttl2() {
    let mut key_to_hash_count = 0;
    let cache = Cache::new();

    (0..ITER).into_par_iter().for_each(|i| {
        let guard = cache.guard();
        cache.set(i, i + 7, 1, &guard);
    });
}

#[test]
fn test_cache_with_ttl() {
    let mut key_to_hash_count = 0;
    let cache = Cache::new();

    let timer = timer::Timer::new();


    let guard = cache.guard();

    let key = 1;
    let value = 1;
    let cost = 1;
    let ttl = Duration::from_millis(0);

    loop {
        if !cache.set_with_ttl(1, 1, 1, Duration::from_millis(0), &guard) {
            thread::sleep(Duration::from_millis(10));
            continue;
        }
        thread::sleep(Duration::from_millis(50));
        match cache.get(&key, &guard) {
            None => {
                assert!(false)
            }
            Some(v) => {
                assert_eq!(v, &value)
            }
        }
        break;
    }


    let cache = Arc::new(Cache::new());
    let c1 = Arc::clone(&cache);


    let t1 = thread::spawn(move || {
        let guard = c1.guard();
        for i in 0..ITER {
            c1.set_with_ttl(i, i, 1, Duration::from_millis(1000), &guard);
        }
    });

    let g = {
        let c2 = Arc::clone(&cache);
        timer.schedule_repeating(chrono::Duration::milliseconds(2000), move || {
            let guard = c2.guard();
            c2.clean_up(&guard)
        })
    };

    thread::sleep(std::time::Duration::from_secs(5));
    drop(g);
    for i in 0..ITER {
        assert_eq!(cache.get(&i, &guard), None)
    }
}

#[test]
fn test_cache_with_ttl_without_timmer() {
    let cache = Arc::new(Cache::new());
    let c1 = Arc::clone(&cache);


    let t1 = thread::spawn(move || {
        let guard = c1.guard();
        for i in 0..ITER {
            let now = Instant::now().add(Duration::from_secs(200));
            c1.set_with_ttl(i, i+7, 0, now.elapsed(), &guard);
        }
    });
    t1.join().unwrap();
    thread::sleep(Duration::from_millis(2000));
    let guard = cache.guard();
    for i in 0..ITER {
        assert_eq!(cache.get(&i, &guard), Some(&(i+7)));
        // println!("{:?}",cache.get(&i, &guard));
    }
}