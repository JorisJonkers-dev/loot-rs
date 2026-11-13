//! Property-based tests of the sorter (`loot_rs::sort`) and of `loot_rs::api::sort` over synthetic plugins.
#![allow(
    clippy::format_push_string,
    clippy::type_complexity,
    clippy::case_sensitive_file_extension_comparisons
)]

mod common;

use common::Fixture;
use loot_rs::api::{Masterlist, PluginSlot, SortError, SortOptions, sort};
use loot_rs::sort::{GraphError, order};
use proptest::prelude::*;
use std::collections::HashSet;

fn graph() -> impl Strategy<Value = (usize, Vec<bool>, Vec<(usize, usize)>, Vec<(usize, usize)>)> {
    (1usize..9).prop_flat_map(|n| {
        let edges = proptest::collection::vec((0..n, 0..n), 0..14);
        (
            Just(n),
            proptest::collection::vec(any::<bool>(), n),
            edges.clone(),
            edges,
        )
    })
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

proptest! {
    #![proptest_config(common::proptest_config(256))]

    #[test]
    fn the_result_is_a_permutation_that_honours_every_kept_rule((n, masters, hard, soft) in graph()) {
        let result = order(n, &masters, &hard, &soft);
        let model_ok = acyclic(n, &hard) && hard.iter().all(|&(a, b)| masters[a] || !masters[b]);
        prop_assert_eq!(result.is_ok(), model_ok);
        if let Ok(sorted) = result {
            let mut seen = sorted.order.clone();
            seen.sort_unstable();
            prop_assert_eq!(seen, (0..n).collect::<Vec<_>>());
            let at = |node: usize| sorted.order.iter().position(|&x| x == node).expect("present");
            for &(a, b) in &hard {
                prop_assert!(at(a) < at(b), "hard edge {a}->{b}");
            }
            for &(a, b) in &soft {
                if !sorted.dropped.contains(&(a, b)) {
                    prop_assert!(at(a) < at(b), "kept soft edge {a}->{b}");
                }
            }
            for a in 0..n {
                for b in 0..n {
                    if masters[a] && !masters[b] {
                        prop_assert!(at(a) < at(b), "master {a} before {b}");
                    }
                }
            }
        }
    }

    #[test]
    fn a_dropped_soft_edge_really_could_not_be_kept((n, masters, hard, soft) in graph()) {
        if let Ok(sorted) = order(n, &masters, &hard, &soft) {
            for &(a, b) in &sorted.dropped {
                let mut edges = hard.clone();
                edges.extend(soft.iter().filter(|e| !sorted.dropped.contains(e)));
                edges.push((a, b));
                prop_assert!(
                    !acyclic(n, &edges) || (!masters[a] && masters[b]) || a == b,
                    "{a}->{b} could have been kept"
                );
            }
        }
    }

    #[test]
    fn sorting_is_deterministic_and_a_sorted_order_is_a_fixed_point((n, masters, hard, soft) in graph()) {
        if let Ok(first) = order(n, &masters, &hard, &soft) {
            prop_assert_eq!(&order(n, &masters, &hard, &soft).expect("again"), &first);
            // Renumber everything to the sorted positions: nothing moves.
            let new = |old: usize| first.order.iter().position(|&x| x == old).expect("present");
            let flags: Vec<bool> = first.order.iter().map(|&old| masters[old]).collect();
            let map = |edges: &[(usize, usize)]| -> Vec<(usize, usize)> {
                edges.iter().map(|&(a, b)| (new(a), new(b))).collect()
            };
            let kept: Vec<(usize, usize)> = soft.iter().copied().filter(|e| !first.dropped.contains(e)).collect();
            let second = order(n, &flags, &map(&hard), &map(&kept)).expect("sorts");
            prop_assert_eq!(second.order, (0..n).collect::<Vec<_>>());
        }
    }

    #[test]
    fn errors_name_edges_that_exist((n, masters, hard, soft) in graph()) {
        match order(n, &masters, &hard, &soft) {
            Err(GraphError::Cycle(a, b) | GraphError::MasterOrder(a, b)) => prop_assert!(hard.contains(&(a, b))),
            Err(other) => prop_assert!(false, "unexpected {other}"),
            Ok(_) => {}
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
