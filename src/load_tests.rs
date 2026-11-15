//! Unit tests of `load` with small limits: the exact repeat when the shared budget refuses, the verdicts, the cache
//! and cancellation.

use super::*;
use crate::archives::synthetic_bsa;
use crate::records::synthetic_records;
use crate::support_tests::TempDir;

const NO_LIMITS: Limits = Limits::DEFAULT;

fn hooks(limits: Limits, cancel: Option<&CancelToken>) -> Hooks<'_> {
    Hooks {
        cancel,
        done: &|| {},
        limits,
    }
}

/// Two plugins of 3 and 2 records, and an unreadable one.
fn plugins(dir: &TempDir) -> Vec<PathBuf> {
    vec![
        dir.file(
            "A.esp",
            &synthetic_records(0, &[], &[(*b"NPC_", &[(1, 0), (2, 0), (3, 0)])]),
        ),
        dir.file(
            "B.esp",
            &synthetic_records(0, &[], &[(*b"NPC_", &[(1, 0), (2, 0)])]),
        ),
        dir.file("Bad.esp", b"TES4 cut"),
    ]
}

fn jobs<'a>(paths: &'a [PathBuf], names: &'a [&'a str]) -> Vec<RecordJob<'a>> {
    paths
        .iter()
        .zip(names)
        .map(|(path, name)| RecordJob {
            name,
            path: Some(path),
            masters: &[],
        })
        .collect()
}

#[test]
fn records_are_read_on_threads_and_an_unreadable_plugin_is_noted() {
    let dir = TempDir::new();
    let paths = plugins(&dir);
    let names = ["A.esp", "B.esp", "Bad.esp"];
    let mut warnings = Vec::new();
    let read = read_records(
        &jobs(&paths, &names),
        4,
        None,
        hooks(NO_LIMITS, None),
        &mut warnings,
    )
    .expect("reads");
    let counts: Vec<usize> = read.iter().map(|records| records.keys.len()).collect();
    assert_eq!(counts, [3, 2, 0]);
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].starts_with("Bad.esp"));
}

#[test]
fn records_over_the_total_are_refused_and_at_the_total_are_not() {
    let dir = TempDir::new();
    let paths = plugins(&dir);
    let names = ["A.esp", "B.esp", "Bad.esp"];
    for (total, fits) in [(5, true), (4, false)] {
        let limits = Limits {
            records_total: total,
            ..NO_LIMITS
        };
        let result = read_records(
            &jobs(&paths, &names),
            4,
            None,
            hooks(limits, None),
            &mut Vec::new(),
        );
        assert_eq!(result.is_ok(), fits, "total {total}");
    }
}

#[test]
fn a_refused_chunked_read_is_repeated_exactly_and_ignores_the_cache() {
    // The chunk (65536) passes a total of 5 at once, so the threads are refused and the exact repeat decides.
    let dir = TempDir::new();
    let paths = plugins(&dir);
    let names = ["A.esp", "B.esp", "Bad.esp"];
    let cache = SortCache::new();
    let limits = Limits {
        records_total: 5,
        ..NO_LIMITS
    };
    let read = read_records(
        &jobs(&paths, &names),
        2,
        Some(&cache),
        hooks(limits, None),
        &mut Vec::new(),
    );
    assert_eq!(read.map(|all| all.len()), Ok(3));
    assert_eq!(
        cache.len(),
        (0, 0),
        "the exact repeat puts nothing in the cache"
    );
}

#[test]
fn a_plugin_over_its_own_limit_is_refused_even_from_the_cache() {
    let dir = TempDir::new();
    let paths = plugins(&dir);
    let names = ["A.esp", "B.esp", "Bad.esp"];
    let cache = SortCache::new();
    read_records(
        &jobs(&paths, &names),
        2,
        Some(&cache),
        hooks(NO_LIMITS, None),
        &mut Vec::new(),
    )
    .expect("reads");
    let limits = Limits {
        records_per_plugin: 2,
        ..NO_LIMITS
    };
    let again = read_records(
        &jobs(&paths, &names),
        2,
        Some(&cache),
        hooks(limits, None),
        &mut Vec::new(),
    );
    assert_eq!(again.map(|_| ()), Err(Stop::OverLimit));
}

#[test]
fn a_cancelled_read_stops() {
    let dir = TempDir::new();
    let paths = plugins(&dir);
    let names = ["A.esp", "B.esp", "Bad.esp"];
    let cancel = CancelToken::new();
    cancel.cancel();
    let records = read_records(
        &jobs(&paths, &names),
        2,
        None,
        hooks(NO_LIMITS, Some(&cancel)),
        &mut Vec::new(),
    );
    assert_eq!(records.map(|_| ()), Err(Stop::Cancelled));
    let assets = read_assets(
        &["A.esp"],
        &[Vec::new()],
        2,
        None,
        hooks(NO_LIMITS, Some(&cancel)),
        &mut Vec::new(),
    );
    assert_eq!(assets.map(|_| ()), Err(Stop::Cancelled));
}

/// A plugin with two archives (3 files, one repeated across them) and one with a broken archive.
fn archives(dir: &TempDir) -> (Vec<&'static str>, Vec<Vec<PathBuf>>) {
    let first = dir.file(
        "A.bsa",
        &synthetic_bsa(104, &[("meshes", &["a.nif", "b.nif"])]),
    );
    let second = dir.file(
        "A - More.bsa",
        &synthetic_bsa(104, &[("meshes", &["b.nif"])]),
    );
    let broken = dir.file("B.bsa", b"not an archive");
    (
        vec!["A.esp", "B.esp"],
        vec![vec![first, second], vec![broken]],
    )
}

#[test]
fn archives_are_read_once_each_and_a_broken_one_is_noted() {
    let dir = TempDir::new();
    let (names, found) = archives(&dir);
    let cache = SortCache::new();
    let mut warnings = Vec::new();
    let read = read_assets(
        &names,
        &found,
        4,
        Some(&cache),
        hooks(NO_LIMITS, None),
        &mut warnings,
    )
    .expect("reads");
    assert_eq!(read[0].keys.len(), 2, "b.nif once");
    assert_eq!(read[1].keys, Vec::<u64>::new());
    assert_eq!(warnings.len(), 1);
    assert_eq!(cache.len().1, 2, "the two readable archives are kept");
    let again = read_assets(
        &names,
        &found,
        4,
        Some(&cache),
        hooks(NO_LIMITS, None),
        &mut Vec::new(),
    )
    .expect("reads");
    assert_eq!(again, read);
}

#[test]
fn archive_files_over_the_total_are_refused_by_the_serial_rule() {
    // A reads 3 file records (2 distinct): a total of 3 fits, 2 does not, also when the chunked read is refused.
    let dir = TempDir::new();
    let (names, found) = archives(&dir);
    for (total, fits) in [(3, true), (2, false)] {
        let limits = Limits {
            assets_total: total,
            ..NO_LIMITS
        };
        let result = read_assets(
            &names,
            &found,
            2,
            None,
            hooks(limits, None),
            &mut Vec::new(),
        );
        assert_eq!(result.is_ok(), fits, "total {total}");
    }
}
