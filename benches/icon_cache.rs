use std::{hint::black_box};
use criterion::{Criterion, criterion_group, criterion_main};
use futures::executor::block_on;
use icon::IconSearch;

fn bench_system_cache_populate() {
    let mut icons = IconSearch::new().search().icons_cached();
    block_on(black_box(icons.pre_populate_cache()));
}

fn criterion_benchmark(c: &mut Criterion) {
    c.bench_function("System icon cache populating", |b| b.iter(|| bench_system_cache_populate()));
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);