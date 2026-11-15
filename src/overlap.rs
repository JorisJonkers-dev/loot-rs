// SPDX-License-Identifier: GPL-3.0-or-later
//
// Which plugins have something in common, for libloot's overlap edges (see `sort`). The test, "do two sorted
// lists of ids share one", is esplugin's `sorted_slices_intersect` and libloot's `do_assets_overlap`
// (GPL-3.0-or-later, Copyright (C) the libloot authors), with a cheaper search when one list is much shorter and
// the steps counted. See NOTICE.md.

//! Sorted lists of ids per plugin, and the question whether two of them share an id.

use std::cell::Cell;
use std::cmp::Ordering;
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};

use crate::sort::Overlaps;

/// One sorted list of `u64` ids for each plugin of the list given to the sorter.
#[derive(Debug, Default)]
pub struct KeySets {
    sets: Vec<Vec<u64>>,
    work: Cell<u64>,
}

impl KeySets {
    /// `sets[i]` is the sorted ids of plugin `i` (empty for a plugin without any).
    #[must_use]
    pub fn new(sets: Vec<Vec<u64>>) -> Self {
        Self {
            sets,
            work: Cell::new(0),
        }
    }

    /// Do plugins `a` and `b` share an id? An index that is not a plugin shares nothing.
    #[must_use]
    pub fn share(&self, a: usize, b: usize) -> bool {
        match (self.sets.get(a), self.sets.get(b)) {
            (Some(a), Some(b)) => self.intersect(a, b),
            _ => false,
        }
    }

    /// Steps spent on answering so far.
    #[must_use]
    pub fn work(&self) -> u64 {
        self.work.get()
    }

    fn intersect(&self, a: &[u64], b: &[u64]) -> bool {
        let (shared, steps) = intersect(a, b);
        self.work.set(self.work.get() + steps);
        shared
    }
}

/// Do two sorted lists share an id, and how many steps did it take to find out? The steps depend only on the two
/// lists, so a total over many pairs does not depend on the order in which they are compared.
#[must_use]
pub fn intersect(a: &[u64], b: &[u64]) -> (bool, u64) {
    let (small, large) = if a.len() <= b.len() { (a, b) } else { (b, a) };
    let (Some(&first), Some(&last)) = (small.first(), small.last()) else {
        return (false, 0);
    };
    if large.last().is_some_and(|&top| top < first)
        || large.first().is_some_and(|&bottom| bottom > last)
    {
        return (false, 0);
    }
    if small.len().saturating_mul(16) < large.len() {
        let steps = u64::from(large.len().ilog2() + 1);
        let shared = small.iter().any(|key| large.binary_search(key).is_ok());
        return (shared, small.len() as u64 * steps);
    }
    let steps = (small.len() + large.len()) as u64;
    let (mut x, mut y) = (0, 0);
    while x < small.len() && y < large.len() {
        match small[x].cmp(&large[y]) {
            Ordering::Less => x += 1,
            Ordering::Greater => y += 1,
            Ordering::Equal => return (true, steps),
        }
    }
    (false, steps)
}

fn keys_of(sets: &[Vec<u64>], index: usize) -> &[u64] {
    sets.get(index).map_or(&[], Vec::as_slice)
}

/// Most steps of record and archive comparisons [`PluginOverlaps::precompute`] spends. Each plugin's ids are compared
/// with at most [`crate::sort::MAX_PLUGINS`] others, so the limits on records and archive files bound the work by
/// about 20 billion steps anyway; this stops well before that.
pub const OVERLAP_WORK_BUDGET: u64 = 8_000_000_000;

/// What the sorter knows of each plugin that decides which pairs it compares: the master flag (plugins are only
/// compared within their partition), and the counts of override records and of archive files.
#[derive(Debug, Clone, Copy)]
pub struct PairNeeds<'a> {
    /// The master flag of each plugin.
    pub is_master: &'a [bool],
    /// Override records of each plugin.
    pub override_records: &'a [usize],
    /// Archive files of each plugin.
    pub assets: &'a [usize],
}

/// Answers for pairs of plugins, computed ahead of the sort.
#[derive(Debug, Default)]
struct PairTable {
    n: usize,
    /// For `a * n + b`: the bits below.
    cells: Vec<u8>,
}

const RECORDS_KNOWN: u8 = 1;
const RECORDS: u8 = 2;
const ASSETS_KNOWN: u8 = 4;
const ASSETS: u8 = 8;

