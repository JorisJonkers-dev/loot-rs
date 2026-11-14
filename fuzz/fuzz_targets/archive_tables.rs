#![no_main]

use libfuzzer_sys::fuzz_target;
use loot_rs::archives::{read_archive_from, PathIds, MAX_ASSETS_TOTAL};
use std::io::Cursor;

// Any bytes as a BSA: an error that leaves `keys` as it was, or keys within the budget, and the budget is not
// touched by the reader. The sizes in the header are checked against the length of the input before they are
// allocated, so no input makes it allocate more than the limit on tables.
fuzz_target!(|data: &[u8]| {
    let mut keys = vec![7u64];
    let mut budget = MAX_ASSETS_TOTAL;
    let result = read_archive_from(
        Cursor::new(data),
        data.len() as u64,
        &mut PathIds::default(),
        &mut keys,
        &mut budget,
    );
    assert_eq!(budget, MAX_ASSETS_TOTAL);
    match result {
        Ok(()) => assert!(keys.len() <= MAX_ASSETS_TOTAL + 1 && keys[0] == 7),
        Err(_) => assert_eq!(keys, [7]),
    }
});
