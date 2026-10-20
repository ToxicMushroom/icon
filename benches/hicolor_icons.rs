//! Benchmarks of finding icons on this system, looking them up in the `hicolor` theme.
//! 
//! The current benchmarks are host-environment dependent.

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use icon::{Icons, IconsCache};
use std::hint::black_box;

/// The size and scale icons are looked up at.
const SIZE: u32 = 32;
const SCALE: u32 = 1;
/// The theme icons are looked up in.
const THEME: &str = "hicolor";
/// How many distinct icon names to look up in the lookup benchmarks.
const LOOKUPS: usize = 200;

/// A deterministic (for a given system) selection of names of icons in [THEME], plus some names
/// that don't exist, as those are looked up too in practice.
fn icon_names(icons: &Icons) -> Vec<String> {
    let mut names: Vec<String> = icons
        .find_all_icons_filtered(|theme| theme.info.internal_name == THEME, |_| true, |_| true)
        .map(|(_, _, icon)| icon.icon_name().to_string())
        .collect();
    names.sort_unstable();
    names.dedup();
    assert!(!names.is_empty(), "no icons found in {THEME}; is it installed?");

    let step = names.len().div_ceil(LOOKUPS).max(1);
    let mut names: Vec<String> = names.into_iter().step_by(step).collect();
    names.extend((0..names.len() / 10).map(|i| format!("this-icon-does-not-exist-{i}")));
    names
}

fn cache(c: &mut Criterion) {
    let mut group = c.benchmark_group("cache");
    group.sample_size(20);

    // applications that only look up icons in one theme (and its parents) can index only those;
    // see `all_themes.rs` for indexing all of them
    group.bench_function("pre_populate_cache_one_theme", |b| {
        b.iter_batched(
            || {
                let mut icons = Icons::new();
                icons.themes.retain(|name, _| name == THEME);
                IconsCache::from(icons)
            },
            |mut cache| {
                cache.pre_populate_cache();
                cache
            },
            BatchSize::PerIteration,
        )
    });

    group.finish();
}

fn find_icon(c: &mut Criterion) {
    let icons = Icons::new();
    let names = icon_names(&icons);

    let mut group = c.benchmark_group("find_icon");

    group.bench_function("uncached", |b| {
        b.iter(|| {
            for name in &names {
                black_box(icons.find_icon(name, SIZE, SCALE, THEME));
            }
        })
    });

    group.bench_function("cache_cold", |b| {
        b.iter_batched(
            || IconsCache::from(Icons::new()),
            |mut cache| {
                for name in &names {
                    black_box(cache.find_icon(name, SIZE, SCALE, THEME));
                }
                cache
            },
            BatchSize::PerIteration,
        )
    });

    let mut warm = IconsCache::from(Icons::new());
    warm.pre_populate_cache();
    group.bench_function("cache_pre_populated", |b| {
        b.iter(|| {
            for name in &names {
                black_box(warm.find_icon(name, SIZE, SCALE, THEME));
            }
        })
    });

    group.bench_function("cached_only", |b| {
        b.iter(|| {
            for name in &names {
                black_box(warm.find_cached_icon(name, SIZE, SCALE, THEME));
            }
        })
    });

    group.finish();
}

criterion_group!(benches, cache, find_icon);
criterion_main!(benches);
