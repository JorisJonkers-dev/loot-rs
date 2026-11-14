// SPDX-License-Identifier: GPL-3.0-or-later
//
// Ported to Rust from libloot (https://github.com/loot/libloot, `src/sorting/`, GPL-3.0-or-later, Copyright (C)
// the libloot authors), using the documentation at https://loot-api.readthedocs.io/en/latest/api/sorting.html. See
// NOTICE.md for what was ported, what was left out and the licences.

//! LOOT's sorting algorithm (libloot's plugin graph), on plain indices: no file system, no metadata.
//!
//! The stages, as in libloot:
//!
//! 1. The plugins become vertices in the byte order of their names, so that the work does not depend on the
//!    current load order. Plugins with the master flag and the others are sorted in two separate graphs and
//!    the results joined (masters first); an edge between the two kinds is therefore never added, and metadata
//!    that asks for one is an error.
//! 2. Edges from the header masters, the masterlist's `req` and `after` files, and the game's hard-coded early
//!    loaders. A cycle among these is an error that names it.
//! 3. Edges from groups: a plugin loads before the plugins of the groups that load after its group, found with
//!    depth-first searches of the group graph, each edge only when it closes no cycle.
//! 4. Overlap edges: of two plugins that change the same records the one that changes more loads first (then the
//!    one that loads more assets), unless that closes a cycle.
//! 5. Tie-break edges that join the plugins in the current load order wherever that closes no cycle, which leaves
//!    one possible order, and the topological sort that returns it.
//!
//! [`sort_minimal`] is the other mode: it keeps the given order wherever no hard constraint is violated and moves
//! the fewest plugins that fix the violations (see the `minimal` module for the algorithm and its proof).
//!
//! The graph keeps a matrix of the paths it knows of and searches from both ends, as libloot does, because the
//! edges it ends up with (and so the order) depend on which paths are found. The work is counted and bounded
//! ([`MAX_PLUGINS`], [`WORK_BUDGET`]).

mod groups;
mod minimal;
mod plugins;
mod search;

use std::fmt;

pub use minimal::{COMPONENT_BUDGET, Move as MinimalMove, Repaired};
use plugins::{PluginGraph, Vertex};

/// Name of the group every plugin is in unless the masterlist says otherwise.
pub const DEFAULT_GROUP: &str = "default";
/// Most plugins that are sorted at once (the game loads 255).
pub const MAX_PLUGINS: usize = 1024;
/// Most elementary steps of graph searches (and record comparisons) before the sort gives up.
pub const WORK_BUDGET: u64 = 400_000_000;

/// Why an edge is in the graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum EdgeType {
    /// The game loads the plugin at a fixed place.
    Hardcoded,
    /// One has the master flag and the other has not.
    MasterFlag,
    /// The plugin lists the other as a master in its header.
    Master,
    /// The masterlist says the plugin requires the other (`req`).
    MasterlistRequirement,
    /// The masterlist says the plugin loads after the other (`after`).
    MasterlistLoadAfter,
    /// The plugin's group loads after the other's.
    MasterlistGroup,
    /// The plugins change the same records and one changes more.
    RecordOverlap,
    /// The plugins load assets from archives with the same paths and one loads more.
    AssetOverlap,
    /// Added so that exactly one order is left.
    TieBreak,
}

impl EdgeType {
    /// The name LOOT uses.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Hardcoded => "Hardcoded",
            Self::MasterFlag => "Master Flag",
            Self::Master => "Master",
            Self::MasterlistRequirement => "Masterlist Requirement",
            Self::MasterlistLoadAfter => "Masterlist Load After",
            Self::MasterlistGroup => "Masterlist Group",
            Self::RecordOverlap => "Record Overlap",
            Self::AssetOverlap => "Asset Overlap",
            Self::TieBreak => "Tie Break",
        }
    }
}

impl fmt::Display for EdgeType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// Which plugins change the same records or load the same assets. Indices are those of the list given to
/// [`sort`].
pub trait Overlaps {
    /// Do the two plugins have a record in common (one changes a record the other changes or adds)?
    fn records(&self, a: usize, b: usize) -> bool;
    /// Do archives the two plugins load hold a file with the same path?
    fn assets(&self, a: usize, b: usize) -> bool;
    /// Steps spent so far on answering, counted against [`WORK_BUDGET`].
    fn work(&self) -> u64 {
        0
    }
}

