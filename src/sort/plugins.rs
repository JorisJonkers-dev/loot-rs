// SPDX-License-Identifier: GPL-3.0-or-later
//
// Ported to Rust from libloot (https://github.com/loot/libloot, `src/sorting/plugins.rs`, GPL-3.0-or-later,
// Copyright (C) the libloot authors). See NOTICE.md. The user metadata (userlist) edge types and blueprint masters of
// libloot are left out: this crate reads one masterlist and sorts Fallout: New Vegas.

//! The plugin graph of one partition (the masters, or the other plugins) and the edges libloot adds to it, in the
//! order it adds them.

use std::collections::HashMap;

use super::groups::{GroupGraph, sorted_nodes};
use super::search::{
    Adjacency, Colour, DfsVisitor, Edge, Event, Scratch, bidirectional_bfs, depth_first_search,
    find_cycle, topological_sort,
};
use super::{EdgeType, GraphError, Overlaps, SortedEdge, WORK_BUDGET};

/// What the sorter needs to know of one plugin, with every name already looked up.
#[derive(Debug, Clone)]
pub(super) struct Vertex {
    /// Index in the list the caller gave (also the current load order).
    pub(super) input: usize,
    pub(super) name: String,
    pub(super) group: String,
    pub(super) override_records: usize,
    pub(super) assets: usize,
    /// Vertices (of this graph) the plugin must load after, by kind.
    pub(super) masters: Vec<usize>,
    pub(super) requirements: Vec<usize>,
    pub(super) load_after: Vec<usize>,
}

/// A bit matrix of `n * n` entries.
#[derive(Debug)]
struct Bits {
    words: usize,
    bits: Vec<u64>,
}

impl Bits {
    fn new(n: usize) -> Self {
        let words = n.div_ceil(64).max(1);
        Self {
            words,
            bits: vec![0; n * words],
        }
    }
    fn get(&self, from: usize, to: usize) -> bool {
        (self.bits[from * self.words + to / 64] >> (to % 64)) & 1 == 1
    }
    fn set(&mut self, from: usize, to: usize) {
        self.bits[from * self.words + to / 64] |= 1 << (to % 64);
    }
}

pub(super) struct PluginGraph<'a> {
    vertices: Vec<Vertex>,
    outgoing: Adjacency,
    incoming: Vec<Vec<usize>>,
    /// Known paths: `(from, to)` is set when a path from `from` to `to` is known to exist.
    paths: Bits,
    /// Direct edges.
    direct: Bits,
    scratch: Scratch,
    overlaps: &'a dyn Overlaps,
    exceeded: bool,
    added: Vec<(usize, usize, EdgeType)>,
    skipped: Vec<(usize, usize, EdgeType)>,
}

impl<'a> PluginGraph<'a> {
    pub(super) fn new(vertices: Vec<Vertex>, overlaps: &'a dyn Overlaps) -> Self {
        let n = vertices.len();
        Self {
            vertices,
            outgoing: vec![Vec::new(); n],
            incoming: vec![Vec::new(); n],
            paths: Bits::new(n),
            direct: Bits::new(n),
            scratch: Scratch::new(n),
            overlaps,
            exceeded: false,
            added: Vec::new(),
            skipped: Vec::new(),
        }
    }

    fn len(&self) -> usize {
        self.vertices.len()
    }

    /// True once the work limit has been passed; every search after that gives up at once.
    fn out_of_budget(&mut self) -> bool {
        if !self.exceeded && self.scratch.work.saturating_add(self.overlaps.work()) > WORK_BUDGET {
            self.exceeded = true;
        }
        self.exceeded
    }

    fn check_budget(&mut self) -> Result<(), GraphError> {
        if self.out_of_budget() {
            Err(GraphError::TooMuchWork)
        } else {
            Ok(())
        }
    }

    fn add_edge(&mut self, from: usize, to: usize, kind: EdgeType) {
        if self.paths.get(from, to) {
            return;
        }
        self.outgoing[from].push((to, kind));
        self.incoming[to].push(from);
        self.direct.set(from, to);
        self.added.push((from, to, kind));
        self.paths.set(from, to);
    }

