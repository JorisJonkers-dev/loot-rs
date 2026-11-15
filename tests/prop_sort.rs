//! Property-based tests of the sorter (`loot_rs::sort`) and of `loot_rs::api::sort` over synthetic plugins.
#![allow(
    clippy::format_push_string,
    clippy::type_complexity,
    clippy::case_sensitive_file_extension_comparisons
)]

mod common;

use common::Fixture;
use loot_rs::api::{Masterlist, PluginSlot, SortError, SortOptions, sort};
use loot_rs::sort::{
    EdgeType, GraphError, GroupInput, Overlaps, PluginInput, Sorted, sort as sort_plugins,
};
use proptest::prelude::*;
use std::collections::HashSet;

/// A random set of plugins `p0..` with their metadata and overlap data, over ids (not positions), so that the
/// same plugins can be listed in any order.
#[derive(Debug, Clone)]
struct World {
    n: usize,
    is_master: Vec<bool>,
    group: Vec<usize>,
    masters: Vec<(usize, usize)>,
    requirements: Vec<(usize, usize)>,
    load_after: Vec<(usize, usize)>,
    records: Vec<usize>,
    assets: Vec<usize>,
    record_overlap: Vec<Vec<bool>>,
    asset_overlap: Vec<Vec<bool>>,
}

fn world(max: usize, edges: usize) -> impl Strategy<Value = World> {
    (1usize..max).prop_flat_map(move |n| {
        let pairs = proptest::collection::vec((0..n, 0..n), 0..edges);
        let matrix = proptest::collection::vec(proptest::collection::vec(any::<bool>(), n), n);
        (
            Just(n),
            proptest::collection::vec(any::<bool>(), n),
            proptest::collection::vec(0usize..4, n),
            (pairs.clone(), pairs.clone(), pairs),
            (
                proptest::collection::vec(0usize..4, n),
                proptest::collection::vec(0usize..4, n),
            ),
            (matrix.clone(), matrix),
        )
            .prop_map(
                |(
                    n,
                    is_master,
                    group,
                    (masters, requirements, load_after),
                    (records, assets),
                    (rec, ast),
                )| {
                    let symmetric = |m: Vec<Vec<bool>>| -> Vec<Vec<bool>> {
                        (0..n)
                            .map(|a| (0..n).map(|b| m[a][b] || m[b][a]).collect())
                            .collect()
                    };
                    World {
                        n,
                        is_master,
                        group,
                        masters,
                        requirements,
                        load_after,
                        records,
                        assets,
                        record_overlap: symmetric(rec),
                        asset_overlap: symmetric(ast),
                    }
                },
            )
    })
}

fn groups() -> Vec<GroupInput> {
    let group = |name: &str, after: &[&str]| GroupInput {
        name: name.to_string(),
        after: after.iter().map(ToString::to_string).collect(),
    };
    vec![
        group("A", &[]),
        group("B", &["A"]),
        group("C", &["A", "B"]),
        group("default", &["C"]),
    ]
}

fn group_name(index: usize) -> &'static str {
    ["default", "A", "B", "C"][index]
}

struct WorldOverlaps<'a> {
    world: &'a World,
    ids: &'a [usize],
}

impl Overlaps for WorldOverlaps<'_> {
    fn records(&self, a: usize, b: usize) -> bool {
        self.world.record_overlap[self.ids[a]][self.ids[b]]
    }
    fn assets(&self, a: usize, b: usize) -> bool {
        self.world.asset_overlap[self.ids[a]][self.ids[b]]
    }
}

fn inputs(world: &World, ids: &[usize]) -> Vec<PluginInput> {
    let name = |id: usize| format!("p{id}.{}", if world.is_master[id] { "esm" } else { "esp" });
    let listed = |pairs: &[(usize, usize)], id: usize| -> Vec<String> {
        pairs
            .iter()
            .filter(|&&(plugin, _)| plugin == id)
            .map(|&(_, other)| name(other))
            .collect()
    };
    ids.iter()
        .map(|&id| PluginInput {
            name: name(id),
            is_master: world.is_master[id],
            masters: listed(&world.masters, id),
            requirements: listed(&world.requirements, id),
            load_after: listed(&world.load_after, id),
            group: group_name(world.group[id]).to_string(),
            override_records: world.records[id],
            assets: world.assets[id],
            ..PluginInput::default()
        })
        .collect()
}

fn run(world: &World, ids: &[usize]) -> Result<Sorted, GraphError> {
    let plugins = inputs(world, ids);
    sort_plugins(
        &plugins,
        &groups(),
        &["p0.esm".to_string()],
        &WorldOverlaps { world, ids },
    )
}

/// Independent model: is the graph of `edges` over `n` nodes acyclic?
fn acyclic(n: usize, edges: &[(usize, usize)]) -> bool {
    let mut remaining: Vec<usize> = (0..n).collect();
    let mut live: Vec<(usize, usize)> = edges.to_vec();
    loop {
        let free: Vec<usize> = remaining
            .iter()
            .copied()
            .filter(|node| live.iter().all(|&(_, to)| to != *node))
            .collect();
        if free.is_empty() {
            return remaining.is_empty();
        }
        remaining.retain(|node| !free.contains(node));
        live.retain(|&(from, _)| !free.contains(&from));
    }
}