/// No overlap data: only the metadata decides.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoOverlaps;

impl Overlaps for NoOverlaps {
    fn records(&self, _: usize, _: usize) -> bool {
        false
    }
    fn assets(&self, _: usize, _: usize) -> bool {
        false
    }
}

/// One plugin to sort. The list given to [`sort`] is the current load order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PluginInput {
    /// File name; names are compared without regard to case where plugins refer to each other.
    pub name: String,
    /// The master flag of the header.
    pub is_master: bool,
    /// The masters in the header, by name.
    pub masters: Vec<String>,
    /// Files the masterlist says the plugin requires (conditions already applied).
    pub requirements: Vec<String>,
    /// Files the masterlist says the plugin loads after (conditions already applied).
    pub load_after: Vec<String>,
    /// Group name; `default` when the masterlist names none.
    pub group: String,
    /// Records of the plugin that are also in one of its masters.
    pub override_records: usize,
    /// Files the plugin's archives hold.
    pub assets: usize,
}

impl PluginInput {
    /// A plugin with a name and nothing else (in the default group).
    #[must_use]
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            group: DEFAULT_GROUP.to_string(),
            ..Self::default()
        }
    }
}

/// A group and the groups it loads after.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GroupInput {
    /// Case-sensitive name.
    pub name: String,
    /// Names of the groups it loads after.
    pub after: Vec<String>,
}

/// Why no order exists, or was not tried.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum GraphError {
    /// More than [`MAX_PLUGINS`] plugins.
    TooMany(usize),
    /// A cycle among the edges: each plugin (index in the list given) with the type of the edge to the next, the
    /// last back to the first.
    Cycle(Vec<(usize, EdgeType)>),
    /// A plugin without the master flag has to load before one with it (`plugin` before `master`; `kind` says
    /// why).
    MasterOrder {
        /// The plugin without the master flag.
        plugin: usize,
        /// The plugin with it.
        master: usize,
        /// The kind of rule that asked.
        kind: EdgeType,
    },
    /// Groups that each load after the other, by their names.
    GroupCycle(Vec<String>),
    /// The work budget ran out.
    TooMuchWork,
    /// A check inside the sorter failed; a bug, never input.
    Internal(&'static str),
}

impl fmt::Display for GraphError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooMany(count) => {
                write!(f, "{count} plugins, more than the limit of {MAX_PLUGINS}")
            }
            Self::Cycle(cycle) => {
                write!(f, "a cycle of {} plugins", cycle.len())?;
                for (plugin, kind) in cycle {
                    write!(f, ", {plugin} ({kind})")?;
                }
                Ok(())
            }
            Self::MasterOrder {
                plugin,
                master,
                kind,
            } => write!(
                f,
                "plugin {plugin} (not a master) must load before {master} (a master): {kind}"
            ),
            Self::GroupCycle(groups) => write!(f, "groups that load after each other: {groups:?}"),
            Self::TooMuchWork => write!(f, "the load order needs more work than the limit allows"),
            Self::Internal(text) => write!(f, "internal error: {text}"),
        }
    }
}

impl std::error::Error for GraphError {}

/// An edge of the final graph, over indices of the list given to [`sort`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SortedEdge {
    /// Loads first.
    pub from: usize,
    /// Loads later.
    pub to: usize,
    /// Why.
    pub kind: EdgeType,
}

/// The result of [`sort`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sorted {
    /// Indices of the list given, in the new load order.
    pub order: Vec<usize>,
    /// The edges that were added, in the order libloot adds them (the masters' graph first, then the other
    /// plugins').
    pub edges: Vec<SortedEdge>,
    /// Edges that would have closed a cycle and were left out (group and overlap edges only).
    pub skipped: Vec<SortedEdge>,
    /// False when two plugins next to each other in the order have no edge between them, that is when the order is
    /// not the only one the edges allow. libloot logs that as an error; it is a bug in the sorter if it happens.
    pub unique: bool,
}

