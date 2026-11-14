// SPDX-License-Identifier: GPL-3.0-or-later
//
// Graph searches used by the sorter. Ported to Rust from libloot (https://github.com/loot/libloot,
// `src/sorting/search.rs`, GPL-3.0-or-later, Copyright (C) the libloot authors), which uses petgraph. The iteration
// orders of petgraph that the results depend on are reproduced here, see NOTICE.md.

//! Searches on adjacency lists: a bidirectional breadth-first search, a depth-first search with a visitor, cycle
//! finding and the topological sort.
//!
//! petgraph lists the neighbours of a vertex in the reverse of the order its edges were added. `libloot`'s results
//! depend on that (it decides which path a search finds), so the breadth-first search and the topological sort walk
//! the adjacency lists backwards and the depth-first search, which `libloot` reverses again, walks them forwards.

use std::collections::VecDeque;

use super::EdgeType;

/// Out-edges of every vertex, in the order they were added.
pub(super) type Adjacency = Vec<Vec<(usize, EdgeType)>>;

/// An edge met by a search.
#[derive(Debug, Clone, Copy)]
pub(super) struct Edge {
    pub(super) source: usize,
    pub(super) target: usize,
    pub(super) kind: EdgeType,
}

/// What a depth-first search reports, as in libloot.
pub(super) trait DfsVisitor {
    fn tree_edge(&mut self, edge: Edge);
    fn forward_or_cross_edge(&mut self, edge: Edge);
    fn back_edge(&mut self, edge: Edge);
    fn discover(&mut self, node: usize);
    fn finish(&mut self, node: usize);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Colour {
    White,
    Grey,
    Black,
}

/// Depth-first search from `start`, edges in the order they were added, `colours` shared between calls when the
/// caller wants one visit per vertex.
pub(super) fn depth_first_search(
    adjacency: &Adjacency,
    colours: &mut [Colour],
    start: usize,
    visitor: &mut impl DfsVisitor,
) {
    let mut stack: Vec<(usize, usize)> = vec![(start, 0)];
    colours[start] = Colour::Grey;
    visitor.discover(start);
    while let Some(top) = stack.last_mut() {
        let current = top.0;
        if let Some(&(target, kind)) = adjacency[current].get(top.1) {
            top.1 += 1;
            let edge = Edge {
                source: current,
                target,
                kind,
            };
            match colours[target] {
                Colour::White => {
                    visitor.tree_edge(edge);
                    colours[target] = Colour::Grey;
                    visitor.discover(target);
                    stack.push((target, 0));
                }
                Colour::Grey => visitor.back_edge(edge),
                Colour::Black => visitor.forward_or_cross_edge(edge),
            }
        } else {
            colours[current] = Colour::Black;
            visitor.finish(current);
            stack.pop();
        }
    }
}

/// The path of a cycle: each vertex with the type of the edge to the next, the last one back to the first.
pub(super) type Cycle = Vec<(usize, EdgeType)>;

struct CycleDetector {
    trail: Cycle,
    found: bool,
}

impl DfsVisitor for CycleDetector {
    fn tree_edge(&mut self, edge: Edge) {
        if !self.found {
            self.trail.push((edge.source, edge.kind));
        }
    }
    fn forward_or_cross_edge(&mut self, _: Edge) {}
    fn back_edge(&mut self, edge: Edge) {
        if self.found {
            return;
        }
        self.tree_edge(edge);
        if let Some(position) = self.trail.iter().position(|&(node, _)| node == edge.target) {
            self.trail.drain(..position);
            self.found = true;
        }
    }
    fn discover(&mut self, _: usize) {}
    fn finish(&mut self, _: usize) {
        if !self.found {
            self.trail.pop();
        }
    }
}

/// The first cycle a depth-first search from the vertices in order finds, if any.
pub(super) fn find_cycle(adjacency: &Adjacency) -> Option<Cycle> {
    let mut detector = CycleDetector {
        trail: Vec::new(),
        found: false,
    };
    let mut colours = vec![Colour::White; adjacency.len()];
    for start in 0..adjacency.len() {
        if colours[start] == Colour::White {
            depth_first_search(adjacency, &mut colours, start, &mut detector);
        }
        if detector.found {
            return Some(detector.trail);
        }
    }
    None
}

/// What the bidirectional search reports while it runs.
#[derive(Debug, Clone, Copy)]
pub(super) enum Event {
    /// The forward search reached `.1` from `.0`.
    Forward(usize, usize),
    /// The reverse search reached `.0`, which has an edge to `.1`.
    Reverse(usize, usize),
    /// The two searches met at this vertex: there is a path.
    Intersection(usize),
}

/// Buffers for [`bidirectional_bfs`], reused so that a search costs no allocation and no clearing.
#[derive(Debug, Default)]
pub(super) struct Scratch {
    forward: Vec<u32>,
    reverse: Vec<u32>,
    epoch: u32,
    forward_queue: VecDeque<usize>,
    reverse_queue: VecDeque<usize>,
    /// Vertices and edges looked at so far, over all searches.
    pub(super) work: u64,
}

impl Scratch {
    pub(super) fn new(n: usize) -> Self {
        Self {
            forward: vec![0; n],
            reverse: vec![0; n],
            ..Self::default()
        }
    }
}

/// Is there a path from `from` to `to`? Searches from both ends, one vertex of each per round, exactly as libloot
/// does, and reports what it sees to `visit`. `incoming[v]` lists the sources of the edges into `v`, in the order
/// they were added.
pub(super) fn bidirectional_bfs(
    outgoing: &Adjacency,
    incoming: &[Vec<usize>],
    scratch: &mut Scratch,
    from: usize,
    to: usize,
    visit: &mut impl FnMut(Event),
) -> bool {
    if scratch.epoch == u32::MAX {
        scratch.forward.fill(0);
        scratch.reverse.fill(0);
        scratch.epoch = 0;
    }
    scratch.epoch += 1;
    let epoch = scratch.epoch;
    scratch.forward_queue.clear();
    scratch.reverse_queue.clear();
    scratch.forward_queue.push_back(from);
    scratch.reverse_queue.push_back(to);
    scratch.forward[from] = epoch;
    scratch.reverse[to] = epoch;

    while let (Some(forward_current), Some(reverse_current)) = (
        scratch.forward_queue.pop_front(),
        scratch.reverse_queue.pop_front(),
    ) {
        scratch.work += 1 + outgoing[forward_current].len() as u64;
        if forward_current == to || scratch.reverse[forward_current] == epoch {
            visit(Event::Intersection(forward_current));
            return true;
        }
        for &(adjacent, _) in outgoing[forward_current].iter().rev() {
            if scratch.forward[adjacent] != epoch {
                visit(Event::Forward(forward_current, adjacent));
                scratch.forward[adjacent] = epoch;
                scratch.forward_queue.push_back(adjacent);
            }
        }

        scratch.work += 1 + incoming[reverse_current].len() as u64;
        if reverse_current == from || scratch.forward[reverse_current] == epoch {
            visit(Event::Intersection(reverse_current));
            return true;
        }
        for &adjacent in incoming[reverse_current].iter().rev() {
            if scratch.reverse[adjacent] != epoch {
                visit(Event::Reverse(adjacent, reverse_current));
                scratch.reverse[adjacent] = epoch;
                scratch.reverse_queue.push_back(adjacent);
            }
        }
    }
    false
}

/// petgraph's `toposort`: a depth-first search that pushes every unvisited successor, and the vertices in the
/// reverse of the order they finish. `None` when a vertex is its own successor (the caller has checked for
/// longer cycles).
pub(super) fn topological_sort(adjacency: &Adjacency) -> Option<Vec<usize>> {
    let n = adjacency.len();
    let mut discovered = vec![false; n];
    let mut finished = vec![false; n];
    let mut stack: Vec<usize> = Vec::new();
    let mut order: Vec<usize> = Vec::with_capacity(n);
    for start in 0..n {
        if discovered[start] {
            continue;
        }
        stack.push(start);
        while let Some(&node) = stack.last() {
            if discovered[node] {
                stack.pop();
                if !finished[node] {
                    finished[node] = true;
                    order.push(node);
                }
            } else {
                discovered[node] = true;
                for &(successor, _) in adjacency[node].iter().rev() {
                    if successor == node {
                        return None;
                    }
                    if !discovered[successor] {
                        stack.push(successor);
                    }
                }
            }
        }
    }
    order.reverse();
    Some(order)
}
