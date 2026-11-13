//! Unit tests of `sort`: the ordering algorithm on indices.

use super::*;

fn sorted(n: usize, masters: &[usize], hard: &[(usize, usize)], soft: &[(usize, usize)]) -> Sorted {
    let flags: Vec<bool> = (0..n).map(|index| masters.contains(&index)).collect();
    order(n, &flags, hard, soft).expect("sorts")
}

#[test]
fn no_rules_keep_the_order() {
    assert_eq!(sorted(5, &[], &[], &[]).order, [0, 1, 2, 3, 4]);
    assert_eq!(sorted(0, &[], &[], &[]).order, Vec::<usize>::new());
}

#[test]
fn masters_come_first_and_keep_their_relative_order() {
    assert_eq!(sorted(5, &[1, 3], &[], &[]).order, [1, 3, 0, 2, 4]);
}

#[test]
fn a_hard_edge_delays_the_plugin_that_has_to_wait_and_nothing_else() {
    // 0 has to load after 3: it waits until 3 is placed, the others keep their order.
    assert_eq!(sorted(5, &[], &[(3, 0)], &[]).order, [1, 2, 3, 0, 4]);
}

#[test]
fn edges_are_transitive() {
    assert_eq!(sorted(4, &[], &[(3, 2), (2, 1)], &[]).order, [0, 3, 2, 1]);
}

#[test]
fn a_hard_cycle_is_an_error_naming_the_closing_edge() {
    assert_eq!(
        order(3, &[false; 3], &[(0, 1), (1, 2), (2, 0)], &[]),
        Err(GraphError::Cycle(2, 0))
    );
    assert_eq!(
        order(2, &[false; 2], &[(1, 1)], &[]),
        Err(GraphError::Cycle(1, 1))
    );
}

#[test]
fn a_soft_edge_that_closes_a_cycle_is_dropped_and_reported() {
    let result = sorted(3, &[], &[(0, 1), (1, 2)], &[(2, 0), (0, 2)]);
    assert_eq!(result.order, [0, 1, 2]);
    assert_eq!(result.dropped, [(2, 0)]);
}

#[test]
fn hard_edges_win_over_soft_ones_whatever_the_order() {
    let result = sorted(2, &[], &[(1, 0)], &[(0, 1)]);
    assert_eq!(result.order, [1, 0]);
    assert_eq!(result.dropped, [(0, 1)]);
}

#[test]
fn a_non_master_before_a_master_is_an_error_when_hard_and_dropped_when_soft() {
    assert_eq!(
        order(2, &[false, true], &[(0, 1)], &[]),
        Err(GraphError::MasterOrder(0, 1))
    );
    let result = sorted(2, &[1], &[], &[(0, 1)]);
    assert_eq!(result.order, [1, 0]);
    assert_eq!(result.dropped, [(0, 1)]);
}

#[test]
fn a_master_after_a_master_edge_is_honoured() {
    assert_eq!(sorted(3, &[0, 1, 2], &[(2, 0)], &[]).order, [1, 2, 0]);
}

#[test]
fn already_ordered_input_is_a_fixed_point() {
    let first = sorted(6, &[2, 4], &[(5, 0), (4, 2)], &[(1, 3)]);
    let flags: Vec<bool> = first.order.iter().map(|&at| at == 2 || at == 4).collect();
    // Renumber the edges to the new positions and sort again.
    let position = |old: usize| {
        first
            .order
            .iter()
            .position(|&at| at == old)
            .expect("present")
    };
    let hard: Vec<_> = [(5, 0), (4, 2)]
        .iter()
        .map(|&(a, b)| (position(a), position(b)))
        .collect();
    let soft = [(position(1), position(3))];
    let second = order(6, &flags, &hard, &soft).expect("sorts");
    assert_eq!(second.order, (0..6).collect::<Vec<_>>());
}

#[test]
fn bad_input_is_an_error() {
    assert_eq!(
        order(2, &[false; 2], &[(0, 5)], &[]),
        Err(GraphError::BadIndex(5))
    );
    assert_eq!(
        order(2, &[false; 3], &[], &[]),
        Err(GraphError::BadIndex(3))
    );
    assert_eq!(
        order(MAX_PLUGINS + 1, &vec![false; MAX_PLUGINS + 1], &[], &[]),
        Err(GraphError::TooMany(MAX_PLUGINS + 1))
    );
}

#[test]
fn the_largest_load_order_with_many_rules_is_sorted_within_the_budget() {
    // 1024 plugins in 32 groups, every plugin after every plugin of the previous group: a million soft edges.
    let n = MAX_PLUGINS;
    let group = |index: usize| index * 32 / n;
    let soft: Vec<(usize, usize)> = (0..n)
        .flat_map(|a| {
            (0..n)
                .filter(move |&b| group(b) == group(a) + 1)
                .map(move |b| (a, b))
        })
        .collect();
    let started = std::time::Instant::now();
    let result = order(n, &vec![false; n], &[], &soft).expect("within the budget");
    assert_eq!(result.order.len(), n);
    assert_eq!(result.dropped, []);
    assert!(started.elapsed() < std::time::Duration::from_secs(10));
}

#[test]
fn errors_read_as_sentences() {
    for error in [
        GraphError::TooMany(2000),
        GraphError::Cycle(1, 2),
        GraphError::MasterOrder(1, 2),
        GraphError::BadIndex(9),
        GraphError::TooMuchWork,
    ] {
        assert!(error.to_string().len() > 5);
    }
}
