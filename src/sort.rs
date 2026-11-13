//! The ordering algorithm, on plain indices: no file system, no metadata.
//!
//! Input: `n` plugins numbered by their position in the load order, which of them are masters (the master
//! flag), *hard* edges `(a, b)` meaning "a must load before b" that have to hold, and *soft* edges that hold when
//! they can. Output: an order of `0..n`.
//!
//! * Masters (flag set) come before every other plugin, as the engine requires. A hard edge that asks for the
//!   opposite is an error, a soft one is dropped.
//! * Hard edges are added first. One that closes a cycle is an error naming it.
//! * Soft edges are added in the order given, each unless it would close a cycle; the ones dropped are
//!   returned. The caller gives them in an order that does not depend on the load order, so which ones are dropped
//!   does not depend on it either.
//! * Among plugins that may come next, the one earliest in the given load order does: a plugin that has to wait
//!   for another is placed right after it, the rest keep their order, and sorting an already sorted order changes
//!   nothing.
//!
//! Reachability is kept as a bit matrix, so "would this edge close a cycle" is one lookup. The work is counted
//! and bounded ([`MAX_PLUGINS`], [`WORK_BUDGET`]).

use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::fmt;

/// Most plugins that are sorted at once (the game loads 255).
pub const MAX_PLUGINS: usize = 1024;
/// Most elementary steps of bit-matrix updates before the sort gives up.
pub const WORK_BUDGET: u64 = 400_000_000;

/// Why no order exists, or was not tried.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum GraphError {
    /// More than [`MAX_PLUGINS`] plugins.
    TooMany(usize),
    /// A hard edge `(from, to)` closes a cycle: `to` already has to load before `from`.
    Cycle(usize, usize),
    /// A hard edge `(from, to)` wants a plugin without the master flag before one with it.
    MasterOrder(usize, usize),
    /// An edge names a plugin that does not exist.
    BadIndex(usize),
    /// The work budget ran out.
    TooMuchWork,
}

impl fmt::Display for GraphError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooMany(count) => {
                write!(f, "{count} plugins, more than the limit of {MAX_PLUGINS}")
            }
            Self::Cycle(from, to) => write!(
                f,
                "plugin {from} must load before {to}, which must load before it"
            ),
            Self::MasterOrder(from, to) => write!(
                f,
                "plugin {from} (not a master) must load before {to} (a master)"
            ),
            Self::BadIndex(index) => write!(f, "no plugin number {index}"),
            Self::TooMuchWork => write!(f, "the load order needs more work than the limit allows"),
        }
    }
}

impl std::error::Error for GraphError {}

/// The result of [`order`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sorted {
    /// The indices in load order.
    pub order: Vec<usize>,
    /// Soft edges that were left out because they would have closed a cycle.
    pub dropped: Vec<(usize, usize)>,
}

struct Reach {
    n: usize,
    words: usize,
    bits: Vec<u64>,
    work: u64,
}

impl Reach {
    fn new(n: usize) -> Self {
        let words = n.div_ceil(64).max(1);
        Self {
            n,
            words,
            bits: vec![0; n * words],
            work: 0,
        }
    }

    fn reaches(&self, from: usize, to: usize) -> bool {
        (self.bits[from * self.words + to / 64] >> (to % 64)) & 1 == 1
    }

    /// Record that `a` loads before `b` and everything that follows from it.
    fn add(&mut self, a: usize, b: usize) -> Result<(), GraphError> {
        let words = self.words;
        let mut tail: Vec<u64> = self.bits[b * words..(b + 1) * words].to_vec();
        tail[b / 64] |= 1 << (b % 64);
        for x in 0..self.n {
            if x == a || self.reaches(x, a) {
                self.work += words as u64;
                if self.work > WORK_BUDGET {
                    return Err(GraphError::TooMuchWork);
                }
                for (word, add) in self.bits[x * words..(x + 1) * words].iter_mut().zip(&tail) {
                    *word |= add;
                }
            }
        }
        Ok(())
    }
}

/// Order `0..n`. See the module documentation.
///
/// # Errors
/// [`GraphError`] when an edge is invalid, a hard edge closes a cycle or asks for a non-master before a master, or
/// the input is too large.
pub fn order(
    n: usize,
    is_master: &[bool],
    hard: &[(usize, usize)],
    soft: &[(usize, usize)],
) -> Result<Sorted, GraphError> {
    if n > MAX_PLUGINS {
        return Err(GraphError::TooMany(n));
    }
    if is_master.len() != n {
        return Err(GraphError::BadIndex(is_master.len()));
    }
    for &(a, b) in hard.iter().chain(soft) {
        if a >= n || b >= n {
            return Err(GraphError::BadIndex(a.max(b)));
        }
    }
    let master = |index: usize| is_master[index];
    let mut reach = Reach::new(n);
    let mut successors: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut add = |reach: &mut Reach, a: usize, b: usize| -> Result<(), GraphError> {
        if reach.reaches(a, b) {
            return Ok(());
        }
        reach.add(a, b)?;
        successors[a].push(b);
        Ok(())
    };
    for &(a, b) in hard {
        if a == b || reach.reaches(b, a) {
            return Err(GraphError::Cycle(a, b));
        }
        if !master(a) && master(b) {
            return Err(GraphError::MasterOrder(a, b));
        }
        add(&mut reach, a, b)?;
    }
    let mut dropped = Vec::new();
    for &(a, b) in soft {
        if a == b || reach.reaches(b, a) || (!master(a) && master(b)) {
            dropped.push((a, b));
        } else {
            add(&mut reach, a, b)?;
        }
    }
    let mut waiting = vec![0usize; n];
    for next in successors.iter().flatten() {
        waiting[*next] += 1;
    }
    let mut ready: BinaryHeap<Reverse<(bool, usize)>> = (0..n)
        .filter(|&index| waiting[index] == 0)
        .map(|index| Reverse((!is_master[index], index)))
        .collect();
    let mut result = Vec::with_capacity(n);
    while let Some(Reverse((_, index))) = ready.pop() {
        result.push(index);
        for &next in &successors[index] {
            waiting[next] -= 1;
            if waiting[next] == 0 {
                ready.push(Reverse((!is_master[next], next)));
            }
        }
    }
    debug_assert_eq!(result.len(), n, "the edges were checked for cycles");
    Ok(Sorted {
        order: result,
        dropped,
    })
}

#[cfg(test)]
#[path = "sort_tests.rs"]
mod tests;
