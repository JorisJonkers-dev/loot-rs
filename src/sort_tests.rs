// SPDX-License-Identifier: GPL-3.0-or-later
//
// Several tests are translated from libloot's tests of `sort_plugins` (https://github.com/loot/libloot,
// `src/sorting/plugins.rs`, GPL-3.0-or-later, Copyright (C) the libloot authors).

//! Unit tests of `sort`: the algorithm on plain inputs.

use super::*;
use crate::tally::{covers, ids};

fn plugin(name: &str, group: &str) -> PluginInput {
    let mut plugin = PluginInput::new(name);
    plugin.group = group.to_string();
    plugin
}

fn groups() -> Vec<GroupInput> {
    vec![
        GroupInput {
            name: "A".into(),
            after: vec![],
        },
        GroupInput {
            name: "B".into(),
            after: vec!["A".into()],
        },
        GroupInput {
            name: "default".into(),
            after: vec!["B".into()],
        },
    ]
}

fn names(plugins: &[PluginInput], sorted: &Sorted) -> Vec<String> {
    sorted
        .order
        .iter()
        .map(|&index| plugins[index].name.clone())
        .collect()
}

fn run(plugins: &[PluginInput]) -> Result<Sorted, GraphError> {
    sort(plugins, &groups(), &[], &NoOverlaps)
}

#[test]
fn nothing_to_sort_is_an_empty_order() {
    let sorted = run(&[]).expect("sorts");
    assert_eq!(sorted.order, Vec::<usize>::new());
    assert!(sorted.unique);
}

#[test]
fn without_rules_the_given_order_is_kept() {
    let plugins: Vec<_> = ["c.esp", "a.esp", "b.esp"]
        .iter()
        .map(|name| PluginInput::new(name))
        .collect();
    let sorted = run(&plugins).expect("sorts");
    assert_eq!(names(&plugins, &sorted), ["c.esp", "a.esp", "b.esp"]);
    assert!(sorted.unique);
}

#[test]
fn sorting_its_own_output_changes_nothing() {
    let plugins = vec![plugin("B.esp", "B"), plugin("A.esp", "A")];
    let first = run(&plugins).expect("sorts");
    assert_eq!(names(&plugins, &first), ["A.esp", "B.esp"]);
    let again: Vec<_> = first.order.iter().map(|&i| plugins[i].clone()).collect();
    let second = run(&again).expect("sorts");
    assert_eq!(second.order, [0, 1]);
}

#[test]
fn masters_load_before_the_others_and_each_group_keeps_its_order() {
    covers(&[ids::sorting::stages::SEPARATE_GRAPHS]);
    let mut master = PluginInput::new("M.esm");
    master.is_master = true;
    let mut other_master = PluginInput::new("N.esm");
    other_master.is_master = true;
    let plugins = vec![
        PluginInput::new("z.esp"),
        other_master,
        PluginInput::new("y.esp"),
        master,
    ];
    let sorted = run(&plugins).expect("sorts");
    assert_eq!(
        names(&plugins, &sorted),
        ["N.esm", "M.esm", "z.esp", "y.esp"]
    );
}

#[test]
fn groups_decide_the_relative_positions() {
    covers(&[
        ids::groups::behaviour::SORTING,
        ids::sorting::stages::GROUP_EDGES,
    ]);
    let plugins = vec![plugin("B.esp", "B"), plugin("A.esp", "A")];
    let sorted = run(&plugins).expect("sorts");
    assert_eq!(names(&plugins, &sorted), ["A.esp", "B.esp"]);
    assert!(
        sorted
            .edges
            .iter()
            .any(|edge| edge.kind == EdgeType::MasterlistGroup && edge.from == 1 && edge.to == 0)
    );
}

#[test]
fn a_plugin_in_a_group_nobody_defined_has_no_group_edges() {
    let plugins = vec![plugin("B.esp", "B"), plugin("A.esp", "missing")];
    let sorted = run(&plugins).expect("sorts");
    assert_eq!(names(&plugins, &sorted), ["B.esp", "A.esp"]);
}

#[test]
fn load_after_and_requirements_decide_the_relative_positions() {
    covers(&[
        ids::sorting::stages::EDGES_RULES,
        ids::sorting::stages::TOPOLOGICAL,
    ]);
    let mut a = PluginInput::new("A.esp");
    a.load_after = vec!["b.esp".into()];
    let plugins = vec![a.clone(), PluginInput::new("B.esp")];
    let sorted = run(&plugins).expect("sorts");
    assert_eq!(names(&plugins, &sorted), ["B.esp", "A.esp"]);

    a.load_after.clear();
    a.requirements = vec!["B.esp".into()];
    let plugins = vec![a, PluginInput::new("B.esp")];
    let sorted = run(&plugins).expect("sorts");
    assert_eq!(names(&plugins, &sorted), ["B.esp", "A.esp"]);
    assert!(
        sorted
            .edges
            .iter()
            .any(|edge| edge.kind == EdgeType::MasterlistRequirement)
    );
}

