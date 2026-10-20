//! Benchmarks of finding and indexing all icon themes on this system.
//!
//! The current benchmarks are host-environment dependent.

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use icon::{IconSearch, Icons, IconsCache};

fn search(c: &mut Criterion) {
    let mut group = c.benchmark_group("search");
    group.sample_size(20);

    group.bench_function("icons", |b| b.iter(|| IconSearch::new().search().icons()));

    group.finish();
}

fn cache(c: &mut Criterion) {
    let mut group = c.benchmark_group("cache");
    group.sample_size(20);

    group.bench_function("pre_populate_cache", |b| {
        b.iter_batched(
            || IconsCache::from(Icons::new()),
            |mut cache| {
                cache.pre_populate_cache();
                cache
            },
            BatchSize::PerIteration,
        )
    });

    group.finish();
}

criterion_group!(benches, search, cache);
criterion_main!(benches);
