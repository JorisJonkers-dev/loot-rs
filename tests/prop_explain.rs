//! Property tests of `SortOutput::explanations`: every plugin whose place changed is explained by a rule that puts it
//! on the other side of another plugin than in the given order, and no other plugin is listed.
#![allow(clippy::type_complexity)]

mod common;

use common::Fixture;
use loot_rs::api::{Masterlist, PluginSlot, SortOptions, SortOutput, sort};
use loot_rs::archives::synthetic_bsa;
use loot_rs::records::synthetic_records;
use proptest::prelude::*;
use std::fmt::Write as _;

/// Per plugin: master flag, overridden base objects, archive files, group, an `after` on another plugin.
type Spec = (bool, Vec<u32>, Vec<u8>, u8, Option<usize>);

fn worlds() -> impl Strategy<Value = (Vec<Spec>, Vec<usize>)> {
    proptest::collection::vec(
        (
            proptest::bool::weighted(0.2),
            proptest::collection::vec(1u32..20, 0..6),
            proptest::collection::vec(0u8..12, 0..4),
            0u8..3,
            proptest::option::of(0usize..10),
        ),
        2..10,
    )
    .prop_flat_map(|specs| {
        let n = specs.len();
        (
            Just(specs),
            Just((0..n).collect::<Vec<usize>>()).prop_shuffle(),
        )
    })
}

fn name(index: usize, specs: &[Spec]) -> String {
    format!("P{index}.{}", if specs[index].0 { "esm" } else { "esp" })
}

fn build(specs: &[Spec], given: &[usize]) -> (Fixture, Vec<PluginSlot>, Masterlist) {
    let fixture = Fixture::new();
    let base: Vec<(u32, u32)> = (1..20).map(|object| (object, 0)).collect();
    fixture.file(
        "FalloutNV.esm",
        &synthetic_records(1, &[], &[(*b"NPC_", &base)]),
    );
    let mut yaml = String::from(
        "groups:\n  - name: default\n  - name: g1\n    after: [default]\n  - name: g2\n    after: [g1]\nplugins:\n",
    );
    for (index, (master, overrides, files, group, after)) in specs.iter().enumerate() {
        let records: Vec<(u32, u32)> = overrides.iter().map(|&object| (object, 0)).collect();
        let flags = u32::from(*master);
        fixture.file(
            &name(index, specs),
            &synthetic_records(flags, &["FalloutNV.esm"], &[(*b"NPC_", &records)]),
        );
        if !files.is_empty() {
            let names: Vec<String> = files.iter().map(|file| format!("f{file}.nif")).collect();
            let refs: Vec<&str> = names.iter().map(String::as_str).collect();
            fixture.file(
                &format!("P{index}.bsa"),
                &synthetic_bsa(104, &[("meshes", &refs)]),
            );
        }
        let group = if *group == 0 {
            "default".to_string()
        } else {
            format!("g{group}")
        };
        let _ = writeln!(
            yaml,
            "  - name: '{}'\n    group: {group}",
            name(index, specs)
        );
        // `after` only within a partition and towards a lower number, so the rules never form a cycle or an error.
        if let Some(other) = after.filter(|&other| other < index && specs[other].0 == *master) {
            let _ = writeln!(yaml, "    after: ['{}']", name(other, specs));
        }
    }
    let mut slots = vec![PluginSlot::new("FalloutNV.esm", true)];
    slots.extend(
        given
            .iter()
            .map(|&index| PluginSlot::new(name(index, specs), true)),
    );
    (fixture, slots, Masterlist::parse(&yaml).expect("parses"))
}

fn places(output: &SortOutput, slots: &[PluginSlot]) -> (Vec<String>, Vec<String>) {
    let given: Vec<String> = slots.iter().map(|slot| slot.name.clone()).collect();
    (given, output.order.clone())
}

proptest! {
    #![proptest_config(common::proptest_config(96))]

    #[test]
    fn every_moved_plugin_has_a_reason_that_puts_it_across_another(world in worlds()) {
        let (specs, given) = world;
        let (fixture, slots, masterlist) = build(&specs, &given);
        let output = sort(&slots, &masterlist, &fixture.data(), &SortOptions::default()).expect("sorts");
        let (given, sorted) = places(&output, &slots);
        let at = |list: &[String], name: &str| list.iter().position(|item| item == name).expect("listed");
        let moved: Vec<&String> = given.iter().filter(|name| at(&given, name) != at(&sorted, name)).collect();
        let explained: Vec<&String> = output.explanations.iter().map(|entry| &entry.plugin).collect();
        let mut moved_sorted = moved.clone();
        moved_sorted.sort();
        let mut explained_sorted = explained.clone();
        explained_sorted.sort();
        prop_assert_eq!(moved_sorted, explained_sorted, "exactly the plugins that moved are explained");
        for entry in &output.explanations {
            prop_assert!(!entry.reasons.is_empty(), "{} has a reason", entry.plugin);
            prop_assert_eq!((entry.from, entry.to), (at(&given, &entry.plugin), at(&sorted, &entry.plugin)));
            for reason in &entry.reasons {
                // The other plugin is on the other side now than it was in the given order.
                let (this_given, other_given) = (at(&given, &entry.plugin), at(&given, &reason.other));
                let (this_sorted, other_sorted) = (at(&sorted, &entry.plugin), at(&sorted, &reason.other));
                prop_assert_eq!(reason.later, other_sorted < this_sorted, "{:?}", reason);
                prop_assert_eq!(reason.later, other_given > this_given, "{:?}", reason);
                if let Some((before, after)) = &reason.via {
                    // The link goes against the given order and holds in the result.
                    prop_assert!(at(&given, before) > at(&given, after), "{:?}", reason);
                    prop_assert!(at(&sorted, before) < at(&sorted, after), "{:?}", reason);
                }
            }
        }
    }
}
