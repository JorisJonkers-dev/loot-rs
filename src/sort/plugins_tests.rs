// SPDX-License-Identifier: GPL-3.0-or-later
//
// The tests below are translated from libloot's own unit tests of its plugin graph
// (https://github.com/loot/libloot, `src/sorting/plugins.rs`, GPL-3.0-or-later, Copyright (C) the libloot authors): the
// same graphs, the same assertions, with the Starfield cases left out. They are what ties this port to libloot's
// behaviour.

#![allow(
    clippy::many_single_char_names,
    clippy::too_many_lines,
    unused_mut,
    unused_variables
)]

use super::*;
use crate::sort::groups;
use crate::sort::{EdgeType, GroupInput, Overlaps};
use std::collections::{HashMap, HashSet};

const PLUGIN_A: &str = "A.esp";
const PLUGIN_B: &str = "B.esp";
const PLUGIN_C: &str = "C.esp";
const PLUGIN_D: &str = "D.esp";
const PLUGIN_E: &str = "E.esp";
const PLUGIN_F: &str = "F.esp";
const PLUGIN_G: &str = "G.esp";
const PLUGIN_H: &str = "H.esp";
const PLUGIN_I: &str = "I.esp";
const PLUGIN_J: &str = "J.esp";
const PLUGIN_A1: &str = "A1.esp";
const PLUGIN_A2: &str = "A2.esp";
const PLUGIN_B1: &str = "B1.esp";
const PLUGIN_B2: &str = "B2.esp";
const PLUGIN_C1: &str = "C1.esp";
const PLUGIN_C2: &str = "C2.esp";
const PLUGIN_D1: &str = "D1.esp";
const PLUGIN_D2: &str = "D2.esp";
const PLUGIN_D3: &str = "D3.esp";

fn g(name: &str, after: &[&str]) -> GroupInput {
    GroupInput {
        name: name.to_string(),
        after: after.iter().map(ToString::to_string).collect(),
    }
}

/// libloot's fixture: the masterlist's A, B after A, C, default after C, E after default, F after E, ...
fn fixture_groups() -> Vec<GroupInput> {
    vec![
        g("A", &[]),
        g("B", &["A"]),
        g("C", &[]),
        g("default", &["C"]),
        g("E", &["default"]),
        g("F", &["E"]),
    ]
}

/// ... and the userlist's C after B.
fn fixture_user_groups() -> Vec<GroupInput> {
    vec![g("C", &["B"])]
}

fn node(nodes: &mut Vec<Vertex>, order: &[&str], name: &str, group: &str) -> usize {
    nodes.push(Vertex {
        input: order.iter().position(|n| *n == name).expect("in the order"),
        name: name.to_string(),
        group: group.to_string(),
        override_records: 0,
        assets: 0,
        masters: Vec::new(),
        requirements: Vec::new(),
        load_after: Vec::new(),
        group_is_user: false,
        user_requirements: Vec::new(),
        user_load_after: Vec::new(),
    });
    nodes.len() - 1
}

#[derive(Default)]
struct TestOverlaps {
    counts: HashMap<String, usize>,
    asset_counts: HashMap<String, usize>,
    records: HashSet<(String, String)>,
    assets: HashSet<(String, String)>,
}

struct Finished {
    names: Vec<String>,
    records: HashSet<(String, String)>,
    assets: HashSet<(String, String)>,
}

impl Overlaps for Finished {
    fn records(&self, a: usize, b: usize) -> bool {
        self.records
            .contains(&(self.names[a].clone(), self.names[b].clone()))
    }
    fn assets(&self, a: usize, b: usize) -> bool {
        self.assets
            .contains(&(self.names[a].clone(), self.names[b].clone()))
    }
}

impl TestOverlaps {
    fn count(&mut self, name: &str, count: usize) {
        self.counts.insert(name.to_string(), count);
    }
    fn asset_count(&mut self, name: &str, count: usize) {
        self.asset_counts.insert(name.to_string(), count);
    }
    fn records(&mut self, a: &str, b: &str) {
        self.records.insert((a.to_string(), b.to_string()));
    }
    fn assets_with(&mut self, a: &str, b: &str) {
        self.assets.insert((a.to_string(), b.to_string()));
    }
    fn finish(&self, order: &[&str], nodes: &mut [Vertex]) -> Finished {
        for vertex in nodes {
            vertex.override_records = self.counts.get(&vertex.name).copied().unwrap_or(0);
            vertex.assets = self.asset_counts.get(&vertex.name).copied().unwrap_or(0);
        }
        Finished {
            names: order.iter().map(ToString::to_string).collect(),
            records: self.records.clone(),
            assets: self.assets.clone(),
        }
    }
}

impl PluginGraph<'_> {
    fn has_edge(&self, from: usize, to: usize) -> bool {
        self.direct.get(from, to)
    }
    fn edge_type(&self, from: usize, to: usize) -> EdgeType {
        self.outgoing[from]
            .iter()
            .find(|&&(target, _)| target == to)
            .expect("an edge")
            .1
    }
    fn node_by_name(&self, name: &str) -> usize {
        self.vertices
            .iter()
            .position(|vertex| vertex.name == name)
            .expect("a node")
    }
}

/// libloot's `add_tie_break_edges` tests: the sorted names and whether the order is the only one.
fn tie_break(order: &[&str], edges: &[(&str, &str)]) -> (Vec<String>, bool) {
    let mut nodes: Vec<Vertex> = Vec::new();
    for name in order {
        node(&mut nodes, order, name, "default");
    }
    let over = TestOverlaps::default().finish(order, &mut nodes);
    let mut graph = PluginGraph::new(nodes, &over);
    for &(from, to) in edges {
        let (from, to) = (graph.node_by_name(from), graph.node_by_name(to));
        graph.add_edge(from, to, EdgeType::RecordOverlap);
    }
    graph.add_tie_break_edges().unwrap();
    let (sorted, unique) = graph.topological_order().unwrap();
    (
        sorted
            .into_iter()
            .map(|index| graph.vertices[index].name.clone())
            .collect(),
        unique,
    )
}

#[test]
fn tie_break_gives_the_creation_order_when_there_are_no_other_edges() {
    let order = [PLUGIN_A, PLUGIN_B, PLUGIN_C, PLUGIN_D, PLUGIN_E];
    let (sorted, unique) = tie_break(&order, &[]);
    assert!(unique);
    assert_eq!(sorted, order);
}

#[test]
fn tie_break_pins_paths_that_prevent_the_creation_order_from_being_used() {
    let order = [
        PLUGIN_A, PLUGIN_B, PLUGIN_C, PLUGIN_D, PLUGIN_E, PLUGIN_F, PLUGIN_G, PLUGIN_H, PLUGIN_I,
        PLUGIN_J,
    ];
    // g -> h -> i -> f, g -> d and i -> e
    let edges = [
        (PLUGIN_G, PLUGIN_H),
        (PLUGIN_H, PLUGIN_I),
        (PLUGIN_I, PLUGIN_F),
        (PLUGIN_G, PLUGIN_D),
        (PLUGIN_I, PLUGIN_E),
    ];
    let (sorted, unique) = tie_break(&order, &edges);
    assert!(unique);
    assert_eq!(
        sorted,
        [
            PLUGIN_A, PLUGIN_B, PLUGIN_C, PLUGIN_G, PLUGIN_D, PLUGIN_H, PLUGIN_I, PLUGIN_E,
            PLUGIN_F, PLUGIN_J
        ]
    );
}

#[test]
fn tie_break_prefixes_the_path_when_the_first_pair_cannot_be_used_in_creation_order() {
    let order = [
        PLUGIN_A, PLUGIN_B, PLUGIN_C, PLUGIN_D, PLUGIN_E, PLUGIN_F, PLUGIN_G, PLUGIN_H, PLUGIN_I,
        PLUGIN_J,
    ];
    // b -> c -> d -> a
    let edges = [
        (PLUGIN_B, PLUGIN_C),
        (PLUGIN_C, PLUGIN_D),
        (PLUGIN_D, PLUGIN_A),
    ];
    let (sorted, unique) = tie_break(&order, &edges);
    assert!(unique);
    assert_eq!(
        sorted,
        [
            PLUGIN_B, PLUGIN_C, PLUGIN_D, PLUGIN_A, PLUGIN_E, PLUGIN_F, PLUGIN_G, PLUGIN_H,
            PLUGIN_I, PLUGIN_J
        ]
    );
}

#[test]
fn tie_break_of_one_plugin_is_fine() {
    let (sorted, unique) = tie_break(&[PLUGIN_A], &[]);
    assert!(unique);
    assert_eq!(sorted, [PLUGIN_A]);
}

#[test]
fn should_add_no_edges_if_there_are_no_early_loading_plugins() {
    let order = [PLUGIN_A, PLUGIN_B, PLUGIN_C];
    let mut over = TestOverlaps::default();
    let groups_graph = groups::build(&fixture_groups(), &fixture_user_groups()).unwrap();

    let mut nodes: Vec<Vertex> = Vec::new();
    let a = node(&mut nodes, &order, PLUGIN_A, "default");
    let b = node(&mut nodes, &order, PLUGIN_B, "default");
    let c = node(&mut nodes, &order, PLUGIN_C, "default");

    let over = over.finish(&order, &mut nodes);
    let mut graph = PluginGraph::new(nodes, &over);
    graph.add_early_loading_edges(&[]);

    assert!(!graph.has_edge(a, b));
    assert!(!graph.has_edge(a, c));
    assert!(!graph.has_edge(b, a));
    assert!(!graph.has_edge(b, c));
    assert!(!graph.has_edge(c, a));
    assert!(!graph.has_edge(c, b));
}

