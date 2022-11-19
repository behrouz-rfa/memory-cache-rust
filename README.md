# memory-cache-rust

memory-cache is a fast, concurrent cache library built with a focus on performance and correctness.

The motivation to build Ristretto comes from the need for a contention-free
cache in [Dgraph][].

[Dgraph]: https://github.com/dgraph-io/dgraph

## Features

* **High Hit Ratios** - with our unique admission/eviction policy pairing, Ristretto's performance is best in class.
    * **Eviction: SampledLFU** - on par with exact LRU and better performance on Search and Database traces.
    * **Admission: TinyLFU** - extra performance with little memory overhead (12 bits per counter).
* **Fast Throughput** - we use a variety of techniques for managing contention and the result is excellent throughput.
* **Cost-Based Eviction** - any large new item deemed valuable can evict multiple smaller items (cost could be anything).
* **Fully Concurrent** - you can use as many goroutines as you want with little throughput degradation.
* **Metrics** - optional performance metrics for throughput, hit ratios, and other stats.
* **Simple API** - just figure out your ideal `Config` values and you're off and running.

## Status

Ristretto is usable but still under active development. We expect it to be production ready in the near future.


## Usage

### Example

```rust
fn main() {
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
    if cache.set("key", "value", 1, &guard) {
        thread::sleep(Duration::from_millis(2000));
        println!("{:?}", cache.get("key", &guard));
    }
}
```

### Config

The `Config` struct is passed to `NewCache` when creating Ristretto instances (see the example above).

**NumCounters** `int64`

NumCounters is the number of 4-bit access counters to keep for admission and eviction. We've seen good performance in setting this to 10x the number of items you expect to keep in the cache when full.

For example, if you expect each item to have a cost of 1 and MaxCost is 100, set NumCounters to 1,000. Or, if you use variable cost values but expect the cache to hold around 10,000 items when full, set NumCounters to 100,000. The important thing is the *number of unique items* in the full cache, not necessarily the MaxCost value.

**MaxCost** `int64`

MaxCost is how eviction decisions are made. For example, if MaxCost is 100 and a new item with a cost of 1 increases total cache cost to 101, 1 item will be evicted.

MaxCost can also be used to denote the max size in bytes. For example, if MaxCost is 1,000,000 (1MB) and the cache is full with 1,000 1KB items, a new item (that's accepted) would cause 5 1KB items to be evicted.

MaxCost could be anything as long as it matches how you're using the cost values when calling Set.

**BufferItems** `int64`

BufferItems is the size of the Get buffers. The best value we've found for this is 64.

If for some reason you see Get performance decreasing with lots of contention (you shouldn't), try increasing this value in increments of 64. This is a fine-tuning mechanism and you probably won't have to touch this.

**Metrics** `bool`

Metrics is true when you want real-time logging of a variety of stats. The reason this is a Config flag is because there's a 10% throughput performance overhead.

**OnEvict** `func(hashes [2]uint64, value interface{}, cost int64)`

OnEvict is called for every eviction.

**KeyToHash** `func(key interface{}) [2]uint64`

KeyToHash is the hashing algorithm used for every key. If this is nil, Ristretto has a variety of [defaults depending on the underlying interface type](https://github.com/dgraph-io/ristretto/blob/master/z/z.go#L19-L41).

Note that if you want 128bit hashes you should use the full `[2]uint64`,
otherwise just fill the `uint64` at the `0` position and it will behave like
any 64bit hash.

**Cost** `func(value interface{}) int64`

Cost is an optional function you can pass to the Config in order to evaluate
item cost at runtime, and only for the Set calls that aren't dropped (this is
useful if calculating item cost is particularly expensive and you don't want to
waste time on items that will be dropped anyways).

To signal to Ristretto that you'd like to use this Cost function:

1. Set the Cost field to a non-nil function.
2. When calling Set for new items or item updates, use a `cost` of 0.