impl PairTable {
    /// The answer for the pair when it was computed: `known` set, then `bit` says it.
    fn get(&self, a: usize, b: usize, known: u8, bit: u8) -> Option<bool> {
        let at = a.checked_mul(self.n)?.checked_add(b)?;
        let cell = *self.cells.get(at).filter(|_| a < self.n && b < self.n)?;
        (cell & known != 0).then_some(cell & bit != 0)
    }
}

/// The overlap data of a load order: the records that plugins have in common and the archive files they load.
#[derive(Debug, Default)]
pub struct PluginOverlaps {
    records: KeySets,
    assets: KeySets,
    table: PairTable,
}

impl PluginOverlaps {
    /// From the sorted record keys and sorted asset keys of each plugin, in the order given to the sorter.
    #[must_use]
    pub fn new(records: Vec<Vec<u64>>, assets: Vec<Vec<u64>>) -> Self {
        Self {
            records: KeySets::new(records),
            assets: KeySets::new(assets),
            table: PairTable::default(),
        }
    }

    /// Compare, on up to `threads` threads, every pair the sorter can ask about, so that its questions are answered
    /// from a table. The pairs are those the sorter's overlap stage looks at: two plugins of the same partition whose
    /// override record counts differ (records), and, where records do not decide, whose archive file counts differ
    /// (assets). The answers are the same as asking one at a time; a pair that was not compared is still answered
    /// by comparing it then.
    ///
    /// # Errors
    /// `Err(())` when the comparisons need more than [`OVERLAP_WORK_BUDGET`] steps. The verdict depends only on the
    /// input: the steps of each pair are fixed, and the work stops early only once the total is known to pass the
    /// limit.
    #[allow(clippy::result_unit_err)]
    pub fn precompute(&mut self, needs: PairNeeds, threads: usize) -> Result<(), ()> {
        let n = needs
            .is_master
            .len()
            .min(needs.override_records.len())
            .min(needs.assets.len());
        let records = &self.records.sets;
        let assets = &self.assets.sets;
        let work = AtomicU64::new(0);
        let rows: Vec<usize> = (0..n).collect();
        let computed: Vec<Option<Vec<u8>>> = crate::parallel::map(&rows, threads, |&a| {
            let mut row = vec![0u8; n];
            let mut steps = 0u64;
            for (b, slot) in row.iter_mut().enumerate().skip(a + 1) {
                if needs.is_master[a] != needs.is_master[b] {
                    continue;
                }
                let mut cell = 0;
                let mut decided = false;
                if needs.override_records[a] != needs.override_records[b] {
                    let (shared, cost) = intersect(keys_of(records, a), keys_of(records, b));
                    steps += cost;
                    cell |= RECORDS_KNOWN | if shared { RECORDS } else { 0 };
                    decided = shared;
                }
                if !decided && needs.assets[a] != needs.assets[b] {
                    let (shared, cost) = intersect(keys_of(assets, a), keys_of(assets, b));
                    steps += cost;
                    cell |= ASSETS_KNOWN | if shared { ASSETS } else { 0 };
                }
                *slot = cell;
                if steps > 1 << 20 {
                    if work.fetch_add(steps, AtomicOrdering::Relaxed) + steps > OVERLAP_WORK_BUDGET
                    {
                        return None;
                    }
                    steps = 0;
                }
            }
            if work.fetch_add(steps, AtomicOrdering::Relaxed) + steps > OVERLAP_WORK_BUDGET {
                return None;
            }
            Some(row)
        });
        let mut cells = vec![0u8; n * n];
        for (a, row) in computed.into_iter().enumerate() {
            let row = row.ok_or(())?;
            for (b, &cell) in row.iter().enumerate() {
                if cell != 0 {
                    cells[a * n + b] = cell;
                    cells[b * n + a] = cell;
                }
            }
        }
        self.table = PairTable { n, cells };
        Ok(())
    }
}

impl Overlaps for PluginOverlaps {
    fn records(&self, a: usize, b: usize) -> bool {
        self.table
            .get(a, b, RECORDS_KNOWN, RECORDS)
            .unwrap_or_else(|| self.records.share(a, b))
    }

    fn assets(&self, a: usize, b: usize) -> bool {
        self.table
            .get(a, b, ASSETS_KNOWN, ASSETS)
            .unwrap_or_else(|| self.assets.share(a, b))
    }

    fn work(&self) -> u64 {
        self.records.work() + self.assets.work()
    }
}

#[cfg(test)]
#[path = "overlap_tests.rs"]
mod tests;