#[test]
fn should_add_edges_between_consecutive_early_loaders_skipping_missing_plugins() {
    let order = [PLUGIN_A, PLUGIN_C, PLUGIN_D];
    let mut over = TestOverlaps::default();
    let groups_graph = groups::build(&fixture_groups(), &fixture_user_groups()).unwrap();

    let mut nodes: Vec<Vertex> = Vec::new();
    let a = node(&mut nodes, &order, PLUGIN_A, "default");
    let c = node(&mut nodes, &order, PLUGIN_C, "default");
    let d = node(&mut nodes, &order, PLUGIN_D, "default");

    let over = over.finish(&order, &mut nodes);
    let mut graph = PluginGraph::new(nodes, &over);
    graph.add_early_loading_edges(&[
        PLUGIN_A.to_string(),
        PLUGIN_B.to_string(),
        PLUGIN_C.to_string(),
        PLUGIN_D.to_string(),
    ]);

    assert!(graph.has_edge(a, c));
    assert!(graph.has_edge(c, d));
    assert!(!graph.has_edge(a, d));

    assert!(graph.check_for_cycles().is_ok());
}

#[test]
fn should_add_edges_from_only_the_last_installed_early_loader_to_all_non_early_loader_plugins() {
    let order = [PLUGIN_A, PLUGIN_B, PLUGIN_D, PLUGIN_E];
    let mut over = TestOverlaps::default();
    let groups_graph = groups::build(&fixture_groups(), &fixture_user_groups()).unwrap();

    let mut nodes: Vec<Vertex> = Vec::new();
    let a = node(&mut nodes, &order, PLUGIN_A, "default");
    let b = node(&mut nodes, &order, PLUGIN_B, "default");
    let d = node(&mut nodes, &order, PLUGIN_D, "default");
    let e = node(&mut nodes, &order, PLUGIN_E, "default");

    let over = over.finish(&order, &mut nodes);
    let mut graph = PluginGraph::new(nodes, &over);
    graph.add_early_loading_edges(&[
        PLUGIN_A.to_string(),
        PLUGIN_B.to_string(),
        PLUGIN_C.to_string(),
    ]);

    assert!(graph.has_edge(a, b));
    assert!(graph.has_edge(b, d));
    assert!(graph.has_edge(b, e));
    assert!(!graph.has_edge(a, d));
    assert!(!graph.has_edge(a, e));

    assert!(graph.check_for_cycles().is_ok());
}

#[test]
fn should_add_masterlist_group_edge_if_no_user_metadata_is_involved() {
    let order = [PLUGIN_A, PLUGIN_B];
    let mut over = TestOverlaps::default();
    let groups_graph = groups::build(&fixture_groups(), &fixture_user_groups()).unwrap();

    let mut nodes: Vec<Vertex> = Vec::new();
    let a = node(&mut nodes, &order, PLUGIN_A, "A");
    let b = node(&mut nodes, &order, PLUGIN_B, "B");

    let over = over.finish(&order, &mut nodes);
    let mut graph = PluginGraph::new(nodes, &over);
    graph.add_group_edges(&groups_graph).unwrap();

    assert_eq!(EdgeType::MasterlistGroup, graph.edge_type(a, b));

    assert!(graph.check_for_cycles().is_ok());
}

#[test]
fn should_add_edges_between_plugins_in_indirectly_connected_groups_when_an_intermediate_plugin_edge_is_skipped()
 {
    let order = [
        PLUGIN_A1, PLUGIN_A2, PLUGIN_B1, PLUGIN_B2, PLUGIN_C1, PLUGIN_C2,
    ];
    let mut over = TestOverlaps::default();
    let groups_graph = groups::build(&fixture_groups(), &fixture_user_groups()).unwrap();

    let mut nodes: Vec<Vertex> = Vec::new();
    let a1 = node(&mut nodes, &order, PLUGIN_A1, "A");
    let a2 = node(&mut nodes, &order, PLUGIN_A2, "A");
    let b1 = node(&mut nodes, &order, PLUGIN_B1, "B");
    let b2 = node(&mut nodes, &order, PLUGIN_B2, "B");
    let c1 = node(&mut nodes, &order, PLUGIN_C1, "C");
    let c2 = node(&mut nodes, &order, PLUGIN_C2, "C");

    let over = over.finish(&order, &mut nodes);
    let mut graph = PluginGraph::new(nodes, &over);
    graph.add_edge(b1, a1, EdgeType::Master);

    graph.add_group_edges(&groups_graph).unwrap();

    // Should be A2.esp -> B1.esp -> A1.esp -> B2.esp -> C1.esp
    //                                                -> C2.esp
    assert!(graph.has_edge(b1, a1));
    assert!(graph.has_edge(a1, b2));
    assert!(graph.has_edge(a2, b1));
    assert!(graph.has_edge(a2, b2));
    assert!(graph.has_edge(b1, c1));
    assert!(graph.has_edge(b1, c2));
    assert!(graph.has_edge(b2, c1));
    assert!(graph.has_edge(b2, c2));
    assert!(graph.has_edge(a1, c1));
    assert!(graph.has_edge(a1, c2));
    assert!(!graph.has_edge(c1, c2));
    assert!(!graph.has_edge(c2, c1));

    assert!(graph.check_for_cycles().is_ok());
}

#[test]
fn should_add_edges_across_empty_groups() {
    let order = [PLUGIN_A, PLUGIN_C];
    let mut over = TestOverlaps::default();
    let groups_graph = groups::build(&fixture_groups(), &fixture_user_groups()).unwrap();

    let mut nodes: Vec<Vertex> = Vec::new();
    let a = node(&mut nodes, &order, PLUGIN_A, "A");
    let c = node(&mut nodes, &order, PLUGIN_C, "C");

    let over = over.finish(&order, &mut nodes);
    let mut graph = PluginGraph::new(nodes, &over);
    graph.add_group_edges(&groups_graph).unwrap();

    // Should be A.esp -> C.esp
    assert!(graph.has_edge(a, c));

    assert!(graph.check_for_cycles().is_ok());
}

#[test]
fn should_add_edges_across_the_non_empty_default_group() {
    let order = [PLUGIN_A, PLUGIN_D, PLUGIN_E];
    let mut over = TestOverlaps::default();
    let groups_graph = groups::build(&fixture_groups(), &fixture_user_groups()).unwrap();

    let mut nodes: Vec<Vertex> = Vec::new();
    let a = node(&mut nodes, &order, PLUGIN_A, "A");
    let d = node(&mut nodes, &order, PLUGIN_D, "default");
    let e = node(&mut nodes, &order, PLUGIN_E, "E");

    let over = over.finish(&order, &mut nodes);
    let mut graph = PluginGraph::new(nodes, &over);
    graph.add_group_edges(&groups_graph).unwrap();

    // Should be A.esp -> D.esp -> E.esp
    //                 ---------->
    assert!(graph.has_edge(a, d));
    assert!(graph.has_edge(d, e));
    assert!(graph.has_edge(a, e));

    assert!(graph.check_for_cycles().is_ok());
}

#[test]
fn should_skip_an_edge_that_would_cause_a_cycle() {
    let order = [PLUGIN_A, PLUGIN_C];
    let mut over = TestOverlaps::default();
    let groups_graph = groups::build(&fixture_groups(), &fixture_user_groups()).unwrap();

    let mut nodes: Vec<Vertex> = Vec::new();
    let a = node(&mut nodes, &order, PLUGIN_A, "A");
    let c = node(&mut nodes, &order, PLUGIN_C, "C");

    let over = over.finish(&order, &mut nodes);
    let mut graph = PluginGraph::new(nodes, &over);
    graph.add_edge(c, a, EdgeType::Master);

    graph.add_group_edges(&groups_graph).unwrap();

    assert!(graph.has_edge(c, a));
    assert!(!graph.has_edge(a, c));

    assert!(graph.check_for_cycles().is_ok());
}

#[test]
fn should_skip_an_edge_that_would_cause_a_cycle_involving_other_non_default_groups() {
    let order = [PLUGIN_A, PLUGIN_B, PLUGIN_C];
    let mut over = TestOverlaps::default();
    let groups_graph = groups::build(&fixture_groups(), &fixture_user_groups()).unwrap();

    let mut nodes: Vec<Vertex> = Vec::new();
    let a = node(&mut nodes, &order, PLUGIN_A, "A");
    let b = node(&mut nodes, &order, PLUGIN_B, "B");
    let c = node(&mut nodes, &order, PLUGIN_C, "C");

    let over = over.finish(&order, &mut nodes);
    let mut graph = PluginGraph::new(nodes, &over);
    graph.add_edge(c, a, EdgeType::Master);

    graph.add_group_edges(&groups_graph).unwrap();

    assert!(graph.has_edge(c, a));
    assert!(graph.has_edge(a, b));

    assert!(graph.check_for_cycles().is_ok());
}

