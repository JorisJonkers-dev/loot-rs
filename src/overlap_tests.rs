//! Unit tests of `overlap`: sorted id lists and which plugins share an id.

use super::*;

#[test]
fn plugins_share_an_id_when_both_lists_hold_it() {
    let sets = KeySets::new(vec![vec![1, 5, 9], vec![2, 5], vec![3, 4], Vec::new()]);
    assert!(sets.share(0, 1) && sets.share(1, 0));
    assert!(!sets.share(0, 2));
    assert!(!sets.share(0, 3) && !sets.share(3, 3));
    assert!(
        !sets.share(0, 7),
        "an index that is no plugin shares nothing"
    );
    assert!(sets.work() > 0);
}

#[test]
fn a_much_shorter_list_is_searched_in_the_longer_one_and_equal_lists_are_merged() {
    let large: Vec<u64> = (0..1000).map(|id| id * 2).collect();
    let sets = KeySets::new(vec![
        large.clone(),
        vec![500],
        vec![501],
        vec![998, 5000],
        large,
    ]);
    assert!(sets.share(0, 1));
    assert!(
        !sets.share(0, 2),
        "odd ids are not in the list of even ones"
    );
    assert!(sets.share(0, 3));
    assert!(sets.share(0, 4), "merged: equal lengths");
    // Lists whose ranges do not meet are told apart without a search.
    let apart = KeySets::new(vec![vec![1, 2], vec![10, 11], vec![0], vec![20, 30]]);
    assert!(!apart.share(0, 1) && !apart.share(1, 0) && !apart.share(2, 3));
    assert_eq!(apart.work(), 0);
    // A gap inside the range that the range check does not rule out.
    assert!(!KeySets::new(vec![vec![1, 9], vec![2, 3, 4]]).share(0, 1));
    assert!(!KeySets::default().share(0, 0));
}

#[test]
fn records_and_assets_are_answered_apart_and_the_work_is_added() {
    let overlaps = PluginOverlaps::new(
        vec![vec![1, 2], vec![2, 3], vec![9]],
        vec![vec![7], vec![8], vec![8]],
    );
    assert!(overlaps.records(0, 1) && !overlaps.records(0, 2));
    assert!(overlaps.assets(1, 2) && !overlaps.assets(0, 1));
    assert!(overlaps.work() > 0);
    assert!(!PluginOverlaps::default().records(0, 0) && !PluginOverlaps::default().assets(0, 0));
}

#[test]
fn precomputed_answers_are_the_answers_of_comparing_one_pair_at_a_time() {
    // Small deterministic sets with some overlaps, different counts, and both partitions.
    let records: Vec<Vec<u64>> = (0..12u64)
        .map(|i| {
            (0..i % 5)
                .map(|k| (i * 3 + k * 7) % 17)
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .collect()
        })
        .collect();
    let assets: Vec<Vec<u64>> = (0..12u64)
        .map(|i| {
            (0..(i % 4))
                .map(|k| (i + k * 5) % 9)
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .collect()
        })
        .collect();
    let is_master: Vec<bool> = (0..12).map(|i| i % 3 == 0).collect();
    let override_records: Vec<usize> = (0..12).map(|i| i % 4).collect();
    let asset_counts: Vec<usize> = assets.iter().map(Vec::len).collect();
    let fresh = PluginOverlaps::new(records.clone(), assets.clone());
    let mut table = PluginOverlaps::new(records, assets);
    let needs = PairNeeds {
        is_master: &is_master,
        override_records: &override_records,
        assets: &asset_counts,
    };
    table
        .precompute(needs, 4, &|| {})
        .expect("within the budget");
    for a in 0..12 {
        for b in 0..12 {
            assert_eq!(table.records(a, b), fresh.records(a, b), "records {a} {b}");
            assert_eq!(table.assets(a, b), fresh.assets(a, b), "assets {a} {b}");
        }
    }
}

#[test]
fn intersect_counts_the_same_steps_in_either_order() {
    let a = [1, 4, 9, 12];
    let b = [2, 4, 7];
    assert_eq!(intersect(&a, &b), intersect(&b, &a));
    assert!(intersect(&a, &b).0);
}