#[test]
fn header_masters_load_first_and_names_are_compared_without_case() {
    let mut a = PluginInput::new("A.esp");
    a.masters = vec!["b.ESP".into(), "not installed.esm".into()];
    let plugins = vec![a, PluginInput::new("B.esp")];
    let sorted = run(&plugins).expect("sorts");
    assert_eq!(names(&plugins, &sorted), ["B.esp", "A.esp"]);
}

#[test]
fn an_early_loader_loads_before_everything_else_of_its_kind() {
    covers(&[ids::sorting::stages::EARLY_LOADER]);
    let plugins = vec![PluginInput::new("B.esp"), PluginInput::new("A.esp")];
    let sorted = sort(&plugins, &groups(), &["A.esp".to_string()], &NoOverlaps).expect("sorts");
    assert_eq!(names(&plugins, &sorted), ["A.esp", "B.esp"]);
    assert!(
        sorted
            .edges
            .iter()
            .any(|edge| edge.kind == EdgeType::Hardcoded)
    );
}

#[test]
fn a_cycle_among_the_metadata_names_the_plugins_and_edges() {
    covers(&[ids::sorting::stages::CYCLE_ERROR]);
    let mut a = PluginInput::new("A.esp");
    a.masters = vec!["B.esp".into()];
    let mut b = PluginInput::new("B.esp");
    b.masters = vec!["A.esp".into()];
    assert_eq!(
        run(&[a, b]),
        Err(GraphError::Cycle(vec![
            (0, EdgeType::Master),
            (1, EdgeType::Master)
        ]))
    );
    let mut selfish = PluginInput::new("S.esp");
    selfish.load_after = vec!["s.esp".into()];
    assert_eq!(
        run(&[selfish]),
        Err(GraphError::Cycle(vec![(0, EdgeType::MasterlistLoadAfter)]))
    );
}

#[test]
fn a_rule_that_puts_a_non_master_before_a_master_is_an_error() {
    let mut a = PluginInput::new("A.esp");
    a.is_master = true;
    for (setter, kind) in [
        (
            (|a: &mut PluginInput| a.masters = vec!["B.esp".into()]) as fn(&mut PluginInput),
            EdgeType::Master,
        ),
        (
            |a: &mut PluginInput| a.load_after = vec!["B.esp".into()],
            EdgeType::MasterlistLoadAfter,
        ),
        (
            |a: &mut PluginInput| a.requirements = vec!["B.esp".into()],
            EdgeType::MasterlistRequirement,
        ),
    ] {
        let mut plugin = a.clone();
        setter(&mut plugin);
        assert_eq!(
            run(&[plugin, PluginInput::new("B.esp")]),
            Err(GraphError::MasterOrder {
                plugin: 1,
                master: 0,
                kind
            })
        );
    }
}

#[test]
fn a_non_master_that_the_game_loads_first_is_an_error() {
    let mut a = PluginInput::new("A.esp");
    a.is_master = true;
    assert_eq!(
        sort(
            &[a, PluginInput::new("B.esp")],
            &groups(),
            &["B.esp".to_string()],
            &NoOverlaps
        ),
        Err(GraphError::MasterOrder {
            plugin: 1,
            master: 0,
            kind: EdgeType::Hardcoded
        })
    );
}

#[test]
fn a_master_that_asks_for_a_master_is_not_an_error_and_a_non_master_may_ask_for_a_master() {
    let mut esm = PluginInput::new("A.esm");
    esm.is_master = true;
    let mut other = PluginInput::new("B.esp");
    other.masters = vec!["A.esm".into()];
    let plugins = vec![other, esm];
    let sorted = run(&plugins).expect("sorts");
    assert_eq!(names(&plugins, &sorted), ["A.esm", "B.esp"]);
}

#[test]
fn groups_that_load_after_each_other_are_an_error() {
    covers(&[
        ids::groups::behaviour::CYCLIC,
        ids::sorting::stages::GROUP_CYCLE,
    ]);
    let cyclic = vec![
        GroupInput {
            name: "A".into(),
            after: vec!["B".into()],
        },
        GroupInput {
            name: "B".into(),
            after: vec!["A".into()],
        },
    ];
    let result = sort(&[PluginInput::new("a.esp")], &cyclic, &[], &NoOverlaps);
    assert!(matches!(result, Err(GraphError::GroupCycle(groups)) if groups.len() == 2));
}