#[test]
fn should_skip_only_edges_to_the_target_group_plugins_that_would_cause_a_cycle() {
    let order = [PLUGIN_A, PLUGIN_C1, PLUGIN_C2];
    let mut over = TestOverlaps::default();
    let groups_graph = groups::build(&fixture_groups(), &fixture_user_groups()).unwrap();

    let mut nodes: Vec<Vertex> = Vec::new();
    let a = node(&mut nodes, &order, PLUGIN_A, "A");
    let c1 = node(&mut nodes, &order, PLUGIN_C1, "C");
    let c2 = node(&mut nodes, &order, PLUGIN_C2, "C");

    let over = over.finish(&order, &mut nodes);
    let mut graph = PluginGraph::new(nodes, &over);
    graph.add_edge(c1, a, EdgeType::Master);

    graph.add_group_edges(&groups_graph).unwrap();

    // Should be C1.esp -> A.esp -> C2.esp
    assert!(graph.has_edge(c1, a));
    assert!(graph.has_edge(a, c2));

    assert!(graph.check_for_cycles().is_ok());
}

#[test]
fn should_skip_only_edges_from_ancestors_to_the_target_group_plugins_that_would_cause_a_cycle() {
    let order = [PLUGIN_B, PLUGIN_C, PLUGIN_D1, PLUGIN_D2, PLUGIN_D3];
    let mut over = TestOverlaps::default();
    let groups_graph = groups::build(&fixture_groups(), &fixture_user_groups()).unwrap();

    let mut nodes: Vec<Vertex> = Vec::new();
    let b = node(&mut nodes, &order, PLUGIN_B, "B");
    let c = node(&mut nodes, &order, PLUGIN_C, "C");
    let d1 = node(&mut nodes, &order, PLUGIN_D1, "default");
    let d2 = node(&mut nodes, &order, PLUGIN_D2, "default");
    let d3 = node(&mut nodes, &order, PLUGIN_D3, "default");

    let over = over.finish(&order, &mut nodes);
    let mut graph = PluginGraph::new(nodes, &over);
    graph.add_edge(d1, b, EdgeType::Master);
    graph.add_edge(d2, b, EdgeType::Master);
    graph.add_edge(c, b, EdgeType::Master);
    graph.add_edge(c, d2, EdgeType::Master);
    graph.add_edge(c, d3, EdgeType::Master);

    graph.add_group_edges(&groups_graph).unwrap();

    // Should be: C.esp -> D2.esp -> B.esp -> D3.esp
    //                  -> D1.esp ->
    //                  -------------------->
    //                  ----------->
    assert!(graph.has_edge(d1, b));
    assert!(graph.has_edge(d2, b));
    assert!(graph.has_edge(c, b));
    assert!(graph.has_edge(c, d2));
    assert!(graph.has_edge(c, d3));
    assert!(graph.has_edge(b, d3));
    assert!(graph.has_edge(c, d1));

    assert!(graph.check_for_cycles().is_ok());
}

#[test]
fn should_add_plugin_edges_across_a_successor_if_at_least_one_edge_to_the_successor_group_was_skipped_with_successive_depths()
 {
    let order = [
        PLUGIN_A1, PLUGIN_A2, PLUGIN_B1, PLUGIN_B2, PLUGIN_C1, PLUGIN_C2,
    ];
    let mut over = TestOverlaps::default();
    let groups_graph = groups::build(&fixture_groups(), &fixture_user_groups()).unwrap();

    let mut nodes: Vec<Vertex> = Vec::new();
    let a1 = node(&mut nodes, &order, PLUGIN_A1, "A");
    let a2 = node(&mut nodes, &order, PLUGIN_A2, "A");
    let b1 = node(&mut nodes, &order, PLUGIN_B1, "B");
    let b2 = node(&mut nodes, &order, PLUGIN_B2, "B");
    let c1 = node(&mut nodes, &order, PLUGIN_C1, "C");
    let c2 = node(&mut nodes, &order, PLUGIN_C2, "C");

    let over = over.finish(&order, &mut nodes);
    let mut graph = PluginGraph::new(nodes, &over);
    graph.add_edge(b1, a1, EdgeType::Master);
    graph.add_edge(c1, b2, EdgeType::Master);

    graph.add_group_edges(&groups_graph).unwrap();

    // Should be A2.esp -> B1.esp -> A1.esp -> C1.esp -> B2.esp -> C2.esp
    assert!(graph.has_edge(b1, a1));
    assert!(graph.has_edge(c1, b2));
    assert!(graph.has_edge(a1, b2));
    assert!(graph.has_edge(a1, c1));
    assert!(graph.has_edge(a1, c2));
    assert!(graph.has_edge(a2, b1));
    assert!(graph.has_edge(a2, b2));
    assert!(graph.has_edge(b1, c1));
    assert!(graph.has_edge(b1, c2));
    assert!(graph.has_edge(b2, c2));

    assert!(graph.check_for_cycles().is_ok());
}

#[test]
fn should_add_plugin_edges_across_a_successor_if_at_least_one_edge_to_the_successor_group_was_skipped_with_successive_depths_and_a_different_order()
 {
    let order = [
        PLUGIN_A1, PLUGIN_A2, PLUGIN_B1, PLUGIN_B2, PLUGIN_C1, PLUGIN_C2,
    ];
    let mut over = TestOverlaps::default();
    let groups_graph = groups::build(&fixture_groups(), &fixture_user_groups()).unwrap();

    let mut nodes: Vec<Vertex> = Vec::new();
    let a1 = node(&mut nodes, &order, PLUGIN_A1, "A");
    let a2 = node(&mut nodes, &order, PLUGIN_A2, "A");
    let b1 = node(&mut nodes, &order, PLUGIN_B1, "B");
    let b2 = node(&mut nodes, &order, PLUGIN_B2, "B");
    let c1 = node(&mut nodes, &order, PLUGIN_C1, "C");
    let c2 = node(&mut nodes, &order, PLUGIN_C2, "C");

    let over = over.finish(&order, &mut nodes);
    let mut graph = PluginGraph::new(nodes, &over);
    graph.add_edge(b1, a1, EdgeType::Master);
    graph.add_edge(c1, b1, EdgeType::Master);

    graph.add_group_edges(&groups_graph).unwrap();

    // Should be A2.esp -> C1.esp -> B1.esp -> A1.esp -> B2.esp -> C2.esp
    assert!(graph.has_edge(b1, a1));
    assert!(graph.has_edge(c1, b1));
    assert!(graph.has_edge(a1, b2));
    assert!(graph.has_edge(a1, c2));
    assert!(graph.has_edge(a2, b1));
    assert!(graph.has_edge(a2, b2));
    assert!(graph.has_edge(a2, c1));
    assert!(graph.has_edge(b1, c2));
    assert!(graph.has_edge(b2, c2));
    assert!(!graph.has_edge(b2, c1));

    assert!(graph.check_for_cycles().is_ok());
}

#[test]
fn should_add_edge_from_ancestor_to_successor_if_none_of_a_groups_plugins_can() {
    let order = [PLUGIN_A, PLUGIN_B1, PLUGIN_B2, PLUGIN_C];
    let mut over = TestOverlaps::default();
    let groups_graph = groups::build(&fixture_groups(), &fixture_user_groups()).unwrap();

    let mut nodes: Vec<Vertex> = Vec::new();
    let a = node(&mut nodes, &order, PLUGIN_A, "A");
    let b1 = node(&mut nodes, &order, PLUGIN_B1, "B");
    let b2 = node(&mut nodes, &order, PLUGIN_B2, "B");
    let c = node(&mut nodes, &order, PLUGIN_C, "C");

    let over = over.finish(&order, &mut nodes);
    let mut graph = PluginGraph::new(nodes, &over);
    graph.add_edge(c, b1, EdgeType::Master);
    graph.add_edge(c, b2, EdgeType::Master);

    graph.add_group_edges(&groups_graph).unwrap();

    // Should be A.esp -> C1.esp -> B1.esp
    //                           -> B2.esp
    assert!(graph.has_edge(a, b1));
    assert!(graph.has_edge(a, b2));
    assert!(graph.has_edge(c, b1));
    assert!(graph.has_edge(c, b2));
    assert!(graph.has_edge(a, c));
    assert!(!graph.has_edge(b1, b2));
    assert!(!graph.has_edge(b2, b1));

    assert!(graph.check_for_cycles().is_ok());
}

#[test]
fn should_add_edge_from_ancestor_to_successor_if_none_of_a_groups_plugins_can_with_edges_across_the_skipped_group()
 {
    let order = [
        PLUGIN_A1, PLUGIN_A2, PLUGIN_B1, PLUGIN_B2, PLUGIN_C1, PLUGIN_C2, PLUGIN_D1, PLUGIN_D2,
    ];
    let mut over = TestOverlaps::default();
    let groups_graph = groups::build(&fixture_groups(), &fixture_user_groups()).unwrap();

    let mut nodes: Vec<Vertex> = Vec::new();
    let a1 = node(&mut nodes, &order, PLUGIN_A1, "A");
    let a2 = node(&mut nodes, &order, PLUGIN_A2, "A");
    let b1 = node(&mut nodes, &order, PLUGIN_B1, "B");
    let b2 = node(&mut nodes, &order, PLUGIN_B2, "B");
    let c1 = node(&mut nodes, &order, PLUGIN_C1, "C");
    let c2 = node(&mut nodes, &order, PLUGIN_C2, "C");
    let d1 = node(&mut nodes, &order, PLUGIN_D1, "default");
    let d2 = node(&mut nodes, &order, PLUGIN_D2, "default");

    let over = over.finish(&order, &mut nodes);
    let mut graph = PluginGraph::new(nodes, &over);
    graph.add_edge(b1, a1, EdgeType::Master);
    graph.add_edge(c1, b1, EdgeType::Master);
    graph.add_edge(d1, c1, EdgeType::Master);
    graph.add_edge(d2, c1, EdgeType::Master);

    graph.add_group_edges(&groups_graph).unwrap();

    // Should be:
    // A2.esp -> D1.esp -> C1.esp -> B1.esp -> A1.esp -> B2.esp -> C2.esp
    //        -> D2.esp ->
    assert!(graph.has_edge(b1, a1));
    assert!(graph.has_edge(c1, b1));
    assert!(graph.has_edge(d1, c1));
    assert!(graph.has_edge(d2, c1));
    assert!(graph.has_edge(a1, b2));
    assert!(graph.has_edge(a2, b1));
    assert!(graph.has_edge(a2, b2));
    assert!(graph.has_edge(a1, c2));
    assert!(graph.has_edge(b1, c2));
    assert!(graph.has_edge(a2, c1));
    assert!(graph.has_edge(a2, d1));
    assert!(graph.has_edge(a2, d2));
    assert!(!graph.has_edge(b2, c1));
    assert!(!graph.has_edge(d1, d2));
    assert!(!graph.has_edge(d2, d1));

    assert!(graph.check_for_cycles().is_ok());
}