    fn path_exists(&mut self, from: usize, to: usize) -> bool {
        if self.paths.get(from, to) {
            return true;
        }
        if self.out_of_budget() {
            // Pretend there is one: nothing more is added, and the caller reports the limit.
            return true;
        }
        let paths = &mut self.paths;
        bidirectional_bfs(
            &self.outgoing,
            &self.incoming,
            &mut self.scratch,
            from,
            to,
            &mut |event| match event {
                Event::Forward(_, target) => paths.set(from, target),
                Event::Reverse(source, _) => paths.set(source, to),
                Event::Intersection(_) => {}
            },
        )
    }

    /// A path from `from` to `to`, vertices in order, found the way libloot finds it.
    fn find_path(&mut self, from: usize, to: usize) -> Result<Option<Vec<usize>>, GraphError> {
        let n = self.len();
        let mut parents: Vec<Option<usize>> = vec![None; n];
        let mut children: Vec<Option<usize>> = vec![None; n];
        let mut meeting = None;
        let paths = &mut self.paths;
        let found = bidirectional_bfs(
            &self.outgoing,
            &self.incoming,
            &mut self.scratch,
            from,
            to,
            &mut |event| match event {
                Event::Forward(source, target) => {
                    paths.set(from, target);
                    parents[target] = Some(source);
                }
                Event::Reverse(source, target) => {
                    paths.set(source, to);
                    children[source] = Some(target);
                }
                Event::Intersection(node) => meeting = Some(node),
            },
        );
        self.check_budget()?;
        let (true, Some(meeting)) = (found, meeting) else {
            return Ok(None);
        };
        let internal = GraphError::Internal("a path was found but cannot be followed");
        let mut path = vec![meeting];
        let mut current = meeting;
        while current != from {
            current = parents[current].ok_or(internal.clone())?;
            path.push(current);
        }
        path.reverse();
        current = meeting;
        while current != to {
            current = children[current].ok_or(internal.clone())?;
            path.push(current);
        }
        Ok(Some(path))
    }

    /// Edges from the masters, requirements and load-after files. (libloot also adds edges between plugins whose
    /// master flags differ here; the partitions make that a no-op.)
    pub(super) fn add_specific_edges(&mut self) {
        for node in 0..self.len() {
            let masters = self.vertices[node].masters.clone();
            let requirements = self.vertices[node].requirements.clone();
            let load_after = self.vertices[node].load_after.clone();
            for other in masters {
                self.add_edge(other, node, EdgeType::Master);
            }
            for other in requirements {
                self.add_edge(other, node, EdgeType::MasterlistRequirement);
            }
            for other in load_after {
                self.add_edge(other, node, EdgeType::MasterlistLoadAfter);
            }
        }
    }

    /// Plugins the game loads at fixed places: in the order given, before everything else of this partition.
    pub(super) fn add_early_loading_edges(&mut self, early_loading: &[String]) {
        let mut early: Vec<(usize, usize)> = Vec::new();
        let mut others: Vec<usize> = Vec::new();
        for node in 0..self.len() {
            let name = &self.vertices[node].name;
            match early_position(early_loading, name) {
                Some(position) => early.push((position, node)),
                None => others.push(node),
            }
        }
        early.sort_by_key(|&(position, _)| position);
        for pair in early.windows(2) {
            self.add_edge(pair[0].1, pair[1].1, EdgeType::Hardcoded);
        }
        if let Some(&(_, last)) = early.last() {
            for other in others {
                self.add_edge(last, other, EdgeType::Hardcoded);
            }
        }
    }

    pub(super) fn check_for_cycles(&self) -> Result<(), GraphError> {
        match find_cycle(&self.outgoing) {
            Some(cycle) => Err(GraphError::Cycle(
                cycle
                    .into_iter()
                    .map(|(node, kind)| (self.vertices[node].input, kind))
                    .collect(),
            )),
            None => Ok(()),
        }
    }