fn lower(name: &str) -> String {
    name.to_lowercase()
}

/// Sort `plugins` (given in their current load order) with libloot's algorithm.
///
/// `early_loading` are the plugins the game loads before all others, in order (for Fallout: New Vegas,
/// `FalloutNV.esm`). Names that refer to plugins that are not in the list are ignored.
///
/// # Errors
/// [`GraphError`] for a cycle among the metadata, a plugin without the master flag that has to load before one
/// with it, groups that load after each other, more than [`MAX_PLUGINS`] plugins, or too much work.
pub fn sort(
    plugins: &[PluginInput],
    groups: &[GroupInput],
    early_loading: &[String],
    overlaps: &dyn Overlaps,
) -> Result<Sorted, GraphError> {
    if plugins.len() > MAX_PLUGINS {
        return Err(GraphError::TooMany(plugins.len()));
    }
    let group_graph = groups::build(groups)?;
    if plugins.is_empty() {
        return Ok(Sorted {
            order: Vec::new(),
            edges: Vec::new(),
            skipped: Vec::new(),
            unique: true,
        });
    }

    // Vertices in the byte order of the names.
    let mut by_name: Vec<usize> = (0..plugins.len()).collect();
    by_name.sort_by(|&a, &b| plugins[a].name.cmp(&plugins[b].name));
    let (masters, others): (Vec<usize>, Vec<usize>) = by_name
        .into_iter()
        .partition(|&index| plugins[index].is_master);

    validate(plugins, &masters, &others, early_loading)?;

    let mut order = Vec::with_capacity(plugins.len());
    let mut edges = Vec::new();
    let mut skipped = Vec::new();
    let mut unique = true;
    for partition in [&masters, &others] {
        let part = sort_partition(plugins, partition, &group_graph, early_loading, overlaps)?;
        order.extend(part.order);
        edges.extend(part.edges);
        skipped.extend(part.skipped);
        unique &= part.unique;
    }
    Ok(Sorted {
        order,
        edges,
        skipped,
        unique,
    })
}

/// Keep the given order and move as few plugins as possible so that every hard constraint holds.
///
/// The hard constraints are: plugins with the master flag before the others, the header masters, the masterlist's
/// `req` and `after` files, and the early loaders first. With `with_groups` the group edges that libloot's sort
/// adds (the group rules it can satisfy without a cycle) are hard too. The full sort is run first, so the same
/// errors are returned, and so that the constraints are known to be consistent.
///
/// The result satisfies every one of those constraints, which libloot's order satisfies too, and a given order that
/// violates none is returned unchanged. `moves` says which constraint class forced each move.
///
/// # Errors
/// The errors of [`sort`].
pub fn sort_minimal(
    plugins: &[PluginInput],
    groups: &[GroupInput],
    early_loading: &[String],
    overlaps: &dyn Overlaps,
    with_groups: bool,
) -> Result<Repaired, GraphError> {
    let full = sort(plugins, groups, early_loading, overlaps)?;
    let mut constraints = direct_constraints(plugins, early_loading);
    if with_groups {
        constraints.extend(
            full.edges
                .iter()
                .filter(|edge| edge.kind == EdgeType::MasterlistGroup)
                .copied(),
        );
    }
    let is_master: Vec<bool> = plugins.iter().map(|plugin| plugin.is_master).collect();
    minimal::repair(&is_master, &constraints)
}