#[test]
fn should_deprioritise_edges_from_default_group_plugins_with_default_last() {
    let order = [PLUGIN_B, PLUGIN_C, PLUGIN_D];
    let mut over = TestOverlaps::default();
    let groups_graph = groups::build(&fixture_groups(), &fixture_user_groups()).unwrap();

    let mut nodes: Vec<Vertex> = Vec::new();
    let b = node(&mut nodes, &order, PLUGIN_B, "B");
    let c = node(&mut nodes, &order, PLUGIN_C, "C");
    let d = node(&mut nodes, &order, PLUGIN_D, "default");

    let over = over.finish(&order, &mut nodes);
    let mut graph = PluginGraph::new(nodes, &over);
    graph.add_edge(d, b, EdgeType::Master);

    graph.add_group_edges(&groups_graph).unwrap();

    // Should be D.esp -> B.esp -> C.esp
    assert!(graph.has_edge(b, c));
    assert!(graph.has_edge(d, b));
    assert!(!graph.has_edge(c, d));

    assert!(graph.check_for_cycles().is_ok());
}

#[test]
fn should_deprioritise_edges_from_default_group_plugins_with_default_first() {
    let order = [PLUGIN_D, PLUGIN_E, PLUGIN_F];
    let mut over = TestOverlaps::default();
    let groups_graph = groups::build(&fixture_groups(), &fixture_user_groups()).unwrap();

    let mut nodes: Vec<Vertex> = Vec::new();
    let d = node(&mut nodes, &order, PLUGIN_D, "default");
    let e = node(&mut nodes, &order, PLUGIN_E, "E");
    let f = node(&mut nodes, &order, PLUGIN_F, "F");

    let over = over.finish(&order, &mut nodes);
    let mut graph = PluginGraph::new(nodes, &over);
    graph.add_edge(f, d, EdgeType::Master);

    graph.add_group_edges(&groups_graph).unwrap();

    // Should be E.esp -> F.esp -> D.esp
    assert!(graph.has_edge(e, f));
    assert!(graph.has_edge(f, d));
    assert!(!graph.has_edge(d, e));

    assert!(graph.check_for_cycles().is_ok());
}

#[test]
fn should_deprioritise_edges_from_default_group_plugins_across_skipped_intermediate_groups() {
    let order = [PLUGIN_D, PLUGIN_E, PLUGIN_F];
    let mut over = TestOverlaps::default();
    let groups_graph = groups::build(&fixture_groups(), &fixture_user_groups()).unwrap();

    let mut nodes: Vec<Vertex> = Vec::new();
    let d = node(&mut nodes, &order, PLUGIN_D, "default");
    let e = node(&mut nodes, &order, PLUGIN_E, "E");
    let f = node(&mut nodes, &order, PLUGIN_F, "F");

    let over = over.finish(&order, &mut nodes);
    let mut graph = PluginGraph::new(nodes, &over);
    graph.add_edge(e, d, EdgeType::Master);

    graph.add_group_edges(&groups_graph).unwrap();

    // Should be E.esp -> D.esp -> F.esp
    assert!(graph.has_edge(e, d));
    assert!(graph.has_edge(d, f));
    assert!(!graph.has_edge(f, e));

    assert!(graph.check_for_cycles().is_ok());
}

#[test]
fn should_deprioritise_edges_from_default_group_plugins_with_d1_first_d2_last() {
    let order = [PLUGIN_D1, PLUGIN_D2, PLUGIN_E, PLUGIN_F];
    let mut over = TestOverlaps::default();
    let groups_graph = groups::build(&fixture_groups(), &fixture_user_groups()).unwrap();

    let mut nodes: Vec<Vertex> = Vec::new();
    let d1 = node(&mut nodes, &order, PLUGIN_D1, "default");
    let d2 = node(&mut nodes, &order, PLUGIN_D2, "default");
    let e = node(&mut nodes, &order, PLUGIN_E, "E");
    let f = node(&mut nodes, &order, PLUGIN_F, "F");

    let over = over.finish(&order, &mut nodes);
    let mut graph = PluginGraph::new(nodes, &over);
    graph.add_edge(f, d2, EdgeType::Master);

    graph.add_group_edges(&groups_graph).unwrap();

    // Should be D1.esp -> E.esp -> F.esp -> D2.esp
    assert!(graph.has_edge(e, f));
    assert!(graph.has_edge(f, d2));
    assert!(graph.has_edge(d1, e));
    assert!(!graph.has_edge(d2, d1));

    assert!(graph.check_for_cycles().is_ok());
}

#[test]
fn should_deprioritise_edges_from_default_group_plugins_with_no_ideal_result() {
    let order = [PLUGIN_B, PLUGIN_C, PLUGIN_D, PLUGIN_E, PLUGIN_F];
    let mut over = TestOverlaps::default();
    let groups_graph = groups::build(&fixture_groups(), &fixture_user_groups()).unwrap();

    let mut nodes: Vec<Vertex> = Vec::new();
    let b = node(&mut nodes, &order, PLUGIN_B, "B");
    let c = node(&mut nodes, &order, PLUGIN_C, "C");
    let d = node(&mut nodes, &order, PLUGIN_D, "default");
    let e = node(&mut nodes, &order, PLUGIN_E, "E");
    let f = node(&mut nodes, &order, PLUGIN_F, "F");

    let over = over.finish(&order, &mut nodes);
    let mut graph = PluginGraph::new(nodes, &over);
    graph.add_edge(d, b, EdgeType::Master);
    graph.add_edge(f, d, EdgeType::Master);

    graph.add_group_edges(&groups_graph).unwrap();

    // No ideal result, expected is F.esp -> D.esp -> B.esp -> C.esp -> E.esp
    assert!(graph.has_edge(f, d));
    assert!(graph.has_edge(d, b));
    assert!(graph.has_edge(b, c));
    assert!(graph.has_edge(c, e));
    assert!(!graph.has_edge(e, f));

    assert!(graph.check_for_cycles().is_ok());
}

#[test]
fn should_deprioritise_edges_from_default_group_plugins_with_default_in_middle_and_d_bookends() {
    let order = [PLUGIN_B, PLUGIN_C, PLUGIN_D1, PLUGIN_D2, PLUGIN_E, PLUGIN_F];
    let mut over = TestOverlaps::default();
    let groups_graph = groups::build(&fixture_groups(), &fixture_user_groups()).unwrap();

    let mut nodes: Vec<Vertex> = Vec::new();
    let b = node(&mut nodes, &order, PLUGIN_B, "B");
    let c = node(&mut nodes, &order, PLUGIN_C, "C");
    let d1 = node(&mut nodes, &order, PLUGIN_D1, "default");
    let d2 = node(&mut nodes, &order, PLUGIN_D2, "default");
    let e = node(&mut nodes, &order, PLUGIN_E, "E");
    let f = node(&mut nodes, &order, PLUGIN_F, "F");

    let over = over.finish(&order, &mut nodes);
    let mut graph = PluginGraph::new(nodes, &over);
    graph.add_edge(d2, b, EdgeType::Master);
    graph.add_edge(f, d1, EdgeType::Master);

    graph.add_group_edges(&groups_graph).unwrap();

    // Should be D2.esp -> B.esp -> C.esp -> E.esp -> F.esp -> D1.esp
    assert!(graph.has_edge(d2, b));
    assert!(graph.has_edge(b, c));
    assert!(graph.has_edge(c, e));
    assert!(graph.has_edge(e, f));
    assert!(graph.has_edge(f, d1));
    assert!(!graph.has_edge(d1, d2));

    assert!(graph.check_for_cycles().is_ok());
}

