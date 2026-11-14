// SPDX-License-Identifier: GPL-3.0-only
//! Compare what loot-rs reads from a plugin with what esplugin reads from it: the header (master flag, masters,
//! description, header version, record count) and the records (how many override a master's record, and, for two
//! plugins, whether and how much they overlap, which is what libloot's `do_records_overlap` and
//! `override_record_count` take from esplugin). Differences are returned as text for the caller; the real-data
//! mode of `oracle_run.py` keeps only their counts.

use std::collections::HashSet;
use std::path::Path;

use esplugin::{GameId, ParseOptions, Plugin};
use loot_rs::plugin::read_header;
use loot_rs::records::{NameIds, PluginRecords, read_records};

/// What one comparison found: differences in the header, and in the records.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Differences {
    pub header: Vec<String>,
    pub records: Vec<String>,
}

impl Differences {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.header.is_empty() && self.records.is_empty()
    }
}

/// A plugin as both readers see it.
pub struct Both {
    pub name: String,
    pub esplugin: Option<Plugin>,
    pub records: Option<PluginRecords>,
}

fn same<T: PartialEq + std::fmt::Debug>(out: &mut Vec<String>, what: &str, ours: &T, theirs: &T) {
    if ours != theirs {
        out.push(format!("{what}: loot-rs {ours:?}, esplugin {theirs:?}"));
    }
}

/// Read `path` (named `name` for the record keys) with both readers and compare the single-plugin facts.
///
/// A plugin that one reader refuses and the other reads is a difference; both refusing is agreement.
#[must_use]
pub fn compare_plugin(path: &Path, name: &str, names: &mut NameIds) -> (Both, Differences) {
    let mut found = Differences::default();
    let mut theirs = Plugin::new(GameId::FalloutNV, path);
    let theirs_ok = theirs.parse_file(ParseOptions::whole_plugin()).is_ok();
    let ours = read_header(path);
    let mut both = Both {
        name: name.to_string(),
        esplugin: None,
        records: None,
    };
    match (ours, theirs_ok) {
        (Err(_), false) => return (both, found),
        (Err(error), true) => {
            found
                .header
                .push(format!("only esplugin reads it (loot-rs: {error})"));
            return (both, found);
        }
        (Ok(_), false) => {
            found.header.push("only loot-rs reads the header".into());
            return (both, found);
        }
        (Ok(_), true) => {}
    }
    let header = read_header(path).expect("read above");
    same(
        &mut found.header,
        "master flag",
        &header.is_master(),
        &theirs.is_master_file(),
    );
    match theirs.masters() {
        Ok(masters) => same(&mut found.header, "masters", &header.masters, &masters),
        Err(error) => found.header.push(format!("esplugin masters: {error}")),
    }
    match theirs.description() {
        Ok(description) => same(
            &mut found.header,
            "description",
            &header.description,
            &description,
        ),
        Err(error) => found.header.push(format!("esplugin description: {error}")),
    }
    same(
        &mut found.header,
        "header version (bits)",
        &header.version.map(f32::to_bits),
        &theirs.header_version().map(f32::to_bits),
    );
    same(
        &mut found.header,
        "record count",
        &header.record_count,
        &theirs.record_and_group_count(),
    );
    let mut budget = loot_rs::records::MAX_RECORDS_TOTAL;
    match read_records(path, name, &header.masters, names, &mut budget) {
        Ok(records) => {
            match theirs.count_override_records() {
                Ok(count) => same(
                    &mut found.records,
                    "override record count",
                    &records.overridden,
                    &count,
                ),
                Err(error) => found
                    .records
                    .push(format!("esplugin override count: {error}")),
            }
            both.records = Some(records);
        }
        Err(error) => found.records.push(format!(
            "only esplugin reads the records (loot-rs: {error})"
        )),
    }
    both.esplugin = Some(theirs);
    (both, found)
}

/// How many of `a`'s records (each counted as often as the plugin repeats it) are also in `b`, as loot-rs's keys say
/// (the owner and object of each record): the quantity esplugin's `overlap_size` returns.
fn shared(a: &PluginRecords, b: &PluginRecords) -> usize {
    let theirs: HashSet<u64> = b.keys.iter().copied().collect();
    a.keys.iter().filter(|key| theirs.contains(key)).count()
}

/// Compare how two plugins overlap: whether they share a record (`do_records_overlap`) and how many of the first's
/// records are in the second (`overlap_size`), both ways round.
#[must_use]
pub fn compare_pair(a: &Both, b: &Both) -> Vec<String> {
    let mut found = Vec::new();
    let (Some(ra), Some(rb), Some(ea), Some(eb)) =
        (&a.records, &b.records, &a.esplugin, &b.esplugin)
    else {
        return found;
    };
    for (x, y, rx, ry, ex, ey) in [(a, b, ra, rb, ea, eb), (b, a, rb, ra, eb, ea)] {
        let size = ex.overlap_size(&[ey]).unwrap_or(usize::MAX);
        same(
            &mut found,
            &format!("records of {} also in {}", x.name, y.name),
            &shared(rx, ry),
            &size,
        );
    }
    let overlaps = ea.overlaps_with(eb).unwrap_or(false);
    same(
        &mut found,
        &format!("{} and {} share a record", a.name, b.name),
        &(shared(ra, rb) > 0),
        &overlaps,
    );
    found
}

/// Compare every plugin of `names` in `data` (a directory), and every pair of them.
#[must_use]
pub fn compare_all(data: &Path, names: &[String]) -> (usize, Differences) {
    let mut ids = NameIds::default();
    let mut total = Differences::default();
    let mut read = Vec::new();
    for name in names {
        let (both, found) = compare_plugin(&data.join(name), name, &mut ids);
        total
            .header
            .extend(found.header.into_iter().map(|d| format!("{name}: {d}")));
        total
            .records
            .extend(found.records.into_iter().map(|d| format!("{name}: {d}")));
        read.push(both);
    }
    for (index, a) in read.iter().enumerate() {
        for b in &read[index + 1..] {
            total.records.extend(compare_pair(a, b));
        }
    }
    (names.len(), total)
}