    pub(super) fn add_group_edges(&mut self, groups: &GroupGraph) -> Result<(), GraphError> {
        let mut plugins_in_groups: HashMap<usize, Vec<usize>> = HashMap::new();
        for node in 0..self.len() {
            if let Some(group) = groups.node(&self.vertices[node].group) {
                plugins_in_groups.entry(group).or_default().push(node);
            }
        }
        let default = groups.default_node();
        let mut finished = vec![false; groups.names.len()];
        for group in sorted_nodes(groups) {
            let mut visitor = GroupsPathVisitor {
                graph: self,
                plugins_in_groups: &plugins_in_groups,
                finished: &mut finished,
                ignore_as_source: Some(default),
                stack: Vec::new(),
                unfinishable: vec![false; groups.names.len()],
            };
            let mut colours = vec![Colour::White; groups.names.len()];
            depth_first_search(&groups.adjacency, &mut colours, group, &mut visitor);
            self.check_budget()?;
        }
        let mut visitor = GroupsPathVisitor {
            graph: self,
            plugins_in_groups: &plugins_in_groups,
            finished: &mut finished,
            ignore_as_source: None,
            stack: Vec::new(),
            unfinishable: vec![false; groups.names.len()],
        };
        let mut colours = vec![Colour::White; groups.names.len()];
        depth_first_search(&groups.adjacency, &mut colours, default, &mut visitor);
        self.check_budget()
    }

    pub(super) fn add_overlap_edges(&mut self) -> Result<(), GraphError> {
        let n = self.len();
        for node in 0..n {
            let records = self.vertices[node].override_records;
            let assets = self.vertices[node].assets;
            if records == 0 && assets == 0 {
                continue;
            }
            self.check_budget()?;
            for other in node + 1..n {
                if self.direct.get(node, other) || self.direct.get(other, node) {
                    continue;
                }
                let other_records = self.vertices[other].override_records;
                let (input, other_input) = (self.vertices[node].input, self.vertices[other].input);
                let (node_first, kind) =
                    if records == other_records || !self.overlaps.records(input, other_input) {
                        let other_assets = self.vertices[other].assets;
                        if assets == other_assets || !self.overlaps.assets(input, other_input) {
                            continue;
                        }
                        (assets > other_assets, EdgeType::AssetOverlap)
                    } else {
                        (records > other_records, EdgeType::RecordOverlap)
                    };
                let (from, to) = if node_first {
                    (node, other)
                } else {
                    (other, node)
                };
                if !self.paths.get(from, to) {
                    if self.path_exists(to, from) {
                        self.skipped.push((from, to, kind));
                    } else {
                        self.add_edge(from, to, kind);
                    }
                }
            }
        }
        self.check_budget()
    }

    pub(super) fn add_tie_break_edges(&mut self) -> Result<(), GraphError> {
        let mut new_order: Vec<usize> = Vec::new();
        let mut processed = vec![false; self.len()];
        let mut nodes: Vec<usize> = (0..self.len()).collect();
        nodes.sort_by_key(|&node| self.vertices[node].input);
        for pair in nodes.windows(2) {
            let (current, next) = (pair[0], pair[1]);
            match self.find_path(next, current)? {
                None => {
                    self.add_edge(current, next, EdgeType::TieBreak);
                    if !processed[current] {
                        new_order.push(current);
                        processed[current] = true;
                    } else if new_order.last() != Some(&current) {
                        self.pin_node_position(&mut processed, &mut new_order, next, 0);
                    }
                }
                Some(mut path) => {
                    if nodes.first() == Some(&current) {
                        for node in path {
                            new_order.push(node);
                            processed[node] = true;
                        }
                        continue;
                    }
                    path.pop();
                    let mut range_start = 0;
                    for node in path {
                        range_start = self.pin_node_position(
                            &mut processed,
                            &mut new_order,
                            node,
                            range_start,
                        );
                    }
                    if !processed[current] {
                        new_order.push(current);
                        processed[current] = true;
                    }
                }
            }
            self.check_budget()?;
        }
        Ok(())
    }

    fn pin_node_position(
        &mut self,
        processed: &mut [bool],
        new_order: &mut Vec<usize>,
        node: usize,
        range_start: usize,
    ) -> usize {
        if processed[node] {
            return range_start;
        }
        let mut previous = None;
        for position in (range_start..new_order.len()).rev() {
            if !self.path_exists(node, new_order[position]) {
                previous = Some(position);
                break;
            }
        }
        if let Some(position) = previous {
            self.add_edge(new_order[position], node, EdgeType::TieBreak);
        }
        let insert_position = previous.map_or(range_start, |position| position + 1);
        if let Some(&following) = new_order.get(insert_position) {
            self.add_edge(node, following, EdgeType::TieBreak);
        }
        new_order.insert(insert_position, node);
        processed[node] = true;
        insert_position + 1
    }

