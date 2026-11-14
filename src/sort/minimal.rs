// SPDX-License-Identifier: GPL-3.0-or-later

//! Minimal-change repair of a load order: keep the given order wherever no hard constraint is violated and move
//! as few plugins as possible to resolve the violations that are left.
//!
//! The input is the given order (the plugins are numbered by their position in it), which of them have the master
//! flag, and the *direct* hard constraints `(before, after, class)`. The required relation is the transitive
//! closure of those constraints plus "every plugin with the master flag loads before every plugin without it".
//! It is a strict partial order (the caller guarantees the constraints are acyclic; a cycle is
//! [`GraphError::Internal`]).
//!
//! # Algorithm
//!
//! 1. A *violation* is a pair `(u, v)` with `u` required before `v` that the given order has the other way round.
//!    Violations are the edges of the *conflict graph*.
//! 2. The plugins that stay (set `K`) must contain no violation among themselves, so `K` is an independent set of
//!    the conflict graph and the number of moves is `n - |K|`. The sort looks for a *largest* independent set,
//!    per connected component of the conflict graph: plugins without a violation are kept, a plugin with one
//!    violation left is kept and its partner moved (an optimal reduction), and the rest is a branch and bound
//!    over the plugin with most violations (move it, or keep it and move its partners), with a work cap per
//!    component. Finding a largest independent set is NP-hard in general, so when the cap is reached the best set
//!    found so far is used (the first leaf of the search is the greedy "move the plugin with most violations
//!    first" answer) and [`Repaired::exact`] is false; it is true otherwise.
//! 3. The kept plugins stay in their given relative order. Each moved plugin, in the given order, is inserted
//!    into the sequence at the place nearest to its given place that respects the required relation towards every
//!    plugin already in the sequence.
//!
//! # Why the result satisfies every constraint
//!
//! The sequence of kept plugins has no violation, so it respects the (transitive) required relation. When a moved
//! plugin `m` is inserted, every plugin `p` already in the sequence that must load before `m` and every plugin `s`
//! that must load after `m` satisfy `p` before `s`, because the relation is transitive (`p < m < s` gives
//! `p < s`) and the sequence respects it by induction. So all `p` lie before all `s`, the interval between the
//! last `p` and the first `s` is not empty, and inserting there keeps the invariant. When the loop ends every
//! plugin is in the sequence and the invariant is the claim.
//!
//! A given order without violations has an empty conflict graph, so `K` is everything and nothing moves.

use std::collections::VecDeque;

use super::{EdgeType, GraphError, SortedEdge};

/// Most search nodes per connected component of the conflict graph before the best answer so far is used.
pub const COMPONENT_BUDGET: u64 = 50_000;

/// One plugin that had to move, and the first constraint that forced it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Move {
    /// The plugin (its position in the given order).
    pub plugin: usize,
    /// Where it was in the given order.
    pub from: usize,
    /// Where it is in the result.
    pub to: usize,
    /// The class of the violated constraint that forced the move ([`EdgeType::MasterFlag`] for "masters first").
    pub class: EdgeType,
    /// The other plugin of that constraint (a plugin that stayed when there is one).
    pub other: usize,
    /// True when the constraint is not stated directly but follows from a chain of constraints; `class` is then
    /// the class of the first link of the chain.
    pub indirect: bool,
}

/// The result of [`repair`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repaired {
    /// Positions in the given order, in the new order.
    pub order: Vec<usize>,
    /// The plugins that moved, in the order they were given.
    pub moves: Vec<Move>,
    /// Number of constraint pairs the given order violated.
    pub violations: usize,
    /// True when the number of moves is proven the smallest possible.
    pub exact: bool,
}

/// Bit matrix of `n` rows.
struct Reach {
    words: usize,
    rows: Vec<u64>,
}

impl Reach {
    fn new(n: usize) -> Self {
        let words = n.div_ceil(64);
        Self {
            words,
            rows: vec![0; n * words],
        }
    }
    fn get(&self, from: usize, to: usize) -> bool {
        self.rows[from * self.words + to / 64] >> (to % 64) & 1 == 1
    }
    fn set(&mut self, from: usize, to: usize) {
        self.rows[from * self.words + to / 64] |= 1 << (to % 64);
    }
    fn merge(&mut self, into: usize, from: usize) {
        for word in 0..self.words {
            let value = self.rows[from * self.words + word];
            self.rows[into * self.words + word] |= value;
        }
    }
}

