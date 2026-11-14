//! Property-based tests of the record reader (`loot_rs::records`) on synthetic plugins and on arbitrary bytes.

mod common;

use loot_rs::records::{
    MAX_RECORDS_TOTAL, NameIds, SyntheticGroup, read_records_from, synthetic_records,
};
use proptest::prelude::*;
use std::io::Cursor;

fn masters(count: usize) -> Vec<String> {
    (0..count).map(|index| format!("M{index}.esm")).collect()
}

proptest! {
    #![proptest_config(common::proptest_config(256))]

    // The reader finds exactly the records that were written, counts those whose master byte is in the list,
    // however they are spread over groups, and does not care about the size of their data.
    #[test]
    fn it_reads_back_what_a_plugin_holds(
        master_count in 0usize..5,
        groups in proptest::collection::vec(proptest::collection::vec((any::<u32>(), 0u32..40), 0..12), 0..4),
    ) {
        let names = masters(master_count);
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let group_refs: Vec<SyntheticGroup> = groups.iter().map(|records| (*b"NPC_", records.as_slice())).collect();
        let bytes = synthetic_records(0, &refs, &group_refs);
        let mut budget = MAX_RECORDS_TOTAL;
        let result = read_records_from(Cursor::new(&bytes), bytes.len() as u64, "Mod.esp", &names, &mut NameIds::default(), &mut budget)
            .expect("reads");
        let written: Vec<u32> = groups.iter().flatten().map(|&(id, _)| id).filter(|&id| id != 0).collect();
        prop_assert_eq!(result.keys.len(), written.len());
        prop_assert_eq!(budget, MAX_RECORDS_TOTAL - written.len());
        let overridden = written.iter().filter(|&&id| ((id >> 24) as usize) < master_count).count();
        prop_assert_eq!(result.overridden, overridden);
        prop_assert!(result.keys.windows(2).all(|pair| pair[0] <= pair[1]));
        // Every cut of the plugin is an error or fewer records, never more and never a panic.
        for cut in (0..bytes.len()).step_by(7) {
            let mut spare = MAX_RECORDS_TOTAL;
            if let Ok(part) = read_records_from(Cursor::new(&bytes[..cut]), cut as u64, "Mod.esp", &names, &mut NameIds::default(), &mut spare) {
                prop_assert!(part.keys.len() <= written.len());
            }
        }
    }

    #[test]
    fn arbitrary_bytes_are_an_error_or_a_consistent_result(bytes in proptest::collection::vec(any::<u8>(), 0..600)) {
        let mut budget = MAX_RECORDS_TOTAL;
        if let Ok(result) = read_records_from(Cursor::new(&bytes), bytes.len() as u64, "a.esp", &masters(2), &mut NameIds::default(), &mut budget) {
            prop_assert!(result.overridden <= result.keys.len());
            prop_assert_eq!(budget, MAX_RECORDS_TOTAL - result.keys.len());
        } else {
            prop_assert_eq!(budget, MAX_RECORDS_TOTAL);
        }
    }
}
