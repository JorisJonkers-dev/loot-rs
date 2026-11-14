//! Unit tests of `records`: the record ids of synthetic plugins and the overlap between them.

use super::*;
use std::io::Cursor;

fn read(
    bytes: Vec<u8>,
    own: &str,
    masters: &[&str],
    names: &mut NameIds,
    budget: &mut usize,
) -> Result<PluginRecords, RecordsError> {
    let length = bytes.len() as u64;
    let masters: Vec<String> = masters.iter().map(ToString::to_string).collect();
    read_records_from(Cursor::new(bytes), length, own, &masters, names, budget)
}

fn records(bytes: Vec<u8>, own: &str, masters: &[&str]) -> PluginRecords {
    read(
        bytes,
        own,
        masters,
        &mut NameIds::default(),
        &mut MAX_RECORDS_TOTAL.clone(),
    )
    .expect("reads")
}

#[test]
fn a_plugin_with_only_a_header_has_no_records() {
    let bytes = synthetic_records(0, &[], &[]);
    assert_eq!(records(bytes, "a.esp", &[]), PluginRecords::default());
}

#[test]
fn records_are_counted_in_every_group_and_those_of_a_master_are_overridden() {
    let bytes = synthetic_records(
        0,
        &["Base.esm", "Second.esm"],
        &[
            (
                *b"NPC_",
                &[(0x0000_0001, 0), (0x0100_0002, 7), (0x0200_0003, 0)],
            ),
            (*b"WEAP", &[(0x0000_0001, 4), (0, 0)]),
        ],
    );
    let result = records(bytes, "Mod.esp", &["Base.esm", "Second.esm"]);
    // Form id 0 is no record; index 2 is past the two masters, so the plugin's own.
    assert_eq!(result.keys.len(), 4);
    assert_eq!(result.overridden, 3);
    assert!(result.keys.windows(2).all(|pair| pair[0] <= pair[1]));
}

#[test]
fn the_same_record_has_the_same_key_in_the_plugin_that_adds_it_and_the_one_that_overrides_it() {
    let adds = synthetic_records(1, &[], &[(*b"NPC_", &[(0x0000_0800, 0), (0x0000_0801, 0)])]);
    let overrides = synthetic_records(0, &["BASE.ESM"], &[(*b"NPC_", &[(0x0000_0800, 0)])]);
    let mut names = NameIds::default();
    let mut budget = 100;
    let base = read(adds, "base.esm", &[], &mut names, &mut budget).expect("reads");
    let patch = read(
        overrides,
        "Patch.esp",
        &["BASE.ESM"],
        &mut names,
        &mut budget,
    )
    .expect("reads");
    assert_eq!(budget, 100 - 3);
    assert_eq!((base.overridden, patch.overridden), (0, 1));
    let overlaps = RecordOverlaps::new(vec![base.keys, patch.keys, Vec::new()]);
    assert!(overlaps.records(0, 1) && overlaps.records(1, 0));
    assert!(!overlaps.records(0, 2) && !overlaps.records(2, 1));
    assert!(
        !overlaps.records(0, 9),
        "an unknown plugin overlaps nothing"
    );
    assert!(!overlaps.assets(0, 1));
    assert!(overlaps.work() > 0);
}

#[test]
fn plugins_that_add_records_with_the_same_object_do_not_overlap() {
    let one = synthetic_records(0, &[], &[(*b"NPC_", &[(0x0000_0800, 0)])]);
    let two = synthetic_records(0, &[], &[(*b"NPC_", &[(0x0000_0800, 0)])]);
    let mut names = NameIds::default();
    let mut budget = 100;
    let a = read(one, "a.esp", &[], &mut names, &mut budget).expect("reads");
    let b = read(two, "b.esp", &[], &mut names, &mut budget).expect("reads");
    assert!(!RecordOverlaps::new(vec![a.keys, b.keys]).records(0, 1));
}