#[test]
fn should_deprioritise_edges_from_default_group_plugins_with_default_in_middle_and_d_throughout() {
    const PLUGIN_D4: &str = "D4.esp";
    let order = [
        PLUGIN_B, PLUGIN_C, PLUGIN_D1, PLUGIN_D2, PLUGIN_D3, PLUGIN_D4, PLUGIN_E, PLUGIN_F,
    ];
    let mut over = TestOverlaps::default();
    let groups_graph = groups::build(&fixture_groups(), &fixture_user_groups()).unwrap();

    let mut nodes: Vec<Vertex> = Vec::new();
    let b = node(&mut nodes, &order, PLUGIN_B, "B");
    let c = node(&mut nodes, &order, PLUGIN_C, "C");
    let d1 = node(&mut nodes, &order, PLUGIN_D1, "default");
    let d2 = node(&mut nodes, &order, PLUGIN_D2, "default");
    let d3 = node(&mut nodes, &order, PLUGIN_D3, "default");
    let d4 = node(&mut nodes, &order, PLUGIN_D4, "default");
    let e = node(&mut nodes, &order, PLUGIN_E, "E");
    let f = node(&mut nodes, &order, PLUGIN_F, "F");

    let over = over.finish(&order, &mut nodes);
    let mut graph = PluginGraph::new(nodes, &over);
    graph.add_edge(d2, b, EdgeType::Master);
    graph.add_edge(d4, c, EdgeType::Master);
    graph.add_edge(f, d1, EdgeType::Master);

    graph.add_group_edges(&groups_graph).unwrap();

    // Should be:
    // D2.esp -> B.esp -> D4.esp -> C.esp -> D3.esp -> E.esp -> F.esp -> D1.esp
    assert!(graph.has_edge(d2, b));
    assert!(graph.has_edge(b, c));
    assert!(graph.has_edge(c, d3));
    assert!(graph.has_edge(c, e));
    assert!(graph.has_edge(d3, e));
    assert!(graph.has_edge(e, f));
    assert!(graph.has_edge(f, d1));
    assert!(graph.has_edge(d4, c));
    assert!(graph.has_edge(b, d4));
    assert!(!graph.has_edge(d1, d2));
    assert!(!graph.has_edge(d1, d3));
    assert!(!graph.has_edge(d1, d4));

    assert!(graph.check_for_cycles().is_ok());
}

#[test]
fn should_handle_asymmetric_branches_in_the_groups_graph() {
    let order = [PLUGIN_A, PLUGIN_B, PLUGIN_C, PLUGIN_D];
    let mut over = TestOverlaps::default();
    let groups_graph = groups::build(&fixture_groups(), &fixture_user_groups()).unwrap();

    let groups = vec![
        g("A", &[]),
        g("B", &["A"]),
        g("C", &["B"]),
        g("D", &["A"]),
        g("default", &[]),
    ];
    let groups_graph = groups::build(&groups, &[]).unwrap();

    let mut nodes: Vec<Vertex> = Vec::new();
    let a = node(&mut nodes, &order, PLUGIN_A, "A");
    let b = node(&mut nodes, &order, PLUGIN_B, "B");
    let c = node(&mut nodes, &order, PLUGIN_C, "C");
    let d = node(&mut nodes, &order, PLUGIN_D, "D");

    let over = over.finish(&order, &mut nodes);
    let mut graph = PluginGraph::new(nodes, &over);
    graph.add_group_edges(&groups_graph).unwrap();

    // Should be A.esp -> B.esp -> C.esp
    //                 -> D.esp
    assert!(graph.has_edge(a, b));
    assert!(graph.has_edge(b, c));
    assert!(graph.has_edge(a, d));
    assert!(!graph.has_edge(d, b));
    assert!(!graph.has_edge(d, c));
    assert!(!graph.has_edge(b, d));
    assert!(!graph.has_edge(c, d));

    assert!(graph.check_for_cycles().is_ok());
}

#[test]
fn should_handle_asymmetric_branches_in_the_groups_graph_that_merge() {
    let order = [PLUGIN_A, PLUGIN_B, PLUGIN_C, PLUGIN_D, PLUGIN_E];
    let mut over = TestOverlaps::default();
    let groups_graph = groups::build(&fixture_groups(), &fixture_user_groups()).unwrap();

    let groups = vec![
        g("A", &[]),
        g("B", &["A"]),
        g("C", &["B"]),
        g("D", &["A"]),
        g("E", &["C", "D"]),
        g("default", &[]),
    ];
    let groups_graph = groups::build(&groups, &[]).unwrap();

    let mut nodes: Vec<Vertex> = Vec::new();
    let a = node(&mut nodes, &order, PLUGIN_A, "A");
    let b = node(&mut nodes, &order, PLUGIN_B, "B");
    let c = node(&mut nodes, &order, PLUGIN_C, "C");
    let d = node(&mut nodes, &order, PLUGIN_D, "D");
    let e = node(&mut nodes, &order, PLUGIN_E, "E");

    let over = over.finish(&order, &mut nodes);
    let mut graph = PluginGraph::new(nodes, &over);
    graph.add_group_edges(&groups_graph).unwrap();

    // Should be A.esp -> B.esp -> C.esp -> E.esp
    //                 -> D.esp ---------->
    assert!(graph.has_edge(a, b));
    assert!(graph.has_edge(b, c));
    assert!(graph.has_edge(c, e));
    assert!(graph.has_edge(a, d));
    assert!(graph.has_edge(d, e));
    assert!(!graph.has_edge(d, b));
    assert!(!graph.has_edge(d, c));
    assert!(!graph.has_edge(b, d));
    assert!(!graph.has_edge(c, d));

    assert!(graph.check_for_cycles().is_ok());
}

#[test]
fn should_handle_branches_in_the_groups_graph_that_form_a_diamond_pattern() {
    let order = [PLUGIN_A, PLUGIN_B, PLUGIN_C, PLUGIN_D];
    let mut over = TestOverlaps::default();
    let groups_graph = groups::build(&fixture_groups(), &fixture_user_groups()).unwrap();

    let groups = vec![
        g("A", &[]),
        g("B", &["A"]),
        g("C", &["A"]),
        g("D", &["B", "C"]),
        g("default", &[]),
    ];
    let groups_graph = groups::build(&groups, &[]).unwrap();

    let mut nodes: Vec<Vertex> = Vec::new();
    let a = node(&mut nodes, &order, PLUGIN_A, "A");
    let b = node(&mut nodes, &order, PLUGIN_B, "B");
    let c = node(&mut nodes, &order, PLUGIN_C, "C");
    let d = node(&mut nodes, &order, PLUGIN_D, "D");

    let over = over.finish(&order, &mut nodes);
    let mut graph = PluginGraph::new(nodes, &over);
    graph.add_group_edges(&groups_graph).unwrap();

    // Should be A.esp -> B.esp -> D.esp
    //                 -> C.esp ->
    assert!(graph.has_edge(a, b));
    assert!(graph.has_edge(b, d));
    assert!(graph.has_edge(a, c));
    assert!(graph.has_edge(c, d));
    assert!(!graph.has_edge(b, c));
    assert!(!graph.has_edge(c, b));

    assert!(graph.check_for_cycles().is_ok());
}

#[test]
fn should_add_edges_across_the_merge_point_of_branches_in_the_groups_graph() {
    let order = [PLUGIN_A, PLUGIN_B, PLUGIN_C, PLUGIN_D, PLUGIN_E];
    let mut over = TestOverlaps::default();
    let groups_graph = groups::build(&fixture_groups(), &fixture_user_groups()).unwrap();

    let groups = vec![
        g("A", &[]),
        g("B", &["A"]),
        g("C", &["A"]),
        g("D", &["B", "C"]),
        g("E", &["D"]),
        g("default", &[]),
    ];
    let groups_graph = groups::build(&groups, &[]).unwrap();

    let mut nodes: Vec<Vertex> = Vec::new();
    let a = node(&mut nodes, &order, PLUGIN_A, "A");
    let b = node(&mut nodes, &order, PLUGIN_B, "B");
    let c = node(&mut nodes, &order, PLUGIN_C, "C");
    let d = node(&mut nodes, &order, PLUGIN_D, "D");
    let e = node(&mut nodes, &order, PLUGIN_E, "E");

    let over = over.finish(&order, &mut nodes);
    let mut graph = PluginGraph::new(nodes, &over);
    graph.add_edge(d, c, EdgeType::Master);

    graph.add_group_edges(&groups_graph).unwrap();

    // Should be A.esp -> B.esp -> D.esp -> C.esp -> E.esp
    assert!(graph.has_edge(d, c));
    assert!(graph.has_edge(a, b));
    assert!(graph.has_edge(b, d));
    assert!(graph.has_edge(d, e));
    assert!(graph.has_edge(a, c));
    assert!(graph.has_edge(c, e));
    assert!(!graph.has_edge(b, c));
    assert!(!graph.has_edge(c, b));
    assert!(!graph.has_edge(c, d));

    assert!(graph.check_for_cycles().is_ok());
}

#[test]
fn should_handle_a_groups_graph_with_multiple_successive_branches() {
    const PLUGIN_G: &str = "G.esp";
    let order = [
        PLUGIN_A, PLUGIN_B, PLUGIN_C, PLUGIN_D, PLUGIN_E, PLUGIN_F, PLUGIN_G,
    ];
    let mut over = TestOverlaps::default();
    let groups_graph = groups::build(&fixture_groups(), &fixture_user_groups()).unwrap();

    let groups = vec![
        g("A", &[]),
        g("B", &["A"]),
        g("C", &["A"]),
        g("D", &["B", "C"]),
        g("E", &["D"]),
        g("F", &["D"]),
        g("G", &["E", "F"]),
        g("default", &[]),
    ];
    let groups_graph = groups::build(&groups, &[]).unwrap();

    let mut nodes: Vec<Vertex> = Vec::new();
    let a = node(&mut nodes, &order, PLUGIN_A, "A");
    let b = node(&mut nodes, &order, PLUGIN_B, "B");
    let c = node(&mut nodes, &order, PLUGIN_C, "C");
    let d = node(&mut nodes, &order, PLUGIN_D, "D");
    let e = node(&mut nodes, &order, PLUGIN_E, "E");
    let f = node(&mut nodes, &order, PLUGIN_E, "F");
    let g = node(&mut nodes, &order, PLUGIN_E, "G");

    let over = over.finish(&order, &mut nodes);
    let mut graph = PluginGraph::new(nodes, &over);
    graph.add_group_edges(&groups_graph).unwrap();

    // Should be:
    // A.esp -> B.esp -> D.esp -> E.esp -> G.esp
    //       -> C.esp ->       -> F.esp ->
    assert!(graph.has_edge(a, b));
    assert!(graph.has_edge(a, c));
    assert!(graph.has_edge(b, d));
    assert!(graph.has_edge(c, d));
    assert!(graph.has_edge(d, e));
    assert!(graph.has_edge(d, f));
    assert!(graph.has_edge(e, g));
    assert!(graph.has_edge(f, g));
    assert!(!graph.has_edge(b, c));
    assert!(!graph.has_edge(c, b));
    assert!(!graph.has_edge(e, f));
    assert!(!graph.has_edge(f, e));

    assert!(graph.check_for_cycles().is_ok());
}

