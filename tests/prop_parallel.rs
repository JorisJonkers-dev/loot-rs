//! Property tests: `api::sort` returns the same result for every thread count and with or without a `SortCache`, on
//! random synthetic worlds with records and archives that overlap.
#![allow(clippy::cast_possible_truncation, clippy::type_complexity)]

mod common;

use common::Fixture;
use loot_rs::api::{Masterlist, PluginSlot, SortCache, SortOptions, sort};
use loot_rs::archives::synthetic_bsa;
use loot_rs::records::synthetic_records;
use proptest::prelude::*;
use std::fmt::Write as _;

#[derive(Debug, Clone)]
struct World {
    /// Per mod: objects of the base master it overrides, own new records, archive files (indices into a small
    /// vocabulary), group, and an `after` on an earlier mod.
    mods: Vec<(Vec<u32>, u32, Vec<u8>, u8, Option<usize>)>,
}

fn world() -> impl Strategy<Value = World> {
    proptest::collection::vec(
        (
            proptest::collection::vec(1u32..40, 0..12),
            0u32..30,
            proptest::collection::vec(0u8..24, 0..6),
            0u8..4,
            proptest::option::of(0usize..12),
        ),
        1..12,
    )
    .prop_map(|mods| World { mods })
}

fn name(index: usize) -> String {
    format!("Mod{index}.esp")
}

fn build(world: &World) -> (Fixture, Vec<PluginSlot>, Masterlist) {
    let fixture = Fixture::new();
    let base: Vec<(u32, u32)> = (1..40).map(|object| (object, 8)).collect();
    fixture.file(
        "FalloutNV.esm",
        &synthetic_records(1, &[], &[(*b"NPC_", &base)]),
    );
    let mut slots = vec![PluginSlot::new("FalloutNV.esm", true)];
    let mut yaml = String::from(
        "groups:\n  - name: default\n  - name: g1\n    after: [default]\n  - name: g2\n    after: [g1]\n  - name: g3\n    after: [default]\nplugins:\n",
    );
    for (index, (overrides, own, files, group, after)) in world.mods.iter().enumerate() {
        let mut records: Vec<(u32, u32)> = overrides.iter().map(|&object| (object, 4)).collect();
        records.extend((1..=*own).map(|object| (1 << 24 | object, 4)));
        fixture.file(
            &name(index),
            &synthetic_records(0, &["FalloutNV.esm"], &[(*b"NPC_", &records)]),
        );
        if !files.is_empty() {
            let names: Vec<String> = files.iter().map(|file| format!("f{file}.nif")).collect();
            let refs: Vec<&str> = names.iter().map(String::as_str).collect();
            fixture.file(
                &format!("Mod{index}.bsa"),
                &synthetic_bsa(104, &[("meshes", &refs)]),
            );
        }
        let _ = writeln!(yaml, "  - name: '{}'\n    group: g{group}", name(index));
        if let Some(after) = after.filter(|&after| after < index) {
            let _ = writeln!(yaml, "    after: ['{}']", name(after));
        }
        slots.push(PluginSlot::new(name(index), true));
    }
    slots.reverse();
    let masterlist = Masterlist::parse(&yaml).expect("the generated masterlist parses");
    (fixture, slots, masterlist)
}

proptest! {
    #![proptest_config(common::proptest_config(48))]

    #[test]
    fn the_result_does_not_depend_on_the_thread_count(world in world()) {
        let (fixture, slots, masterlist) = build(&world);
        let one = sort(&slots, &masterlist, &fixture.data(), &SortOptions::default().with_threads(1));
        for threads in [2, 7] {
            let many = sort(&slots, &masterlist, &fixture.data(), &SortOptions::default().with_threads(threads));
            prop_assert_eq!(&one, &many, "{} threads", threads);
        }
    }

    #[test]
    fn a_cached_sort_returns_what_an_uncached_sort_returns(world in world()) {
        let (fixture, slots, masterlist) = build(&world);
        let plain = sort(&slots, &masterlist, &fixture.data(), &SortOptions::default());
        let cache = SortCache::new();
        let options = SortOptions::default().with_cache(cache.clone());
        let first = sort(&slots, &masterlist, &fixture.data(), &options);
        let second = sort(&slots, &masterlist, &fixture.data(), &options);
        prop_assert!(!cache.is_empty());
        prop_assert_eq!(&plain, &first);
        prop_assert_eq!(&plain, &second);
    }
}