#[test]
fn overlap_is_found_by_search_when_one_plugin_is_much_smaller_and_by_merging_otherwise() {
    let large: Vec<u64> = (0..1000).map(|object| object << 32).collect();
    let overlaps = RecordOverlaps::new(vec![
        large.clone(),
        vec![500 << 32],
        vec![5000 << 32],
        (0..1000).map(|object| (object << 32) | 1).collect(),
        vec![999 << 32, 2000 << 32],
        large,
    ]);
    assert!(overlaps.records(0, 1));
    assert!(
        !overlaps.records(0, 2),
        "past the end of the other plugin's range"
    );
    assert!(!overlaps.records(0, 3), "same objects, different owners");
    assert!(overlaps.records(0, 4));
    assert!(overlaps.records(0, 5), "merged: equal sizes");
    assert!(!overlaps.records(5, 3));
    assert!(!RecordOverlaps::new(vec![vec![1, 2, 3], vec![5 << 32]]).records(0, 1));
    assert!(!RecordOverlaps::default().records(0, 0));
    // A gap in the middle that a range check does not catch.
    let gap = RecordOverlaps::new(vec![
        vec![1 << 32, 9 << 32],
        vec![2 << 32, 3 << 32, 4 << 32],
    ]);
    assert!(!gap.records(0, 1));
}

#[test]
fn names_are_compared_without_regard_to_case() {
    let mut names = NameIds::default();
    assert_eq!(names.id("Base.ESM"), names.id("base.esm"));
    assert_ne!(names.id("a.esp"), names.id("b.esp"));
}

#[test]
fn a_file_that_is_not_a_plugin_or_is_cut_off_is_an_error() {
    let mut names = NameIds::default();
    let mut budget = 100;
    let mut run = |bytes: Vec<u8>| read(bytes, "a.esp", &[], &mut names, &mut budget);
    assert!(matches!(run(Vec::new()), Err(RecordsError::Header(_))));
    assert!(matches!(
        run(b"NOPE".repeat(6)),
        Err(RecordsError::Header(_))
    ));
    assert!(matches!(
        run(b"NOPE".to_vec()),
        Err(RecordsError::Truncated)
    ));
    let good = synthetic_records(0, &[], &[(*b"NPC_", &[(0x0000_0800, 16)])]);
    // Cut in the middle of the header record, of a record header and of a record's data.
    for cut in [30, good.len() - 20, good.len() - 5] {
        assert!(
            matches!(run(good[..cut].to_vec()), Err(RecordsError::Truncated)),
            "cut at {cut}"
        );
    }
    // A record that announces far more data than the file holds is refused without reading it.
    let mut huge = good.clone();
    let record = huge.len() - 24 - 16;
    huge[record + 4..record + 8].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(matches!(run(huge), Err(RecordsError::Truncated)));
}

#[test]
fn the_limits_on_records_are_errors() {
    let bytes = synthetic_records(0, &[], &[(*b"NPC_", &[(0x800, 0), (0x801, 0), (0x802, 0)])]);
    let mut names = NameIds::default();
    let mut budget = 2;
    assert!(matches!(
        read(bytes.clone(), "a.esp", &[], &mut names, &mut budget),
        Err(RecordsError::TooMany)
    ));
    let mut budget = 3;
    assert!(read(bytes, "a.esp", &[], &mut names, &mut budget).is_ok());
    assert_eq!(budget, 0);
}

#[test]
fn records_are_read_from_a_file_and_a_missing_file_is_an_error() {
    let dir = std::env::temp_dir().join(format!("loot-rs-records-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create");
    let path = dir.join("A.esp");
    std::fs::write(
        &path,
        synthetic_records(0, &["M.esm"], &[(*b"NPC_", &[(0x0000_0800, 3)])]),
    )
    .expect("write");
    let mut names = NameIds::default();
    let mut budget = 10;
    let masters = vec!["M.esm".to_string()];
    let result = read_records(&path, "A.esp", &masters, &mut names, &mut budget).expect("reads");
    assert_eq!((result.keys.len(), result.overridden), (1, 1));
    let missing = read_records(
        &dir.join("none.esp"),
        "none.esp",
        &[],
        &mut names,
        &mut budget,
    );
    assert!(matches!(missing, Err(RecordsError::Io(_))));
    std::fs::remove_dir_all(&dir).expect("remove");
}

#[test]
fn errors_read_as_sentences() {
    for error in [
        RecordsError::Io(io::Error::other("denied")),
        RecordsError::Header("x".into()),
        RecordsError::Truncated,
        RecordsError::TooMany,
    ] {
        assert_ne!(error.to_string(), "");
    }
}