#[test]
fn should_find_all_groups_in_all_paths_between_two_groups_when_ignoring_a_plugin() {
    let order = [PLUGIN_A, PLUGIN_B, PLUGIN_C, PLUGIN_D, PLUGIN_E];
    let mut over = TestOverlaps::default();
    let groups_graph = groups::build(&fixture_groups(), &fixture_user_groups()).unwrap();

    let groups = vec![
        g("A", &[]),
        g("B", &["A"]),
        g("C", &["B"]),
        g("D", &["C"]),
        g("default", &[]),
    ];
    let groups_graph = groups::build(&groups, &[]).unwrap();

    let mut nodes: Vec<Vertex> = Vec::new();
    let a = node(&mut nodes, &order, PLUGIN_A, "A");
    let b = node(&mut nodes, &order, PLUGIN_B, "B");
    let c = node(&mut nodes, &order, PLUGIN_C, "C");
    let d = node(&mut nodes, &order, PLUGIN_D, "D");
    let e = node(&mut nodes, &order, PLUGIN_E, "default");

    let over = over.finish(&order, &mut nodes);
    let mut graph = PluginGraph::new(nodes, &over);
    graph.add_edge(e, a, EdgeType::Master);

    graph.add_group_edges(&groups_graph).unwrap();

    // Should be:
    // A.esp -> B.esp -> D.esp -> E.esp -> G.esp
    //       -> C.esp ->       -> F.esp ->
    assert!(graph.has_edge(a, b));
    assert!(graph.has_edge(b, c));
    assert!(graph.has_edge(c, d));

    assert!(!graph.has_edge(a, e));
    assert!(!graph.has_edge(b, e));
    assert!(!graph.has_edge(c, e));
    assert!(!graph.has_edge(d, e));

    assert!(graph.check_for_cycles().is_ok());
}

#[test]
fn should_handle_isolated_groups() {
    let order = [PLUGIN_A, PLUGIN_B, PLUGIN_C];
    let mut over = TestOverlaps::default();
    let groups_graph = groups::build(&fixture_groups(), &fixture_user_groups()).unwrap();

    let groups = vec![g("A", &[]), g("B", &["A"]), g("C", &[]), g("default", &[])];
    let groups_graph = groups::build(&groups, &[]).unwrap();

    let mut nodes: Vec<Vertex> = Vec::new();
    let a = node(&mut nodes, &order, PLUGIN_A, "A");
    let b = node(&mut nodes, &order, PLUGIN_B, "B");
    let c = node(&mut nodes, &order, PLUGIN_C, "C");

    let over = over.finish(&order, &mut nodes);
    let mut graph = PluginGraph::new(nodes, &over);
    graph.add_group_edges(&groups_graph).unwrap();

    // Should be A.esp -> B.esp
    //           C.esp
    assert!(graph.has_edge(a, b));
    assert!(!graph.has_edge(a, c));
    assert!(!graph.has_edge(c, a));
    assert!(!graph.has_edge(b, c));
    assert!(!graph.has_edge(c, b));

    assert!(graph.check_for_cycles().is_ok());
}

#[test]
fn should_handle_disconnected_group_graphs() {
    let order = [PLUGIN_A, PLUGIN_B, PLUGIN_C, PLUGIN_D];
    let mut over = TestOverlaps::default();
    let groups_graph = groups::build(&fixture_groups(), &fixture_user_groups()).unwrap();

    let groups = vec![
        g("A", &[]),
        g("B", &["A"]),
        g("C", &[]),
        g("D", &["C"]),
        g("default", &[]),
    ];
    let groups_graph = groups::build(&groups, &[]).unwrap();

    let mut nodes: Vec<Vertex> = Vec::new();
    let a = node(&mut nodes, &order, PLUGIN_A, "A");
    let b = node(&mut nodes, &order, PLUGIN_B, "B");
    let c = node(&mut nodes, &order, PLUGIN_C, "C");
    let d = node(&mut nodes, &order, PLUGIN_D, "D");

    let over = over.finish(&order, &mut nodes);
    let mut graph = PluginGraph::new(nodes, &over);
    graph.add_group_edges(&groups_graph).unwrap();

    // Should be A.esp -> B.esp
    //           C.esp -> D.esp
    assert!(graph.has_edge(a, b));
    assert!(graph.has_edge(c, d));
    assert!(!graph.has_edge(a, c));
    assert!(!graph.has_edge(a, d));
    assert!(!graph.has_edge(b, c));
    assert!(!graph.has_edge(b, d));
    assert!(!graph.has_edge(c, a));
    assert!(!graph.has_edge(c, b));
    assert!(!graph.has_edge(d, a));
    assert!(!graph.has_edge(d, b));

    assert!(graph.check_for_cycles().is_ok());
}

#[test]
fn should_add_edges_across_the_merge_point_of_two_root_node_paths() {
    let order = [PLUGIN_A, PLUGIN_B, PLUGIN_C, PLUGIN_D];
    let mut over = TestOverlaps::default();
    let groups_graph = groups::build(&fixture_groups(), &fixture_user_groups()).unwrap();

    let groups = vec![
        g("A", &[]),
        g("B", &[]),
        g("C", &["A", "B"]),
        g("D", &["C"]),
        g("default", &[]),
    ];
    let groups_graph = groups::build(&groups, &[]).unwrap();

    let mut nodes: Vec<Vertex> = Vec::new();
    let a = node(&mut nodes, &order, PLUGIN_A, "A");
    let b = node(&mut nodes, &order, PLUGIN_B, "B");
    let c = node(&mut nodes, &order, PLUGIN_C, "C");
    let d = node(&mut nodes, &order, PLUGIN_D, "D");

    let over = over.finish(&order, &mut nodes);
    let mut graph = PluginGraph::new(nodes, &over);
    graph.add_edge(c, b, EdgeType::Master);

    graph.add_group_edges(&groups_graph).unwrap();

    // Should be A.esp -> C.esp -> D.esp
    //           B.esp ---------->
    assert!(graph.has_edge(c, b));
    assert!(graph.has_edge(a, c));
    assert!(graph.has_edge(c, d));
    assert!(graph.has_edge(b, d));
    assert!(!graph.has_edge(a, b));
    assert!(!graph.has_edge(b, a));

    assert!(graph.check_for_cycles().is_ok());
}

#[test]
fn should_not_depend_on_plugin_graph_node_order() {
    let order = [PLUGIN_A1, PLUGIN_A2, PLUGIN_B, PLUGIN_C];
    let mut over = TestOverlaps::default();
    let groups_graph = groups::build(&fixture_groups(), &fixture_user_groups()).unwrap();

    let groups = vec![
        g("A", &[]),
        g("B", &["A"]),
        g("C", &["B"]),
        g("default", &[]),
    ];
    let groups_graph = groups::build(&groups, &[]).unwrap();

    let a1 = (PLUGIN_A1, "A");
    let a2 = (PLUGIN_A2, "A");
    let b = (PLUGIN_B, "B");
    let c = (PLUGIN_C, "C");

    let variations = &[
        [a1, a2, b, c],
        [a1, a2, c, b],
        [a1, b, c, a2],
        [a1, b, a2, c],
        [a1, c, a2, b],
        [a1, c, b, a2],
        [a2, a1, b, c],
        [a2, a1, c, b],
        [a2, b, c, a1],
        [a2, b, a1, c],
        [a2, c, a1, b],
        [a2, c, b, a1],
        [b, a2, a1, c],
        [b, a2, c, a1],
        [b, a1, c, a2],
        [b, a1, a2, c],
        [b, c, a2, a1],
        [b, c, a1, a2],
        [c, a2, b, a1],
        [c, a2, a1, b],
        [c, b, a1, a2],
        [c, b, a2, a1],
        [c, a1, a2, b],
        [c, a1, b, a2],
    ];

    for plugins in variations {
        let mut nodes: Vec<Vertex> = Vec::new();

        for plugin in plugins {
            node(&mut nodes, &order, plugin.0, plugin.1);
        }

        let over = over.finish(&order, &mut nodes);
        let mut graph = PluginGraph::new(nodes, &over);
        let a1 = graph.node_by_name(PLUGIN_A1);
        let a2 = graph.node_by_name(PLUGIN_A2);
        let b = graph.node_by_name(PLUGIN_B);
        let c = graph.node_by_name(PLUGIN_C);

        graph.add_edge(c, a1, EdgeType::Master);

        graph.add_group_edges(&groups_graph).unwrap();

        // Should be A2.esp -> C.esp -> A1.esp -> B.esp
        //           A2.esp -------------------->
        assert!(graph.has_edge(c, a1));
        assert!(graph.has_edge(a1, b));
        assert!(graph.has_edge(a2, b));
        assert!(graph.has_edge(a2, c));
        assert!(!graph.has_edge(a1, a2));
        assert!(!graph.has_edge(a2, a1));
        assert!(!graph.has_edge(b, c));

        assert!(graph.check_for_cycles().is_ok());
    }
}

