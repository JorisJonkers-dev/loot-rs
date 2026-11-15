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
///
/// As libloot's `build_groups_graph`: the masterlist's groups are the first vertices, in the byte order of their
/// names, then the groups only the userlist defines, in the same order; the masterlist's edges come first
/// ([`EdgeType::MasterlistLoadAfter`]), then the userlist's ([`EdgeType::UserLoadAfter`], which also retypes a
/// masterlist edge between the same groups). A masterlist group cannot load after a group only the userlist defines.
pub(super) fn build(
    groups: &[GroupInput],
    user_groups: &[GroupInput],
) -> Result<GroupGraph, GraphError> {
    let merge = |list: &[GroupInput]| {
        let mut merged: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for group in list {
            merged
                .entry(group.name.clone())
                .or_default()
                .extend(group.after.iter().cloned());
        }
        merged
    };
    let mut masterlist = merge(groups);
    masterlist.entry(DEFAULT_GROUP.to_string()).or_default();
    let userlist = merge(user_groups);
    // BTreeMap iterates in the byte order of the names, which is the order libloot sorts them in.
    let mut names: Vec<String> = masterlist.keys().cloned().collect();
    names.extend(
        userlist
            .keys()
            .filter(|name| !masterlist.contains_key(*name))
            .cloned(),
    );
    let mut adjacency: Adjacency = vec![Vec::new(); names.len()];
    let mut has_incoming = vec![false; names.len()];
    let position =
        |wanted: &str, known: usize| names[..known].iter().position(|name| name == wanted);
    let master_count = masterlist.len();
    for (definitions, kind, known) in [
        (&masterlist, EdgeType::MasterlistLoadAfter, master_count),
        (&userlist, EdgeType::UserLoadAfter, names.len()),
    ] {
        for (name, afters) in definitions {
            let Some(index) = position(name, names.len()) else {
                continue;
            };
            let mut afters = afters.clone();
            afters.sort_unstable();
            afters.dedup();
            for other in afters {
                let Some(other_index) = position(&other, known) else {
                    continue;
                };
                if let Some(edge) = adjacency[other_index]
                    .iter_mut()
                    .find(|(to, _)| *to == index)
                {
                    edge.1 = kind;
                } else {
                    adjacency[other_index].push((index, kind));
                    has_incoming[index] = true;
                }
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
