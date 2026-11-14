// SPDX-License-Identifier: GPL-3.0-or-later
//
// Ported to Rust from libloot (https://github.com/loot/libloot, `src/sorting/groups.rs`, GPL-3.0-or-later,
// Copyright (C) the libloot authors). See NOTICE.md.

//! The graph of groups: which group loads after which.

use std::cmp::Reverse;
use std::collections::BTreeMap;

use super::search::{Adjacency, Colour, DfsVisitor, Edge, depth_first_search, find_cycle};
use super::{DEFAULT_GROUP, EdgeType, GraphError, GroupInput};

/// Groups as vertices (in lexicographical order of their names), an edge from a group to each group that loads
/// after it.
#[derive(Debug)]
pub(super) struct GroupGraph {
    pub(super) names: Vec<String>,
    pub(super) adjacency: Adjacency,
    has_incoming: Vec<bool>,
}

impl GroupGraph {
    pub(super) fn node(&self, name: &str) -> Option<usize> {
        self.names.iter().position(|candidate| candidate == name)
    }

    pub(super) fn default_node(&self) -> usize {
        self.node(DEFAULT_GROUP)
            .expect("the default group is always added")
    }

    fn is_root(&self, node: usize) -> bool {
        !self.has_incoming[node]
    }
}

/// Build the graph. Definitions of one name are merged (their `after` lists joined), the `default` group is added
/// when missing, a group that loads after a group nobody defined is left without that edge (libloot refuses such a
/// masterlist; the caller reports it), and a cycle is an error naming the groups on it.
pub(super) fn build(groups: &[GroupInput]) -> Result<GroupGraph, GraphError> {
    let mut merged: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    merged.entry(DEFAULT_GROUP).or_default();
    for group in groups {
        merged
            .entry(group.name.as_str())
            .or_default()
            .extend(group.after.iter().map(String::as_str));
    }
    // BTreeMap iterates in the byte order of the names, which is the order libloot sorts them in.
    let names: Vec<String> = merged.keys().map(|name| (*name).to_string()).collect();
    let mut adjacency: Adjacency = vec![Vec::new(); names.len()];
    let mut has_incoming = vec![false; names.len()];
    for (index, afters) in merged.values().enumerate() {
        let mut afters = afters.clone();
        afters.sort_unstable();
        afters.dedup();
        for other in afters {
            let Ok(other_index) = names.binary_search_by(|name| name.as_str().cmp(other)) else {
                continue;
            };
            if !adjacency[other_index].iter().any(|&(to, _)| to == index) {
                adjacency[other_index].push((index, EdgeType::MasterlistLoadAfter));
                has_incoming[index] = true;
            }
        }
    }
    if let Some(cycle) = find_cycle(&adjacency) {
        return Err(GraphError::GroupCycle(
            cycle.iter().map(|&(node, _)| names[node].clone()).collect(),
        ));
    }
    Ok(GroupGraph {
        names,
        adjacency,
        has_incoming,
    })
}

struct PathLength {
    longest: usize,
    current: usize,
}

impl DfsVisitor for PathLength {
    fn tree_edge(&mut self, _: Edge) {}
    fn forward_or_cross_edge(&mut self, _: Edge) {}
    fn back_edge(&mut self, _: Edge) {}
    fn discover(&mut self, _: usize) {
        self.current += 1;
        self.longest = self.longest.max(self.current);
    }
    fn finish(&mut self, _: usize) {
        self.current -= 1;
    }
}

/// The vertices with the roots first, longest path first, and otherwise in their order.
pub(super) fn sorted_nodes(graph: &GroupGraph) -> Vec<usize> {
    let mut nodes: Vec<(usize, bool, usize)> = (0..graph.names.len())
        .map(|node| {
            if graph.is_root(node) {
                let mut visitor = PathLength {
                    longest: 0,
                    current: 0,
                };
                let mut colours = vec![Colour::White; graph.names.len()];
                depth_first_search(&graph.adjacency, &mut colours, node, &mut visitor);
                (node, true, visitor.longest)
            } else {
                (node, false, 0)
            }
        })
        .collect();
    nodes.sort_by_key(|&(_, root, length)| Reverse((root, length)));
    nodes.into_iter().map(|(node, _, _)| node).collect()
}