/// The constraints the metadata states directly: header masters, `req`, `after` and the early loaders (each before
/// everything else of its kind).
fn direct_constraints(plugins: &[PluginInput], early_loading: &[String]) -> Vec<SortedEdge> {
    let mut by_name: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for (index, plugin) in plugins.iter().enumerate() {
        by_name.entry(lower(&plugin.name)).or_insert(index);
    }
    let mut edges = Vec::new();
    for (index, plugin) in plugins.iter().enumerate() {
        let lists = [
            (&plugin.masters, EdgeType::Master),
            (&plugin.requirements, EdgeType::MasterlistRequirement),
            (&plugin.load_after, EdgeType::MasterlistLoadAfter),
        ];
        for (names, kind) in lists {
            for name in names {
                let other = by_name.get(&lower(name)).copied().filter(|&o| o != index);
                if let Some(other) = other {
                    edges.push(SortedEdge {
                        from: other,
                        to: index,
                        kind,
                    });
                }
            }
        }
    }
    for is_master in [true, false] {
        let mut early: Vec<(usize, usize)> = Vec::new();
        let mut others = Vec::new();
        for (index, plugin) in plugins.iter().enumerate() {
            if plugin.is_master != is_master {
                continue;
            }
            let position = early_loading
                .iter()
                .position(|name| lower(name) == lower(&plugin.name));
            match position {
                Some(position) => early.push((position, index)),
                None => others.push(index),
            }
        }
        early.sort_unstable();
        for pair in early.windows(2) {
            edges.push(SortedEdge {
                from: pair[0].1,
                to: pair[1].1,
                kind: EdgeType::Hardcoded,
            });
        }
        if let Some(&(_, last)) = early.last() {
            for other in others {
                edges.push(SortedEdge {
                    from: last,
                    to: other,
                    kind: EdgeType::Hardcoded,
                });
            }
        }
    }
    edges
}

/// The metadata that asks for a plugin without the master flag to load before one with it.
fn validate(
    plugins: &[PluginInput],
    masters: &[usize],
    others: &[usize],
    early_loading: &[String],
) -> Result<(), GraphError> {
    let other_names: std::collections::HashMap<String, usize> = others
        .iter()
        .map(|&index| (lower(&plugins[index].name), index))
        .collect();
    for &master in masters {
        let plugin = &plugins[master];
        let lists = [
            (&plugin.masters, EdgeType::Master),
            (&plugin.requirements, EdgeType::MasterlistRequirement),
            (&plugin.load_after, EdgeType::MasterlistLoadAfter),
        ];
        for (names, kind) in lists {
            for name in names {
                if let Some(&other) = other_names.get(&lower(name)) {
                    return Err(GraphError::MasterOrder {
                        plugin: other,
                        master,
                        kind,
                    });
                }
            }
        }
    }
    if let Some(&first_master) = masters.first() {
        for name in early_loading {
            if let Some(&other) = other_names.get(&lower(name)) {
                return Err(GraphError::MasterOrder {
                    plugin: other,
                    master: first_master,
                    kind: EdgeType::Hardcoded,
                });
            }
        }
    }
    Ok(())
}

fn sort_partition(
    plugins: &[PluginInput],
    partition: &[usize],
    groups: &groups::GroupGraph,
    early_loading: &[String],
    overlaps: &dyn Overlaps,
) -> Result<Sorted, GraphError> {
    let local: std::collections::HashMap<String, usize> = partition
        .iter()
        .enumerate()
        .map(|(node, &index)| (lower(&plugins[index].name), node))
        .collect();
    let resolve = |names: &[String]| -> Vec<usize> {
        names
            .iter()
            .filter_map(|name| local.get(&lower(name)).copied())
            .collect()
    };
    let vertices: Vec<Vertex> = partition
        .iter()
        .map(|&index| {
            let plugin = &plugins[index];
            Vertex {
                input: index,
                name: plugin.name.clone(),
                group: plugin.group.clone(),
                override_records: plugin.override_records,
                assets: plugin.assets,
                masters: resolve(&plugin.masters),
                requirements: resolve(&plugin.requirements),
                load_after: resolve(&plugin.load_after),
            }
        })
        .collect();

    let mut graph = PluginGraph::new(vertices, overlaps);
    graph.add_specific_edges();
    graph.add_early_loading_edges(early_loading);
    graph.check_for_cycles()?;
    graph.add_group_edges(groups)?;
    graph.add_overlap_edges()?;
    graph.add_tie_break_edges()?;
    graph.check_for_cycles()?;
    let (nodes, unique) = graph.topological_order()?;
    Ok(Sorted {
        order: nodes.into_iter().map(|node| graph.input_of(node)).collect(),
        edges: graph.edges(),
        skipped: graph.skipped(),
        unique,
    })
}

#[cfg(test)]
#[path = "sort_tests.rs"]
mod tests;