/// Every rule `(earlier, later)` of the world's metadata.
fn rules(world: &World) -> Vec<(usize, usize)> {
    world
        .masters
        .iter()
        .chain(&world.requirements)
        .chain(&world.load_after)
        .map(|&(plugin, other)| (other, plugin))
        .collect()
}

proptest! {
    #![proptest_config(common::proptest_config(256))]

    #[test]
    fn the_result_is_a_permutation_that_honours_every_edge_of_the_graph(world in world(9, 14)) {
        let ids: Vec<usize> = (0..world.n).collect();
        let rules = rules(&world);
        let same_kind: Vec<(usize, usize)> = rules.iter().copied().filter(|&(a, b)| world.is_master[a] == world.is_master[b]).collect();
        let cross = rules.iter().any(|&(a, b)| !world.is_master[a] && world.is_master[b]);
        // `p0.esm` (when p0 is a master) is the plugin the game loads first: before every other master.
        let mut edges = same_kind.clone();
        if world.is_master[0] {
            edges.extend((1..world.n).filter(|&b| world.is_master[b]).map(|b| (0, b)));
        }
        let model_ok = !cross && acyclic(world.n, &edges);
        match run(&world, &ids) {
            Ok(sorted) => {
                prop_assert!(model_ok, "the model says this is an error");
                let mut seen = sorted.order.clone();
                seen.sort_unstable();
                prop_assert_eq!(seen, ids.clone());
                let at = |node: usize| sorted.order.iter().position(|&x| x == node).expect("present");
                for edge in &sorted.edges {
                    prop_assert!(at(edge.from) < at(edge.to), "{:?} edge {}->{}", edge.kind, edge.from, edge.to);
                }
                for &(a, b) in &same_kind {
                    prop_assert!(at(a) < at(b), "rule {a}->{b}");
                }
                for a in 0..world.n {
                    for b in 0..world.n {
                        if world.is_master[a] && !world.is_master[b] {
                            prop_assert!(at(a) < at(b), "master {a} before {b}");
                        }
                    }
                }
                prop_assert!(sorted.unique, "more than one order is left");
                for edge in &sorted.skipped {
                    prop_assert!(matches!(edge.kind, EdgeType::MasterlistGroup | EdgeType::RecordOverlap | EdgeType::AssetOverlap));
                }
            }
            Err(GraphError::Cycle(cycle)) => {
                prop_assert!(!cycle.is_empty() && cycle.iter().all(|&(plugin, _)| plugin < world.n));
                prop_assert!(!model_ok);
            }
            Err(GraphError::MasterOrder { plugin, master, .. }) => {
                prop_assert!(plugin < world.n && master < world.n);
                prop_assert!(!world.is_master[plugin] && world.is_master[master]);
                prop_assert!(!model_ok);
            }
            Err(other) => prop_assert!(false, "unexpected {other}"),
        }
    }

    #[test]
    fn sorting_is_deterministic_and_a_sorted_order_is_a_fixed_point(world in world(9, 14), shuffle in any::<u64>()) {
        let mut ids: Vec<usize> = (0..world.n).collect();
        // A deterministic shuffle from the seed.
        let mut state = shuffle | 1;
        for i in (1..ids.len()).rev() {
            state = state.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
            ids.swap(i, (state >> 33) as usize % (i + 1));
        }
        if let Ok(first) = run(&world, &ids) {
            prop_assert_eq!(&run(&world, &ids).expect("again"), &first);
            let sorted_ids: Vec<usize> = first.order.iter().map(|&position| ids[position]).collect();
            let second = run(&world, &sorted_ids).expect("sorts its own output");
            prop_assert_eq!(second.order, (0..world.n).collect::<Vec<_>>());
            // The result does not depend on how the same plugins were first listed, except through ties:
            // the metadata rules hold either way.
            let third = run(&world, &(0..world.n).collect::<Vec<_>>());
            prop_assert!(third.is_ok());
        }
    }

    #[test]
    fn the_work_is_bounded_for_larger_inputs(world in world(40, 120)) {
        let ids: Vec<usize> = (0..world.n).collect();
        // Any answer is fine; it has to come, and an order has to be a permutation.
        if let Ok(sorted) = run(&world, &ids) {
            let mut seen = sorted.order.clone();
            seen.sort_unstable();
            prop_assert_eq!(seen, ids);
            prop_assert!(sorted.unique);
        }
    }
}