#[test]
fn should_start_searching_from_root_groups_before_going_in_lexicographical_order() {
    let order = [PLUGIN_A, PLUGIN_B, PLUGIN_C, PLUGIN_D];
    let mut over = TestOverlaps::default();
    let groups_graph = groups::build(&fixture_groups(), &fixture_user_groups()).unwrap();

    let groups = vec![
        g("D", &[]),
        g("A", &["D"]),
        g("B", &["A"]),
        g("C", &["B"]),
        g("default", &[]),
    ];
    let groups_graph = groups::build(&groups, &[]).unwrap();

    let mut nodes: Vec<Vertex> = Vec::new();
    let a = node(&mut nodes, &order, PLUGIN_A, "A");
    let b = node(&mut nodes, &order, PLUGIN_B, "B");
    let c = node(&mut nodes, &order, PLUGIN_C, "C");
    let d = node(&mut nodes, &order, PLUGIN_D, "D");

    let over = over.finish(&order, &mut nodes);
    let mut graph = PluginGraph::new(nodes, &over);
    graph.add_edge(c, d, EdgeType::Master);

    graph.add_group_edges(&groups_graph).unwrap();

    // Should be C.esp -> D.esp -> A.esp -> B.esp
    // Processing groups lexicographically would give:
    // A.esp -> B.esp -> C.esp -> D.esp
    assert!(graph.has_edge(c, d));
    assert!(graph.has_edge(d, a));
    assert!(graph.has_edge(d, b));
    assert!(graph.has_edge(a, b));

    assert!(!graph.has_edge(a, c));
    assert!(!graph.has_edge(a, d));
    assert!(!graph.has_edge(b, a));
    assert!(!graph.has_edge(b, c));
    assert!(!graph.has_edge(b, d));
    assert!(!graph.has_edge(d, c));

    assert!(graph.check_for_cycles().is_ok());
}

#[test]
fn should_start_searching_from_the_root_group_with_the_longest_path() {
    let order = [PLUGIN_A, PLUGIN_B, PLUGIN_C, PLUGIN_D, PLUGIN_E, PLUGIN_F];
    let mut over = TestOverlaps::default();
    let groups_graph = groups::build(&fixture_groups(), &fixture_user_groups()).unwrap();

    let groups = vec![
        g("D", &[]),
        g("B", &["D"]),
        g("C", &["B"]),
        g("A", &[]),
        g("E", &["C", "A"]),
        g("F", &["E"]),
        g("default", &[]),
    ];
    let groups_graph = groups::build(&groups, &[]).unwrap();

    let mut nodes: Vec<Vertex> = Vec::new();
    let a = node(&mut nodes, &order, PLUGIN_A, "A");
    let b = node(&mut nodes, &order, PLUGIN_B, "B");
    let c = node(&mut nodes, &order, PLUGIN_C, "C");
    let d = node(&mut nodes, &order, PLUGIN_D, "D");
    let e = node(&mut nodes, &order, PLUGIN_E, "E");
    let f = node(&mut nodes, &order, PLUGIN_F, "F");

    let over = over.finish(&order, &mut nodes);
    let mut graph = PluginGraph::new(nodes, &over);
    graph.add_edge(f, b, EdgeType::Master);

    graph.add_group_edges(&groups_graph).unwrap();

    // Should be          D.esp -> B.esp -> C.esp -> E.esp
    //  A.esp -> F.esp ----------> B.esp
    //  A.esp -------------------------------------> E.esp
    assert!(graph.has_edge(d, b));
    assert!(graph.has_edge(b, c));
    assert!(graph.has_edge(c, e));
    assert!(graph.has_edge(a, f));
    assert!(graph.has_edge(a, e));
    assert!(graph.has_edge(f, b));

    assert!(graph.check_for_cycles().is_ok());
}

#[test]
fn does_not_start_searching_with_the_longest_path() {
    let order = [PLUGIN_A, PLUGIN_B, PLUGIN_C, PLUGIN_D, PLUGIN_E];
    let mut over = TestOverlaps::default();
    let groups_graph = groups::build(&fixture_groups(), &fixture_user_groups()).unwrap();

    let groups = vec![
        g("A", &[]),
        g("B", &["A"]),
        g("C", &["A"]),
        g("D", &["C"]),
        g("E", &["B", "D"]),
        g("default", &[]),
    ];
    let groups_graph = groups::build(&groups, &[]).unwrap();

    let mut nodes: Vec<Vertex> = Vec::new();
    let a = node(&mut nodes, &order, PLUGIN_A, "A");
    let b = node(&mut nodes, &order, PLUGIN_B, "B");
    let c = node(&mut nodes, &order, PLUGIN_C, "C");
    let d = node(&mut nodes, &order, PLUGIN_D, "D");
    let e = node(&mut nodes, &order, PLUGIN_E, "E");

    let over = over.finish(&order, &mut nodes);
    let mut graph = PluginGraph::new(nodes, &over);
    graph.add_edge(e, c, EdgeType::Master);

    graph.add_group_edges(&groups_graph).unwrap();

    // Should be A.esp -> B.esp -> E.esp -> C.esp -> D.esp
    assert!(graph.has_edge(a, b));
    assert!(graph.has_edge(b, e));
    assert!(graph.has_edge(e, c));
    assert!(graph.has_edge(c, d));

    assert!(graph.check_for_cycles().is_ok());
}

#[test]
fn should_not_add_edges_between_non_overlapping_plugins() {
    let order = [PLUGIN_A, PLUGIN_B];
    let mut over = TestOverlaps::default();
    let groups_graph = groups::build(&fixture_groups(), &fixture_user_groups()).unwrap();

    let mut nodes: Vec<Vertex> = Vec::new();
    let a = node(&mut nodes, &order, PLUGIN_A, "default");
    let b = node(&mut nodes, &order, PLUGIN_B, "default");

    let over = over.finish(&order, &mut nodes);
    let mut graph = PluginGraph::new(nodes, &over);
    graph.add_overlap_edges().unwrap();

    assert!(!graph.has_edge(a, b));
    assert!(!graph.has_edge(b, a));
}

#[test]
fn should_not_add_edges_between_overlapping_plugins_with_equal_override_counts() {
    let order = [PLUGIN_A, PLUGIN_B];
    let mut over = TestOverlaps::default();
    let groups_graph = groups::build(&fixture_groups(), &fixture_user_groups()).unwrap();

    let a = PLUGIN_A;
    over.count(a, 1);
    over.records(a, PLUGIN_B);

    let b = PLUGIN_B;
    over.count(b, 1);

    let mut nodes: Vec<Vertex> = Vec::new();
    let a = node(&mut nodes, &order, PLUGIN_A, "default");
    let b = node(&mut nodes, &order, PLUGIN_B, "default");

    let over = over.finish(&order, &mut nodes);
    let mut graph = PluginGraph::new(nodes, &over);
    graph.add_overlap_edges().unwrap();

    assert!(!graph.has_edge(a, b));
    assert!(!graph.has_edge(b, a));
}

#[test]
fn should_add_edge_between_overlapping_plugins_with_unequal_override_counts() {
    let order = [PLUGIN_A, PLUGIN_B];
    let mut over = TestOverlaps::default();
    let groups_graph = groups::build(&fixture_groups(), &fixture_user_groups()).unwrap();

    let a = PLUGIN_A;
    over.count(a, 2);
    over.records(a, PLUGIN_B);

    let b = PLUGIN_B;
    over.count(b, 1);

    let mut nodes: Vec<Vertex> = Vec::new();
    let a = node(&mut nodes, &order, PLUGIN_A, "default");
    let b = node(&mut nodes, &order, PLUGIN_B, "default");

    let over = over.finish(&order, &mut nodes);
    let mut graph = PluginGraph::new(nodes, &over);
    graph.add_overlap_edges().unwrap();

    assert_eq!(EdgeType::RecordOverlap, graph.edge_type(a, b));
    assert!(!graph.has_edge(b, a));
}

#[test]
fn should_not_add_edge_between_non_overlapping_plugins_with_unequal_override_counts() {
    let order = [PLUGIN_A, PLUGIN_B];
    let mut over = TestOverlaps::default();
    let groups_graph = groups::build(&fixture_groups(), &fixture_user_groups()).unwrap();

    let a = PLUGIN_A;
    over.count(a, 2);

    let b = PLUGIN_B;
    over.count(b, 1);

    let mut nodes: Vec<Vertex> = Vec::new();
    let a = node(&mut nodes, &order, PLUGIN_A, "default");
    let b = node(&mut nodes, &order, PLUGIN_B, "default");

    let over = over.finish(&order, &mut nodes);
    let mut graph = PluginGraph::new(nodes, &over);
    graph.add_overlap_edges().unwrap();

    assert!(!graph.has_edge(a, b));
    assert!(!graph.has_edge(b, a));
}

