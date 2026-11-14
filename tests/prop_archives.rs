//! Property-based tests of the archive reader (`loot_rs::archives`) on synthetic archives and on arbitrary bytes.

mod common;

use loot_rs::archives::{MAX_ASSETS_TOTAL, PathIds, read_archive_from, synthetic_bsa};
use proptest::prelude::*;
use std::collections::HashSet;
use std::io::Cursor;

fn read(bytes: &[u8], ids: &mut PathIds) -> Result<Vec<u64>, loot_rs::archives::ArchiveError> {
    let mut keys = Vec::new();
    read_archive_from(
        Cursor::new(bytes),
        bytes.len() as u64,
        ids,
        &mut keys,
        &mut MAX_ASSETS_TOTAL.clone(),
    )?;
    Ok(keys)
}

proptest! {
    #![proptest_config(common::proptest_config(256))]

    // Every file listed comes back as a key, in any of the three versions; two files are the same file exactly when
    // their folder and file names are equal once case and slash direction are ignored.
    #[test]
    fn it_reads_back_the_files_an_archive_lists(
        version in prop::sample::select(vec![103u32, 104, 105]),
        folders in proptest::collection::vec(
            ("[A-Za-z/\\\\]{1,12}", proptest::collection::vec("[A-Za-z0-9.]{1,8}", 0..6)),
            0..5,
        ),
    ) {
        let refs: Vec<(&str, Vec<&str>)> = folders
            .iter()
            .map(|(name, files)| (name.as_str(), files.iter().map(String::as_str).collect()))
            .collect();
        let borrowed: Vec<(&str, &[&str])> = refs.iter().map(|(name, files)| (*name, files.as_slice())).collect();
        let bytes = synthetic_bsa(version, &borrowed);
        let keys = read(&bytes, &mut PathIds::default()).expect("reads");
        let listed: usize = folders.iter().map(|(_, files)| files.len()).sum();
        prop_assert_eq!(keys.len(), listed);
        let normal = |text: &str| text.replace('/', "\\").trim_matches('\\').to_ascii_lowercase();
        let distinct: HashSet<(String, String)> = folders
            .iter()
            .flat_map(|(folder, files)| files.iter().map(move |file| (normal(folder), normal(file))))
            .collect();
        prop_assert_eq!(keys.iter().collect::<HashSet<_>>().len(), distinct.len());
        // Every cut of the archive is an error or the same files, never more and never a panic.
        for cut in (0..bytes.len()).step_by(5) {
            if let Ok(part) = read(&bytes[..cut], &mut PathIds::default()) {
                prop_assert!(part.len() <= listed);
            }
        }
    }

    #[test]
    fn arbitrary_bytes_are_an_error_or_bounded_keys(bytes in proptest::collection::vec(any::<u8>(), 0..600)) {
        let mut keys = vec![1];
        let mut budget = MAX_ASSETS_TOTAL;
        let result = read_archive_from(Cursor::new(&bytes), bytes.len() as u64, &mut PathIds::default(), &mut keys, &mut budget);
        prop_assert_eq!(budget, MAX_ASSETS_TOTAL);
        if result.is_err() {
            prop_assert_eq!(keys, vec![1]);
        }
    }

    // Starting from a good archive, any four bytes of the header or tables overwritten: an error or a result, and
    // never an allocation by a size that the file does not hold (the test would not return).
    #[test]
    fn a_damaged_archive_is_an_error_or_bounded(at in 0usize..120, value in any::<u32>()) {
        let mut bytes = synthetic_bsa(104, &[("meshes", &["a.nif", "b.nif"]), ("textures", &["c.dds"])]);
        let at = at.min(bytes.len() - 4);
        bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
        if let Ok(keys) = read(&bytes, &mut PathIds::default()) {
            prop_assert!(keys.len() <= bytes.len());
        }
    }
}