proptest! {
    #![proptest_config(common::proptest_config(128))]

    #[test]
    fn the_order_follows_the_overlap_when_nothing_else_speaks(
        records in proptest::collection::vec(0usize..5, 2..7),
    ) {
        let n = records.len();
        let world = World {
            n,
            is_master: vec![false; n],
            group: vec![0; n],
            masters: vec![],
            requirements: vec![],
            load_after: vec![],
            records: records.clone(),
            assets: vec![0; n],
            record_overlap: vec![vec![true; n]; n],
            asset_overlap: vec![vec![false; n]; n],
        };
        let ids: Vec<usize> = (0..n).collect();
        let sorted = run(&world, &ids).expect("sorts");
        // Every pair that overlaps with different counts: the one with more records loads first. libloot looks at a
        // pair from the plugin that comes first by name, and skips that plugin when it changes no records, so a
        // pair whose first plugin has none is left to the load order.
        for (a, &count_a) in records.iter().enumerate() {
            for (b, &count_b) in records.iter().enumerate() {
                let outer_has_records = records[a.min(b)] > 0;
                if count_a > count_b && outer_has_records {
                    let at = |node: usize| sorted.order.iter().position(|&x| x == node).expect("present");
                    prop_assert!(at(a) < at(b), "{a} ({count_a} records) before {b} ({count_b})");
                }
            }
        }
    }
}

const NAMES: [&str; 6] = ["a.esp", "b.esp", "c.esm", "d.esp", "e.esm", "f.esp"];

proptest! {
    #![proptest_config(common::proptest_config(64))]

    // Plugins with random masters, `after` rules and groups, in a random order: whatever the answer, it is either an
    // error that names plugins of the load order or an order that holds every rule and is stable.
    #[test]
    fn the_library_sorts_synthetic_load_orders(
        listed in proptest::sample::subsequence(NAMES.to_vec(), 1..=6).prop_shuffle(),
        masters in proptest::collection::vec((0usize..6, 0usize..6), 0..4),
        afters in proptest::collection::vec((0usize..6, 0usize..6), 0..4),
        groups in proptest::collection::vec(0usize..3, 6),
    ) {
        let fixture = Fixture::new();
        for (index, name) in NAMES.iter().enumerate() {
            let listed_masters: Vec<&str> = masters
                .iter()
                .filter(|&&(plugin, master)| plugin == index && master != index)
                .map(|&(_, master)| NAMES[master])
                .collect();
            fixture.plugin(name, &listed_masters);
        }
        let mut yaml = String::from("groups:\n  - name: G1\n  - name: G2\n    after: [G1]\nplugins:\n");
        for (index, name) in NAMES.iter().enumerate() {
            yaml += &format!("  - name: {name}\n");
            if groups[index] > 0 {
                yaml += &format!("    group: G{}\n", groups[index]);
            }
            let after: Vec<&str> = afters
                .iter()
                .filter(|&&(plugin, other)| plugin == index && other != index)
                .map(|&(_, other)| NAMES[other])
                .collect();
            if !after.is_empty() {
                yaml += &format!("    after: [{}]\n", after.join(", "));
            }
        }
        let masterlist = Masterlist::parse(&yaml).expect("generated masterlist");
        let slots: Vec<PluginSlot> = listed.iter().map(|name| PluginSlot::new(*name, true)).collect();
        match sort(&slots, &masterlist, &fixture.data(), &SortOptions::default()) {
            Err(SortError::Conflict { first, second }) => {
                prop_assert!(listed.contains(&first.as_str()) && listed.contains(&second.as_str()));
            }
            Err(SortError::MasterOrder { plugin, master }) => {
                prop_assert!(listed.contains(&plugin.as_str()) && listed.contains(&master.as_str()));
            }
            Err(other) => prop_assert!(false, "unexpected {other}"),
            Ok(result) => {
                let got: HashSet<&str> = result.order.iter().map(String::as_str).collect();
                prop_assert_eq!(got, listed.iter().copied().collect::<HashSet<_>>());
                let at = |name: &str| result.order.iter().position(|n| n == name).expect("present");
                for &(plugin, master) in &masters {
                    if plugin != master && listed.contains(&NAMES[plugin]) && listed.contains(&NAMES[master]) {
                        prop_assert!(at(NAMES[master]) < at(NAMES[plugin]), "master {} before {}", NAMES[master], NAMES[plugin]);
                    }
                }
                for &(plugin, other) in &afters {
                    if plugin != other && listed.contains(&NAMES[plugin]) && listed.contains(&NAMES[other]) {
                        prop_assert!(at(NAMES[other]) < at(NAMES[plugin]), "{} after {}", NAMES[plugin], NAMES[other]);
                    }
                }
                let first_plain = result.order.iter().position(|n| !n.ends_with(".esm")).unwrap_or(result.order.len());
                prop_assert!(result.order[first_plain..].iter().all(|n| !n.ends_with(".esm")), "masters first: {:?}", result.order);
                let again: Vec<PluginSlot> = result.order.iter().map(|name| PluginSlot::new(name.clone(), true)).collect();
                let second = sort(&again, &masterlist, &fixture.data(), &SortOptions::default()).expect("sorts again");
                prop_assert_eq!(second.order, result.order);
            }
        }
    }
}
