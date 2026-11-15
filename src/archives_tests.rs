//! Unit tests of `archives`: which archives a plugin loads and which files they hold.

use super::*;
use std::io::Cursor;

fn read(bytes: &[u8]) -> Result<Vec<u64>, ArchiveError> {
    read_with(bytes, &PathIds::default(), &mut MAX_ASSETS_TOTAL.clone())
}

fn read_with(bytes: &[u8], ids: &PathIds, budget: &mut usize) -> Result<Vec<u64>, ArchiveError> {
    let mut keys = Vec::new();
    read_archive_from(
        Cursor::new(bytes),
        bytes.len() as u64,
        ids,
        &mut keys,
        budget,
    )?;
    Ok(keys)
}

#[test]
fn the_files_of_every_supported_version_are_read() {
    for version in [103, 104, 105] {
        let bytes = synthetic_bsa(
            version,
            &[
                ("textures\\armor", &["a.dds", "b.dds"]),
                ("meshes", &["c.nif"]),
                ("empty", &[]),
            ],
        );
        let keys = read(&bytes).expect("reads");
        assert_eq!(keys.len(), 3, "version {version}");
        let mut distinct = keys.clone();
        distinct.sort_unstable();
        distinct.dedup();
        assert_eq!(distinct.len(), 3);
    }
}

#[test]
fn the_same_file_has_the_same_key_whatever_the_case_or_slashes_and_another_folder_does_not() {
    let ids = PathIds::default();
    let budget = &mut MAX_ASSETS_TOTAL.clone();
    let one = read_with(
        &synthetic_bsa(104, &[("Textures/Armor", &["Hat.DDS"])]),
        &ids,
        budget,
    )
    .expect("reads");
    let two = read_with(
        &synthetic_bsa(104, &[("\\textures\\armor\\", &["hat.dds"])]),
        &ids,
        budget,
    )
    .expect("reads");
    let elsewhere = read_with(
        &synthetic_bsa(104, &[("textures", &["hat.dds"])]),
        &ids,
        budget,
    )
    .expect("reads");
    assert_eq!(one, two);
    assert_ne!(one, elsewhere);
}

#[test]
fn a_count_that_promises_more_names_than_there_are_is_stopped_by_the_names() {
    let mut bytes = synthetic_bsa(104, &[("a", &["x.dds"])]);
    // The folder record's file count: 36 + 8.
    bytes[44..48].copy_from_slice(&1_000_000u32.to_le_bytes());
    // The tables are the same size, so the file length does not give it away; the names do. (As in libloot, the
    // text after the last zero byte counts as one more, empty, name.)
    assert!(read(&bytes).expect("reads").len() <= 2);
}

#[test]
fn archives_that_are_not_supported_or_not_whole_are_errors() {
    let good = synthetic_bsa(104, &[("a", &["x.dds", "y.dds"])]);
    let patch = |at: usize, value: u32| {
        let mut bytes = good.clone();
        bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
        bytes
    };
    assert!(matches!(read(b"BSA\0"), Err(ArchiveError::Malformed(_))));
    assert!(matches!(read(&[0; 36]), Err(ArchiveError::Unsupported(_))));
    assert!(matches!(
        read(&patch(4, 106)),
        Err(ArchiveError::Unsupported(_))
    ));
    assert!(matches!(
        read(&patch(12, 3 | 0x40)),
        Err(ArchiveError::Unsupported(_))
    ));
    assert!(matches!(
        read(&patch(12, 1)),
        Err(ArchiveError::Unsupported(_))
    ));
    assert!(matches!(
        read(&patch(12, 2)),
        Err(ArchiveError::Unsupported(_))
    ));
    assert!(matches!(
        read(&patch(8, 40)),
        Err(ArchiveError::Malformed(_))
    ));
    // More folders or files than the file holds, however large the number.
    assert!(matches!(
        read(&patch(16, u32::MAX)),
        Err(ArchiveError::Malformed(_))
    ));
    assert!(matches!(
        read(&patch(20, u32::MAX)),
        Err(ArchiveError::Malformed(_))
    ));
    assert!(matches!(
        read(&patch(28, u32::MAX)),
        Err(ArchiveError::Malformed(_))
    ));
    assert!(matches!(
        read(&good[..good.len() - 1]),
        Err(ArchiveError::Malformed(_))
    ));
    // A folder name before, past or beyond the file records.
    assert!(matches!(
        read(&patch(48, 0)),
        Err(ArchiveError::Malformed(_))
    ));
    assert!(matches!(
        read(&patch(48, u32::MAX)),
        Err(ArchiveError::Malformed(_))
    ));
    let mut long_name = good.clone();
    // The length byte of the folder name follows the header (36) and the one folder record (16).
    long_name[52] = 200;
    assert!(matches!(read(&long_name), Err(ArchiveError::Malformed(_))));
}

#[test]
fn tables_over_the_limit_are_refused_before_anything_is_allocated_by_them() {
    // A header that announces 4 million folders in a reader that claims to be enormous.
    let mut bytes = synthetic_bsa(104, &[("a", &["x.dds"])]);
    bytes[16..20].copy_from_slice(&4_000_000u32.to_le_bytes());
    bytes[20..24].copy_from_slice(&4_000_000u32.to_le_bytes());
    let mut keys = Vec::new();
    let result = read_archive_from(
        Cursor::new(&bytes),
        u64::MAX / 4,
        &PathIds::default(),
        &mut keys,
        &mut MAX_ASSETS_TOTAL.clone(),
    );
    assert!(matches!(result, Err(ArchiveError::TooMuch)), "{result:?}");
}

