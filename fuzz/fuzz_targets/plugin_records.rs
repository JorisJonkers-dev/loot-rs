#![no_main]

use libfuzzer_sys::arbitrary::Unstructured;
use libfuzzer_sys::fuzz_target;
use loot_rs::overlap::KeySets;
use loot_rs::records::{read_records_from, NameIds, MAX_RECORDS_TOTAL};
use std::io::Cursor;

// Any bytes as a plugin: an error, or sorted keys, a count of overridden records that fits, and a budget that went
// down by what was kept. Reading is bounded by the length of the input whatever the sizes in it say.
fuzz_target!(|data: &[u8]| {
    let mut u = Unstructured::new(data);
    let Ok(count) = u.int_in_range(0..=4usize) else {
        return;
    };
    let masters: Vec<String> = (0..count).map(|index| format!("M{index}.esm")).collect();
    let rest = u.take_rest();
    let mut names = NameIds::default();
    let mut budget = MAX_RECORDS_TOTAL;
    let Ok(records) = read_records_from(
        Cursor::new(rest),
        rest.len() as u64,
        "Mod.esp",
        &masters,
        &mut names,
        &mut budget,
    ) else {
        assert_eq!(budget, MAX_RECORDS_TOTAL);
        return;
    };
    assert!(records.keys.windows(2).all(|pair| pair[0] <= pair[1]));
    assert!(records.overridden <= records.keys.len());
    assert_eq!(budget, MAX_RECORDS_TOTAL - records.keys.len());
    let overlaps = KeySets::new(vec![records.keys.clone(), records.keys]);
    assert_eq!(overlaps.share(0, 1), overlaps.share(1, 0));
});