/// The transitive closure of `successors`, or `None` when they contain a cycle.
fn closure(successors: &[Vec<(usize, EdgeType)>]) -> Option<Reach> {
    let n = successors.len();
    let mut pending: Vec<usize> = vec![0; n];
    for list in successors {
        for &(to, _) in list {
            pending[to] += 1;
        }
    }
    let mut queue: VecDeque<usize> = (0..n).filter(|&node| pending[node] == 0).collect();
    let mut topological = Vec::with_capacity(n);
    while let Some(node) = queue.pop_front() {
        topological.push(node);
        for &(to, _) in &successors[node] {
            pending[to] -= 1;
            if pending[to] == 0 {
                queue.push_back(to);
            }
        }
    }
    if topological.len() != n {
        return None;
    }
    let mut reach = Reach::new(n);
    for &node in topological.iter().rev() {
        for &(to, _) in &successors[node] {
            reach.set(node, to);
            reach.merge(node, to);
        }
    }
    Some(reach)
}

/// Largest independent set of the conflict graph of one component (`members`, with adjacency `adjacent`): the
/// vertices to keep, and whether the answer is proven largest.
struct Search<'a> {
    adjacent: &'a [Vec<usize>],
    budget: u64,
    best: Vec<bool>,
    best_size: usize,
    exhausted: bool,
}