#[test]
fn should_not_add_edge_between_plugins_with_asset_overlap_and_equal_asset_counts() {
    let order = [PLUGIN_A, PLUGIN_B];
    let mut over = TestOverlaps::default();
    let groups_graph = groups::build(&fixture_groups(), &fixture_user_groups()).unwrap();

    let a = PLUGIN_A;
    over.asset_count(a, 1);
    over.assets_with(a, PLUGIN_B);

    let b = PLUGIN_B;
    over.asset_count(b, 1);

    let mut nodes: Vec<Vertex> = Vec::new();
    let a = node(&mut nodes, &order, PLUGIN_A, "default");
    let b = node(&mut nodes, &order, PLUGIN_B, "default");

    let over = over.finish(&order, &mut nodes);
    let mut graph = PluginGraph::new(nodes, &over);
    graph.add_overlap_edges().unwrap();

    assert!(!graph.has_edge(a, b));
    assert!(!graph.has_edge(b, a));
}

#[test]
fn should_not_add_edge_between_plugins_with_no_asset_overlap_and_unequal_asset_counts() {
    let order = [PLUGIN_A, PLUGIN_B];
    let mut over = TestOverlaps::default();
    let groups_graph = groups::build(&fixture_groups(), &fixture_user_groups()).unwrap();

    let a = PLUGIN_A;
    over.asset_count(a, 2);

    let b = PLUGIN_B;
    over.asset_count(b, 1);

    let mut nodes: Vec<Vertex> = Vec::new();
    let a = node(&mut nodes, &order, PLUGIN_A, "default");
    let b = node(&mut nodes, &order, PLUGIN_B, "default");

    let over = over.finish(&order, &mut nodes);
    let mut graph = PluginGraph::new(nodes, &over);
    graph.add_overlap_edges().unwrap();

    assert!(!graph.has_edge(a, b));
    assert!(!graph.has_edge(b, a));
}

#[test]
fn should_add_edge_between_plugins_with_asset_overlap_and_unequal_asset_counts() {
    let order = [PLUGIN_A, PLUGIN_B];
    let mut over = TestOverlaps::default();
    let groups_graph = groups::build(&fixture_groups(), &fixture_user_groups()).unwrap();

    let a = PLUGIN_A;
    over.asset_count(a, 2);
    over.assets_with(a, PLUGIN_B);

    let b = PLUGIN_B;
    over.asset_count(b, 1);

    let mut nodes: Vec<Vertex> = Vec::new();
    let a = node(&mut nodes, &order, PLUGIN_A, "default");
    let b = node(&mut nodes, &order, PLUGIN_B, "default");

    let over = over.finish(&order, &mut nodes);
    let mut graph = PluginGraph::new(nodes, &over);
    graph.add_overlap_edges().unwrap();

    assert_eq!(EdgeType::AssetOverlap, graph.edge_type(a, b));
    assert!(!graph.has_edge(b, a));
}

#[test]
fn should_add_edge_between_overlapping_plugins_with_asset_overlap_and_equal_override_count_and_unequal_asset_counts()
 {
    let order = [PLUGIN_A, PLUGIN_B];
    let mut over = TestOverlaps::default();
    let groups_graph = groups::build(&fixture_groups(), &fixture_user_groups()).unwrap();

    let a = PLUGIN_A;
    over.asset_count(a, 2);
    over.records(a, PLUGIN_B);
    over.assets_with(a, PLUGIN_B);

    let b = PLUGIN_B;
    over.asset_count(b, 1);

    let mut nodes: Vec<Vertex> = Vec::new();
    let a = node(&mut nodes, &order, PLUGIN_A, "default");
    let b = node(&mut nodes, &order, PLUGIN_B, "default");

    let over = over.finish(&order, &mut nodes);
    let mut graph = PluginGraph::new(nodes, &over);
    graph.add_overlap_edges().unwrap();

    assert_eq!(EdgeType::AssetOverlap, graph.edge_type(a, b));
    assert!(!graph.has_edge(b, a));
}

#[test]
fn should_add_edge_between_plugins_with_asset_overlap_and_unequal_override_count_and_unequal_asset_counts()
 {
    let order = [PLUGIN_A, PLUGIN_B];
    let mut over = TestOverlaps::default();
    let groups_graph = groups::build(&fixture_groups(), &fixture_user_groups()).unwrap();

    let a = PLUGIN_A;
    over.count(a, 1);
    over.asset_count(a, 2);
    over.assets_with(a, PLUGIN_B);

    let b = PLUGIN_B;
    over.count(b, 2);
    over.asset_count(b, 1);

    let mut nodes: Vec<Vertex> = Vec::new();
    let a = node(&mut nodes, &order, PLUGIN_A, "default");
    let b = node(&mut nodes, &order, PLUGIN_B, "default");

    let over = over.finish(&order, &mut nodes);
    let mut graph = PluginGraph::new(nodes, &over);
    graph.add_overlap_edges().unwrap();

    assert_eq!(EdgeType::AssetOverlap, graph.edge_type(a, b));
    assert!(!graph.has_edge(b, a));
}

#[test]
fn should_choose_record_overlap_over_asset_overlap() {
    let order = [PLUGIN_A, PLUGIN_B];
    let mut over = TestOverlaps::default();
    let groups_graph = groups::build(&fixture_groups(), &fixture_user_groups()).unwrap();

    let a = PLUGIN_A;
    over.count(a, 2);
    over.asset_count(a, 1);
    over.records(a, PLUGIN_B);
    over.assets_with(a, PLUGIN_B);

    let b = PLUGIN_B;
    over.count(b, 1);
    over.asset_count(b, 2);

    let mut nodes: Vec<Vertex> = Vec::new();
    let a = node(&mut nodes, &order, PLUGIN_A, "default");
    let b = node(&mut nodes, &order, PLUGIN_B, "default");

    let over = over.finish(&order, &mut nodes);
    let mut graph = PluginGraph::new(nodes, &over);
    graph.add_overlap_edges().unwrap();

    assert_eq!(EdgeType::RecordOverlap, graph.edge_type(a, b));
    assert!(!graph.has_edge(b, a));
}

#[test]
fn should_not_error_on_a_graph_with_one_node() {
    let order = [PLUGIN_A, PLUGIN_B];
    let mut over = TestOverlaps::default();
    let groups_graph = groups::build(&fixture_groups(), &fixture_user_groups()).unwrap();

    let mut nodes: Vec<Vertex> = Vec::new();
    node(&mut nodes, &order, PLUGIN_A, "default");

    let over = over.finish(&order, &mut nodes);
    let mut graph = PluginGraph::new(nodes, &over);
    assert!(graph.add_tie_break_edges().is_ok());
}

// libloot's tests of the user group edge type (`add_group_edges`).

fn user_group_node(nodes: &mut Vec<Vertex>, order: &[&str], name: &str, group: &str) -> usize {
    let index = node(nodes, order, name, group);
    nodes[index].group_is_user = true;
    index
}

fn group_edge_kind(
    build: impl Fn(&mut Vec<Vertex>, &[&str]) -> (usize, usize),
    order: &[&str],
) -> EdgeType {
    let groups_graph = groups::build(&fixture_groups(), &fixture_user_groups()).unwrap();
    let mut nodes = Vec::new();
    let (from, to) = build(&mut nodes, order);
    let over = TestOverlaps::default().finish(order, &mut nodes);
    let mut graph = PluginGraph::new(nodes, &over);
    graph.add_group_edges(&groups_graph).unwrap();
    assert!(graph.check_for_cycles().is_ok());
    graph
        .added
        .iter()
        .find(|&&(a, b, _)| (a, b) == (from, to))
        .map(|&(_, _, kind)| kind)
        .expect("an edge was added")
}

#[test]
fn should_add_user_group_edge_if_source_plugin_is_in_group_due_to_user_metadata() {
    let kind = group_edge_kind(
        |nodes, order| {
            let a = user_group_node(nodes, order, PLUGIN_A, "A");
            let b = node(nodes, order, PLUGIN_B, "B");
            (a, b)
        },
        &[PLUGIN_A, PLUGIN_B],
    );
    assert_eq!(kind, EdgeType::UserGroup);
}

#[test]
fn should_add_user_group_edge_if_target_plugin_is_in_group_due_to_user_metadata() {
    let kind = group_edge_kind(
        |nodes, order| {
            let a = node(nodes, order, PLUGIN_A, "A");
            let b = user_group_node(nodes, order, PLUGIN_B, "B");
            (a, b)
        },
        &[PLUGIN_A, PLUGIN_B],
    );
    assert_eq!(kind, EdgeType::UserGroup);
}

#[test]
fn should_add_user_group_edge_if_group_path_starts_with_user_metadata() {
    let kind = group_edge_kind(
        |nodes, order| {
            let b = node(nodes, order, PLUGIN_B, "B");
            let d = node(nodes, order, PLUGIN_D, "default");
            (b, d)
        },
        &[PLUGIN_B, PLUGIN_D],
    );
    assert_eq!(kind, EdgeType::UserGroup);
}

#[test]
fn should_add_user_group_edge_if_group_path_ends_with_user_metadata() {
    let kind = group_edge_kind(
        |nodes, order| {
            let a = node(nodes, order, PLUGIN_A, "A");
            let c = node(nodes, order, PLUGIN_C, "C");
            (a, c)
        },
        &[PLUGIN_A, PLUGIN_C],
    );
    assert_eq!(kind, EdgeType::UserGroup);
}

#[test]
fn should_add_user_group_edge_if_group_path_involves_user_metadata() {
    let kind = group_edge_kind(
        |nodes, order| {
            let a = node(nodes, order, PLUGIN_A, "A");
            let d = node(nodes, order, PLUGIN_D, "default");
            (a, d)
        },
        &[PLUGIN_A, PLUGIN_D],
    );
    assert_eq!(kind, EdgeType::UserGroup);
}