    /// The vertices in topological order, and whether consecutive vertices are all joined by an edge (when they
    /// are, the order is the only one there is).
    pub(super) fn topological_order(&self) -> Result<(Vec<usize>, bool), GraphError> {
        let order = topological_sort(&self.outgoing).ok_or_else(|| self.cycle_error())?;
        let unique = order
            .windows(2)
            .all(|pair| self.direct.get(pair[0], pair[1]));
        Ok((order, unique))
    }

    fn cycle_error(&self) -> GraphError {
        self.check_for_cycles()
            .err()
            .unwrap_or(GraphError::Internal("the sort found a cycle"))
    }

    pub(super) fn input_of(&self, node: usize) -> usize {
        self.vertices[node].input
    }

    pub(super) fn edges(&self) -> Vec<SortedEdge> {
        self.added
            .iter()
            .map(|&(from, to, kind)| SortedEdge {
                from: self.vertices[from].input,
                to: self.vertices[to].input,
                kind,
            })
            .collect()
    }

    pub(super) fn skipped(&self) -> Vec<SortedEdge> {
        self.skipped
            .iter()
            .map(|&(from, to, kind)| SortedEdge {
                from: self.vertices[from].input,
                to: self.vertices[to].input,
                kind,
            })
            .collect()
    }
}

fn early_position(early_loading: &[String], name: &str) -> Option<usize> {
    early_loading
        .iter()
        .position(|early| early.to_lowercase() == name.to_lowercase())
}

struct GroupsPathVisitor<'g, 'a> {
    graph: &'g mut PluginGraph<'a>,
    plugins_in_groups: &'g HashMap<usize, Vec<usize>>,
    finished: &'g mut [bool],
    ignore_as_source: Option<usize>,
    /// The edges of the path being walked, each with the plugins of its source group.
    stack: Vec<(Edge, &'g [usize])>,
    unfinishable: Vec<bool>,
}

impl<'g> GroupsPathVisitor<'g, '_> {
    fn plugins_of(&self, group: usize) -> &'g [usize] {
        self.plugins_in_groups
            .get(&group)
            .map_or(&[][..], Vec::as_slice)
    }

    fn ignore_source(&self, group: usize) -> bool {
        self.ignore_as_source == Some(group) || self.finished[group]
    }
}

impl DfsVisitor for GroupsPathVisitor<'_, '_> {
    fn tree_edge(&mut self, edge: Edge) {
        let source_plugins = if self.ignore_source(edge.source) {
            &[][..]
        } else {
            self.plugins_of(edge.source)
        };
        self.stack.push((edge, source_plugins));
        let targets = self.plugins_of(edge.target);
        if targets.is_empty() {
            return;
        }
        for &(_, from_plugins) in &self.stack {
            for &from in from_plugins {
                for &to in targets {
                    if self.graph.out_of_budget() {
                        return;
                    }
                    if !self.graph.paths.get(from, to) {
                        if self.graph.path_exists(to, from) {
                            self.graph
                                .skipped
                                .push((from, to, EdgeType::MasterlistGroup));
                        } else {
                            self.graph.add_edge(from, to, EdgeType::MasterlistGroup);
                        }
                    }
                }
            }
        }
    }

    fn forward_or_cross_edge(&mut self, edge: Edge) {
        for group in self
            .stack
            .iter()
            .map(|(stacked, _)| stacked.source)
            .chain(std::iter::once(edge.source))
        {
            self.unfinishable[group] = true;
        }
    }

    fn back_edge(&mut self, _: Edge) {}

    fn discover(&mut self, _: usize) {}

    fn finish(&mut self, node: usize) {
        if self.ignore_as_source != Some(node) && !self.unfinishable[node] {
            self.finished[node] = true;
        }
        self.stack.pop();
    }
}

#[cfg(test)]
#[path = "plugins_tests.rs"]
mod tests;