#[test]
fn a_group_that_loads_after_a_group_nobody_defined_is_left_without_that_edge() {
    let groups = vec![GroupInput {
        name: "A".into(),
        after: vec!["nowhere".into()],
    }];
    let plugins = vec![plugin("b.esp", "A"), plugin("a.esp", "A")];
    let sorted = sort(&plugins, &groups, &[], &NoOverlaps).expect("sorts");
    assert_eq!(sorted.order, [0, 1]);
}

#[test]
fn too_many_plugins_are_refused() {
    let plugins: Vec<_> = (0..=MAX_PLUGINS)
        .map(|index| PluginInput::new(&format!("{index}.esp")))
        .collect();
    assert_eq!(run(&plugins), Err(GraphError::TooMany(MAX_PLUGINS + 1)));
}

struct Everything;

impl Overlaps for Everything {
    fn records(&self, _: usize, _: usize) -> bool {
        true
    }
    fn assets(&self, _: usize, _: usize) -> bool {
        true
    }
    fn work(&self) -> u64 {
        WORK_BUDGET + 1
    }
}

#[test]
fn a_comparison_that_costs_too_much_stops_the_sort() {
    let mut a = PluginInput::new("a.esp");
    a.override_records = 2;
    let mut b = PluginInput::new("b.esp");
    b.override_records = 1;
    assert_eq!(
        sort(&[a, b], &groups(), &[], &Everything),
        Err(GraphError::TooMuchWork)
    );
}

#[test]
fn the_plugin_that_changes_more_records_loads_first_and_then_the_one_with_more_assets() {
    struct Same;
    impl Overlaps for Same {
        fn records(&self, _: usize, _: usize) -> bool {
            true
        }
        fn assets(&self, _: usize, _: usize) -> bool {
            true
        }
    }
    let mut small = PluginInput::new("a.esp");
    small.override_records = 1;
    let mut big = PluginInput::new("b.esp");
    big.override_records = 5;
    let sorted = sort(&[small.clone(), big.clone()], &groups(), &[], &Same).expect("sorts");
    assert_eq!(sorted.order, [1, 0]);
    assert!(
        sorted
            .edges
            .iter()
            .any(|edge| edge.kind == EdgeType::RecordOverlap)
    );

    small.override_records = 0;
    big.override_records = 0;
    small.assets = 1;
    big.assets = 3;
    let sorted = sort(&[small, big], &groups(), &[], &Same).expect("sorts");
    assert_eq!(sorted.order, [1, 0]);
    assert!(
        sorted
            .edges
            .iter()
            .any(|edge| edge.kind == EdgeType::AssetOverlap)
    );
}

#[test]
fn an_overlap_edge_that_would_close_a_cycle_is_skipped_and_reported() {
    struct Same;
    impl Overlaps for Same {
        fn records(&self, _: usize, _: usize) -> bool {
            true
        }
        fn assets(&self, _: usize, _: usize) -> bool {
            false
        }
    }
    // a changes 1 record and b 5, so the overlap edge would go b -> a, but a -> c -> b already holds.
    let mut a = PluginInput::new("a.esp");
    a.override_records = 1;
    let mut b = PluginInput::new("b.esp");
    b.override_records = 5;
    b.load_after = vec!["c.esp".into()];
    let mut c = PluginInput::new("c.esp");
    c.load_after = vec!["a.esp".into()];
    let sorted = sort(&[a, b, c], &groups(), &[], &Same).expect("sorts");
    assert_eq!(sorted.order, [0, 2, 1]);
    assert!(
        sorted
            .skipped
            .iter()
            .any(|edge| edge.kind == EdgeType::RecordOverlap && edge.from == 1 && edge.to == 0)
    );
}

#[test]
fn display_texts_name_what_went_wrong() {
    assert!(GraphError::TooMany(2000).to_string().contains("2000"));
    assert!(
        GraphError::Cycle(vec![(1, EdgeType::Master), (2, EdgeType::TieBreak)])
            .to_string()
            .contains("Tie Break")
    );
    assert!(
        GraphError::MasterOrder {
            plugin: 1,
            master: 2,
            kind: EdgeType::Hardcoded
        }
        .to_string()
        .contains("Hardcoded")
    );
    assert!(
        GraphError::GroupCycle(vec!["A".into()])
            .to_string()
            .contains("\"A\"")
    );
    assert!(GraphError::TooMuchWork.to_string().contains("work"));
    assert!(GraphError::Internal("x").to_string().contains("internal"));
    for kind in [
        EdgeType::Hardcoded,
        EdgeType::MasterFlag,
        EdgeType::Master,
        EdgeType::MasterlistRequirement,
        EdgeType::MasterlistLoadAfter,
        EdgeType::MasterlistGroup,
        EdgeType::RecordOverlap,
        EdgeType::AssetOverlap,
        EdgeType::TieBreak,
    ] {
        assert_eq!(kind.to_string(), kind.label());
    }
}
