//! Why each plugin moved in a full sort, from the edges the sorter added (`sort::Sorted::edges`).
//!
//! The plugins are numbered by their place in the given order. A plugin `p` moved when its place in the result is
//! not its place in the given order. Its reasons:
//!
//! * every edge into `p` from a plugin that was after it in the given order (the edge forces `p` later), and every
//!   edge out of `p` to a plugin that was before it (the edge forces `p` earlier);
//! * when there is no such edge: a plugin of the other partition that crossed `p` (masters load before the others);
//! * otherwise a plugin `q` that crossed `p` and a path of edges between them: some edge `u -> v` on that path has `u`
//!   after `v` in the given order (the path goes from a later plugin to an earlier one), and that edge is the reason,
//!   marked indirect.
//!
//! Every moved plugin gets at least one reason: if `p`'s place changed, some plugin crossed it, and the result is the
//! only order the edges allow, so the crossing is either the partition or a path of edges.

use std::collections::{HashSet, VecDeque};

use crate::sort::{EdgeType, SortedEdge};

/// What forced a move.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Cause {
    /// An edge of the sorter's graph, of this kind.
    Edge(EdgeType),
    /// The other plugin is in the other partition (masters first).
    MasterFlag,
}

/// One reason a plugin moved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Reason {
    /// What forced it.
    pub cause: Cause,
    /// The other plugin (by its place in the given order).
    pub other: usize,
    /// True when the reason puts the plugin after `other` (it moved later); false when before (it moved earlier).
    pub later: bool,
    /// For an indirect reason, the edge `(from, to)` on the path from or to `other` that goes against the given
    /// order; `None` when the reason is an edge of the plugin itself or the partition.
    pub via: Option<(usize, usize)>,
}

/// A plugin that moved, by its place in the given order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Explained {
    /// The plugin, which is also its place in the given order.
    pub plugin: usize,
    /// Its place in the result.
    pub to: usize,
    /// Why, at least one.
    pub reasons: Vec<Reason>,
}

/// The moves of `order` (the result, plugins by their given place) explained by `edges`.
#[must_use]
pub fn explain(order: &[usize], edges: &[SortedEdge], is_master: &[bool]) -> Vec<Explained> {
    let n = order.len();
    let mut position = vec![0; n];
    for (at, &plugin) in order.iter().enumerate() {
        position[plugin] = at;
    }
    let mut outgoing: Vec<Vec<(usize, EdgeType)>> = vec![Vec::new(); n];
    let mut incoming: Vec<Vec<(usize, EdgeType)>> = vec![Vec::new(); n];
    for edge in edges.iter().filter(|edge| edge.from < n && edge.to < n) {
        outgoing[edge.from].push((edge.to, edge.kind));
        incoming[edge.to].push((edge.from, edge.kind));
    }
    let mut moved = Vec::new();
    for plugin in (0..n).filter(|&plugin| position[plugin] != plugin) {
        let mut reasons: Vec<Reason> = Vec::new();
        for &(from, kind) in incoming[plugin].iter().filter(|&&(from, _)| from > plugin) {
            reasons.push(Reason {
                cause: Cause::Edge(kind),
                other: from,
                later: true,
                via: None,
            });
        }
        for &(to, kind) in outgoing[plugin].iter().filter(|&&(to, _)| to < plugin) {
            reasons.push(Reason {
                cause: Cause::Edge(kind),
                other: to,
                later: false,
                via: None,
            });
        }
        if reasons.is_empty() {
            let partner = (0..n).find(|&other| {
                is_master.get(other) != is_master.get(plugin) && crossed(plugin, other, &position)
            });
            if let Some(other) = partner {
                reasons.push(Reason {
                    cause: Cause::MasterFlag,
                    other,
                    later: position[other] < position[plugin],
                    via: None,
                });
            }
        }
        if reasons.is_empty() {
            reasons.extend(path_reason(plugin, &position, &outgoing, &incoming));
        }
        let mut seen = HashSet::with_capacity(reasons.len());
        reasons.retain(|reason| seen.insert(*reason));
        moved.push(Explained {
            plugin,
            to: position[plugin],
            reasons,
        });
    }
    moved
}

/// Did `a` and `b` swap their relative order?
fn crossed(a: usize, b: usize, position: &[usize]) -> bool {
    (a < b) != (position[a] < position[b])
}

/// A plugin that crossed `plugin` and the first edge against the given order on a path between them.
fn path_reason(
    plugin: usize,
    position: &[usize],
    outgoing: &[Vec<(usize, EdgeType)>],
    incoming: &[Vec<(usize, EdgeType)>],
) -> Option<Reason> {
    // Forced later: a plugin given after it that now loads before it, reached backwards over incoming edges. The
    // real edges run nodes[i + 1] -> nodes[i].
    if let Some((nodes, kinds)) = search(plugin, incoming, |other| {
        other > plugin && position[other] < position[plugin]
    }) {
        let other = *nodes.last()?;
        let at = (0..kinds.len()).find(|&i| nodes[i + 1] > nodes[i])?;
        return Some(Reason {
            cause: Cause::Edge(kinds[at]),
            other,
            later: true,
            via: Some((nodes[at + 1], nodes[at])),
        });
    }
    // Forced earlier: the real edges run nodes[i] -> nodes[i + 1].
    let (nodes, kinds) = search(plugin, outgoing, |other| {
        other < plugin && position[other] > position[plugin]
    })?;
    let other = *nodes.last()?;
    let at = (0..kinds.len()).find(|&i| nodes[i] > nodes[i + 1])?;
    Some(Reason {
        cause: Cause::Edge(kinds[at]),
        other,
        later: false,
        via: Some((nodes[at], nodes[at + 1])),
    })
}

/// Breadth-first search from `start` over `next` to the nearest plugin `wanted` accepts: the plugins of the path
/// from `start` to it, and the kind of each step (`kinds[i]` is the edge between `nodes[i]` and `nodes[i + 1]`).
fn search(
    start: usize,
    next: &[Vec<(usize, EdgeType)>],
    wanted: impl Fn(usize) -> bool,
) -> Option<(Vec<usize>, Vec<EdgeType>)> {
    let mut came_from: Vec<Option<(usize, EdgeType)>> = vec![None; next.len()];
    let mut seen = vec![false; next.len()];
    seen[start] = true;
    let mut queue = VecDeque::from([start]);
    while let Some(current) = queue.pop_front() {
        for &(neighbour, kind) in &next[current] {
            if seen[neighbour] {
                continue;
            }
            seen[neighbour] = true;
            came_from[neighbour] = Some((current, kind));
            if wanted(neighbour) {
                let (mut nodes, mut kinds) = (vec![neighbour], Vec::new());
                let mut at = neighbour;
                while at != start {
                    let (previous, kind) = came_from[at]?;
                    kinds.push(kind);
                    nodes.push(previous);
                    at = previous;
                }
                nodes.reverse();
                kinds.reverse();
                return Some((nodes, kinds));
            }
            queue.push_back(neighbour);
        }
    }
    None
}

#[cfg(test)]
#[path = "explain_tests.rs"]
mod tests;