#[test]
fn a_cached_sort_reads_a_plugin_again_once_it_changed() {
    let fixture = Fixture::new();
    let base: Vec<(u32, u32)> = (1..10).map(|object| (object, 8)).collect();
    fixture.file(
        "FalloutNV.esm",
        &synthetic_records(1, &[], &[(*b"NPC_", &base)]),
    );
    // A overrides one base record, B two: B loads first. Then A is rewritten to override three, larger than before,
    // so its size changes and the cache must read it again: A then loads first.
    let write = |file: &str, objects: &[u32]| {
        let records: Vec<(u32, u32)> = objects.iter().map(|&object| (object, 4)).collect();
        fixture.file(
            file,
            &synthetic_records(0, &["FalloutNV.esm"], &[(*b"NPC_", &records)]),
        );
    };
    write("A.esp", &[1]);
    write("B.esp", &[1, 2]);
    let slots = vec![
        PluginSlot::new("FalloutNV.esm", true),
        PluginSlot::new("A.esp", true),
        PluginSlot::new("B.esp", true),
    ];
    let masterlist = Masterlist::parse("").expect("parses");
    let options = SortOptions::default().with_cache(SortCache::new());
    let before = sort(&slots, &masterlist, &fixture.data(), &options).expect("sorts");
    assert_eq!(before.order, ["FalloutNV.esm", "B.esp", "A.esp"]);
    write("A.esp", &[1, 2, 3]);
    let after = sort(&slots, &masterlist, &fixture.data(), &options).expect("sorts");
    assert_eq!(after.order, ["FalloutNV.esm", "A.esp", "B.esp"]);
}

#[test]
fn clearing_a_cache_empties_it() {
    let fixture = Fixture::new();
    fixture.file(
        "FalloutNV.esm",
        &synthetic_records(1, &[], &[(*b"NPC_", &[(1, 4)])]),
    );
    let cache = SortCache::new();
    let slots = vec![PluginSlot::new("FalloutNV.esm", true)];
    let masterlist = Masterlist::parse("").expect("parses");
    sort(
        &slots,
        &masterlist,
        &fixture.data(),
        &SortOptions::default().with_cache(cache.clone()),
    )
    .expect("sorts");
    assert_eq!(cache.len(), (1, 0));
    cache.clear();
    assert!(cache.is_empty());
}

#[test]
fn two_threads_sort_against_one_masterlist_and_one_cache_at_once() {
    let fixture = Fixture::new();
    fixture
        .plugin("FalloutNV.esm", &[])
        .plugin("A.esp", &["FalloutNV.esm"]);
    let masterlist = Masterlist::parse("").expect("parses");
    let cache = SortCache::new();
    let slots = vec![
        PluginSlot::new("A.esp", true),
        PluginSlot::new("FalloutNV.esm", true),
    ];
    let data = fixture.data();
    let orders: Vec<Vec<String>> = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..2)
            .map(|_| {
                scope.spawn(|| {
                    let options = SortOptions::default().with_cache(cache.clone());
                    sort(&slots, &masterlist, &data, &options)
                        .expect("sorts")
                        .order
                })
            })
            .collect();
        workers
            .into_iter()
            .map(|worker| worker.join().expect("joins"))
            .collect()
    });
    assert_eq!(orders[0], ["FalloutNV.esm", "A.esp"]);
    assert_eq!(orders[0], orders[1]);
}

#[test]
fn the_masterlist_the_options_and_the_cache_can_be_shared_between_threads() {
    fn shared<T: Send + Sync>() {}
    shared::<Masterlist>();
    shared::<SortOptions>();
    shared::<SortCache>();
}