impl Search<'_> {
    fn degree(&self, alive: &[bool], vertex: usize) -> usize {
        self.adjacent[vertex].iter().filter(|&&w| alive[w]).count()
    }

    fn run(&mut self, mut alive: Vec<bool>, mut kept: Vec<bool>) {
        if self.budget == 0 {
            self.exhausted = true;
            return;
        }
        self.budget -= 1;
        // Reductions: a vertex with no live neighbour is kept; one with a single neighbour is kept and the
        // neighbour dropped (never worse).
        loop {
            let mut changed = false;
            for vertex in 0..alive.len() {
                if !alive[vertex] {
                    continue;
                }
                let live: Vec<usize> = self.adjacent[vertex]
                    .iter()
                    .copied()
                    .filter(|&w| alive[w])
                    .collect();
                if live.len() <= 1 {
                    alive[vertex] = false;
                    kept[vertex] = true;
                    for w in live {
                        alive[w] = false;
                    }
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
        let kept_count = kept.iter().filter(|&&k| k).count();
        let remaining = alive.iter().filter(|&&a| a).count();
        if remaining == 0 {
            if kept_count > self.best_size {
                self.best_size = kept_count;
                self.best = kept;
            }
            return;
        }
        if kept_count + remaining <= self.best_size {
            return;
        }
        // The vertex with most live neighbours (the highest position among equals).
        let mut pick = usize::MAX;
        let mut pick_degree = 0;
        for vertex in 0..alive.len() {
            if alive[vertex] {
                let degree = self.degree(&alive, vertex);
                if pick == usize::MAX || degree >= pick_degree {
                    pick = vertex;
                    pick_degree = degree;
                }
            }
        }
        // Move it first (the greedy answer), then try keeping it.
        let mut without = alive.clone();
        without[pick] = false;
        self.run(without, kept.clone());
        if self.budget == 0 {
            self.exhausted = true;
            return;
        }
        alive[pick] = false;
        kept[pick] = true;
        for &w in &self.adjacent[pick] {
            alive[w] = false;
        }
        self.run(alive, kept);
    }
}

/// Which plugins to keep: `keep[v]` for every vertex, and whether the answer is proven largest.
fn choose_kept(conflicts: &[Vec<usize>]) -> (Vec<bool>, bool) {
    let n = conflicts.len();
    let mut keep = vec![true; n];
    let mut exact = true;
    let mut seen = vec![false; n];
    for start in 0..n {
        if seen[start] || conflicts[start].is_empty() {
            continue;
        }
        let mut component = vec![start];
        seen[start] = true;
        let mut at = 0;
        while at < component.len() {
            for &w in &conflicts[component[at]] {
                if !seen[w] {
                    seen[w] = true;
                    component.push(w);
                }
            }
            at += 1;
        }
        // Renumber the component so that the search arrays are small.
        component.sort_unstable();
        let local = |vertex: usize| component.binary_search(&vertex).expect("member");
        let adjacent: Vec<Vec<usize>> = component
            .iter()
            .map(|&vertex| conflicts[vertex].iter().map(|&w| local(w)).collect())
            .collect();
        let mut search = Search {
            adjacent: &adjacent,
            budget: COMPONENT_BUDGET,
            best: vec![false; component.len()],
            best_size: 0,
            exhausted: false,
        };
        search.run(vec![true; component.len()], vec![false; component.len()]);
        exact &= !search.exhausted;
        let mut chosen = search.best;
        // Keep more where the search stopped early: any moved plugin without a kept partner stays.
        for vertex in 0..chosen.len() {
            if !chosen[vertex] && adjacent[vertex].iter().all(|&w| !chosen[w]) {
                chosen[vertex] = true;
            }
        }
        for (at, &vertex) in component.iter().enumerate() {
            keep[vertex] = chosen[at];
        }
    }
    (keep, exact)
}

/// Repair the given order (positions `0..is_master.len()`).
///
/// `constraints` are the direct hard constraints over positions: `from` must load before `to`.
///
/// # Errors
/// [`GraphError::Internal`] when the constraints (with "masters first") contain a cycle, which the caller rules
/// out.
pub fn repair(is_master: &[bool], constraints: &[SortedEdge]) -> Result<Repaired, GraphError> {
    let n = is_master.len();
    let mut successors: Vec<Vec<(usize, EdgeType)>> = vec![Vec::new(); n];
    for edge in constraints {
        if edge.from >= n || edge.to >= n || edge.from == edge.to {
            return Err(GraphError::Internal("a constraint names no plugin"));
        }
        successors[edge.from].push((edge.to, edge.kind));
    }
    let reach = closure(&successors).ok_or(GraphError::Internal("the constraints form a cycle"))?;
    let required = |a: usize, b: usize| (is_master[a] && !is_master[b]) || reach.get(a, b);
    // A required pair the wrong way round is a violation; a master after a non-master is one too.
    for a in 0..n {
        for b in 0..n {
            if is_master[a] && !is_master[b] && reach.get(b, a) {
                return Err(GraphError::Internal(
                    "a plugin without the master flag must load before one with it",
                ));
            }
        }
    }
    let mut conflicts: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut violations = 0;
    for earlier in 0..n {
        for later in earlier + 1..n {
            // `earlier` is first in the given order; a violation is `later` required before `earlier`.
            if required(later, earlier) {
                conflicts[earlier].push(later);
                conflicts[later].push(earlier);
                violations += 1;
            }
        }
    }
    let (keep, exact) = choose_kept(&conflicts);

    let mut order: Vec<usize> = (0..n).filter(|&p| keep[p]).collect();
    for plugin in (0..n).filter(|&p| !keep[p]) {
        let mut low = 0;
        let mut high = order.len();
        for (at, &other) in order.iter().enumerate() {
            if required(other, plugin) {
                low = low.max(at + 1);
            }
            if required(plugin, other) {
                high = high.min(at);
            }
        }
        // Where it was, relative to the plugins in the sequence.
        let wanted = order.iter().filter(|&&other| other < plugin).count();
        order.insert(wanted.clamp(low, high), plugin);
    }

    let mut final_position = vec![0; n];
    for (at, &plugin) in order.iter().enumerate() {
        final_position[plugin] = at;
    }
    let moves = (0..n)
        .filter(|&p| !keep[p])
        .map(|plugin| {
            let (class, other, indirect) = reason(
                plugin,
                &conflicts[plugin],
                &keep,
                &successors,
                is_master,
                &reach,
            );
            Move {
                plugin,
                from: plugin,
                to: final_position[plugin],
                class,
                other,
                indirect,
            }
        })
        .collect();
    Ok(Repaired {
        order,
        moves,
        violations,
        exact,
    })
}

/// The constraint that explains why `plugin` moved: among its violations, a direct one before an indirect one,
/// then the class in the order of [`EdgeType`], then a kept partner before a moved one, then the lowest position.
fn reason(
    plugin: usize,
    partners: &[usize],
    keep: &[bool],
    successors: &[Vec<(usize, EdgeType)>],
    is_master: &[bool],
    reach: &Reach,
) -> (EdgeType, usize, bool) {
    let mut best: Option<((bool, EdgeType, bool, usize), usize)> = None;
    for &other in partners {
        // A violation: the one later in the given order is required before the other.
        let (before, after) = (plugin.max(other), plugin.min(other));
        let direct = successors[before]
            .iter()
            .filter(|&&(to, _)| to == after)
            .map(|&(_, kind)| kind)
            .min();
        let (class, indirect) = match direct {
            Some(kind) => (kind, false),
            None if is_master[before] && !is_master[after] => (EdgeType::MasterFlag, false),
            None => (
                successors[before]
                    .iter()
                    .filter(|&&(to, _)| to == after || reach.get(to, after))
                    .map(|&(_, kind)| kind)
                    .min()
                    .unwrap_or(EdgeType::MasterFlag),
                true,
            ),
        };
        let key = (indirect, class, !keep[other], other);
        if best.is_none_or(|(current, _)| key < current) {
            best = Some((key, other));
        }
    }
    best.map_or(
        (EdgeType::MasterFlag, plugin, false),
        |((indirect, class, _, _), other)| (class, other, indirect),
    )
}

#[cfg(test)]
#[path = "minimal_tests.rs"]
mod tests;