#[test]
fn more_files_than_the_budget_leaves_is_an_error_and_adds_nothing() {
    let bytes = synthetic_bsa(104, &[("a", &["1", "2", "3"])]);
    let mut keys = vec![42];
    let result = read_archive_from(
        Cursor::new(&bytes),
        bytes.len() as u64,
        &PathIds::default(),
        &mut keys,
        &mut 3,
    );
    assert!(matches!(result, Err(ArchiveError::TooMuch)));
    assert_eq!(keys, [42]);
    assert_eq!(
        read_with(&bytes, &PathIds::default(), &mut 4)
            .expect("fits")
            .len(),
        3
    );
}

fn data_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("loot-rs-archives-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create");
    dir
}

#[test]
fn a_plugin_loads_the_archives_whose_names_begin_with_its_name() {
    let dir = data_dir("find");
    for name in [
        "Mod.esp",
        "Mod.bsa",
        "mod - Textures.BSA",
        "Modern.bsa",
        "Other.bsa",
        "Mod.txt",
        "Mo.bsa",
    ] {
        std::fs::write(dir.join(name), b"x").expect("write");
    }
    std::fs::create_dir(dir.join("Mod - Folder.bsa")).expect("dir");
    let names = |paths: Vec<PathBuf>| -> Vec<String> {
        paths
            .iter()
            .map(|path| {
                path.file_name()
                    .expect("name")
                    .to_string_lossy()
                    .into_owned()
            })
            .collect()
    };
    assert_eq!(
        names(associated_archives(&dir, "MOD.ESP")),
        ["Mod.bsa", "Modern.bsa", "mod - Textures.BSA"]
    );
    assert_eq!(names(associated_archives(&dir, "Other.esm")), ["Other.bsa"]);
    assert_eq!(
        names(associated_archives(&dir, "None.esp")),
        Vec::<String>::new()
    );
    assert_eq!(
        names(associated_archives(&dir, ".esp")),
        Vec::<String>::new()
    );
    assert_eq!(
        associated_archives(&dir.join("missing"), "Mod.esp"),
        Vec::<PathBuf>::new()
    );
    std::fs::remove_dir_all(&dir).expect("remove");
}

#[test]
fn the_archives_of_a_plugin_are_merged_and_one_that_cannot_be_read_is_skipped() {
    let dir = data_dir("merge");
    let write = |name: &str, bytes: &[u8]| -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, bytes).expect("write");
        path
    };
    let one = write(
        "A - Main.bsa",
        &synthetic_bsa(104, &[("x", &["1.dds", "2.dds"])]),
    );
    let two = write(
        "A - More.bsa",
        &synthetic_bsa(104, &[("x", &["2.dds", "3.dds"])]),
    );
    let bad = write(
        "A - Bad.bsa",
        b"not an archive at all, but longer than a header....",
    );
    let missing = dir.join("A - Gone.bsa");
    let mut skipped = Vec::new();
    let mut budget = 100;
    let assets = read_plugin_assets(
        &[one.clone(), bad, two, missing],
        &PathIds::default(),
        &mut budget,
        &mut skipped,
    )
    .expect("reads");
    assert_eq!(assets.keys.len(), 3, "2.dds is in both");
    assert!(assets.keys.windows(2).all(|pair| pair[0] < pair[1]));
    assert_eq!(budget, 97);
    assert_eq!(skipped.len(), 2);
    assert!(
        skipped
            .iter()
            .all(|(_, error)| !error.to_string().is_empty())
    );
    let result = read_plugin_assets(&[one], &PathIds::default(), &mut 1, &mut skipped);
    assert!(matches!(result, Err(ArchiveError::TooMuch)));
    std::fs::remove_dir_all(&dir).expect("remove");
}

#[test]
fn errors_read_as_sentences() {
    for error in [
        ArchiveError::Io(io::Error::other("denied")),
        ArchiveError::Unsupported("BSA version 7".into()),
        ArchiveError::Malformed("x"),
        ArchiveError::TooMuch,
    ] {
        assert_ne!(error.to_string(), "");
    }
}

#[test]
fn an_index_of_several_directories_lists_each_directory_s_archives_in_order() {
    let first = crate::support_tests::TempDir::new();
    let second = crate::support_tests::TempDir::new();
    std::fs::write(first.path().join("Mod - Main.bsa"), b"").expect("write");
    std::fs::write(second.path().join("mod.bsa"), b"").expect("write");
    std::fs::write(second.path().join("Other.bsa"), b"").expect("write");
    std::fs::write(second.path().join("Mod.txt"), b"").expect("write");
    let index = ArchiveIndex::new(&[first.path(), second.path()]);
    let found = index.archives_of("Mod.esp");
    assert_eq!(
        found,
        [
            first.path().join("Mod - Main.bsa"),
            second.path().join("mod.bsa")
        ]
    );
    assert_eq!(
        associated_archives(second.path(), "Mod.esp"),
        [second.path().join("mod.bsa")]
    );
}
