// SPDX-License-Identifier: GPL-3.0-or-later
//
// Which plugins have something in common, for libloot's overlap edges (see `sort`). The test, "do two sorted
// lists of ids share one", is esplugin's `sorted_slices_intersect` and libloot's `do_assets_overlap`
// (GPL-3.0-or-later, Copyright (C) the libloot authors), with a cheaper search when one list is much shorter and
// the steps counted. See NOTICE.md.

//! Sorted lists of ids per plugin, and the question whether two of them share an id.

use std::cell::Cell;
use std::cmp::Ordering;

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
        let (small, large) = if a.len() <= b.len() { (a, b) } else { (b, a) };
        let (Some(&first), Some(&last)) = (small.first(), small.last()) else {
            return false;
        };
        if large.last().is_some_and(|&top| top < first)
            || large.first().is_some_and(|&bottom| bottom > last)
        {
            return false;
        }
        if small.len().saturating_mul(16) < large.len() {
            let steps = u64::from(large.len().ilog2() + 1);
            self.work.set(self.work.get() + small.len() as u64 * steps);
            return small.iter().any(|key| large.binary_search(key).is_ok());
        }
        self.work
            .set(self.work.get() + (small.len() + large.len()) as u64);
        let (mut x, mut y) = (0, 0);
        while x < small.len() && y < large.len() {
            match small[x].cmp(&large[y]) {
                Ordering::Less => x += 1,
                Ordering::Greater => y += 1,
                Ordering::Equal => return true,
            }
        }
        false
    }
}

/// The overlap data of a load order: the records that plugins have in common and the archive files they load.
#[derive(Debug, Default)]
pub struct PluginOverlaps {
    records: KeySets,
    assets: KeySets,
}

impl PluginOverlaps {
    /// From the sorted record keys and sorted asset keys of each plugin, in the order given to the sorter.
    #[must_use]
    pub fn new(records: Vec<Vec<u64>>, assets: Vec<Vec<u64>>) -> Self {
        Self {
            records: KeySets::new(records),
            assets: KeySets::new(assets),
        }
    }
}

impl Overlaps for PluginOverlaps {
    fn records(&self, a: usize, b: usize) -> bool {
        self.records.share(a, b)
    }

    fn assets(&self, a: usize, b: usize) -> bool {
        self.assets.share(a, b)
    }

    fn work(&self) -> u64 {
        self.records.work() + self.assets.work()
    }
}

#[cfg(test)]
#[path = "overlap_tests.rs"]
mod tests;
