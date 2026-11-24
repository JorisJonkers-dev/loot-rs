use super::{Repaired, repair};
use crate::sort::{EdgeType, SortedEdge};
use crate::tally::{covers, ids};

fn edge(from: usize, to: usize, kind: EdgeType) -> SortedEdge {
    SortedEdge { from, to, kind }
}

fn run(is_master: &[bool], constraints: &[SortedEdge]) -> Repaired {
    repair(is_master, constraints).expect("consistent constraints")
}

#[test]
fn an_order_without_violations_is_returned_unchanged() {
    let result = run(
        &[true, true, false, false],
        &[
            edge(0, 1, EdgeType::Master),
            edge(2, 3, EdgeType::MasterlistLoadAfter),
        ],
    );
    assert_eq!(result.order, vec![0, 1, 2, 3]);
    assert_eq!(result.moves.len(), 0);
    assert_eq!(result.violations, 0);
    assert!(result.exact);
}

#[test]
fn one_plugin_that_loads_too_late_is_moved_alone_to_the_place_the_rule_needs() {
    covers(&[ids::groups::behaviour::MINIMAL_CHANGE]);
    // Plugin 5 has to load before plugin 1; the other four keep their places.
    let result = run(&[false; 6], &[edge(5, 1, EdgeType::MasterlistLoadAfter)]);
    assert_eq!(result.order, vec![0, 5, 1, 2, 3, 4]);
    assert_eq!(result.moves.len(), 1);
    let moved = result.moves[0];
    assert_eq!((moved.plugin, moved.from, moved.to), (5, 5, 1));
    assert_eq!(moved.class, EdgeType::MasterlistLoadAfter);
    assert!(!moved.indirect);
}

#[test]
fn the_fewest_plugins_move_when_one_move_fixes_many_violations() {
    // Plugin 0 has to load after 1, 2 and 3: moving 0 (one move) beats moving three plugins.
    let result = run(
        &[false; 4],
        &[
            edge(1, 0, EdgeType::Master),
            edge(2, 0, EdgeType::Master),
            edge(3, 0, EdgeType::Master),
        ],
    );
    assert_eq!(result.moves.len(), 1);
    assert_eq!(result.moves[0].plugin, 0);
    assert_eq!(result.order, vec![1, 2, 3, 0]);
    assert_eq!(result.violations, 3);
    assert!(result.exact);
}

#[test]
fn a_master_flag_violation_moves_the_plugin_without_the_flag() {
    // Order: esp, esm, esm; the esp has to come after both esms (two violations, one move).
    let result = run(&[false, true, true], &[]);
    assert_eq!(result.order, vec![1, 2, 0]);
    assert_eq!(result.moves.len(), 1);
    assert_eq!(result.moves[0].class, EdgeType::MasterFlag);
}

#[test]
fn a_reversed_chain_is_sorted_and_each_move_names_a_direct_constraint() {
    // 2 -> 1 -> 0 is required; the order given is the reverse. Two plugins move; the one that stays is the one the search keeps.
    let result = run(
        &[false; 3],
        &[
            edge(2, 1, EdgeType::Master),
            edge(1, 0, EdgeType::MasterlistRequirement),
        ],
    );
    assert_eq!(result.order, vec![2, 1, 0]);
    assert_eq!(result.moves.len(), 2);
    assert!(result.moves.iter().all(|moved| !moved.indirect));
    let moved: Vec<usize> = result.moves.iter().map(|moved| moved.plugin).collect();
    assert_eq!(moved, vec![1, 2]);
    // Each reports its first constraint in the order of the classes, a direct one before an indirect one.
    assert!(
        result
            .moves
            .iter()
            .all(|moved| moved.class == EdgeType::Master)
    );
}

#[test]
fn a_chain_with_a_plugin_between_keeps_the_two_ends_apart_correctly() {
    // 0 before 2 through 1; 1 sits first. Keeping 0 and 2 forces 1 between them.
    let result = run(
        &[false; 3],
        &[edge(0, 1, EdgeType::Master), edge(1, 2, EdgeType::Master)],
    );
    assert_eq!(result.order, vec![0, 1, 2]);
    assert_eq!(result.moves.len(), 0);
    let result = run(
        &[false; 4],
        &[edge(0, 3, EdgeType::Master), edge(3, 2, EdgeType::Master)],
    );
    let position = |p: usize| result.order.iter().position(|&x| x == p).unwrap();
    assert!(position(0) < position(3) && position(3) < position(2));
}

#[test]
fn a_cycle_is_an_internal_error() {
    let cycle = [edge(0, 1, EdgeType::Master), edge(1, 0, EdgeType::Master)];
    assert!(repair(&[false, false], &cycle).is_err());
}

#[test]
fn a_constraint_that_names_no_plugin_is_an_internal_error() {
    assert!(repair(&[false], &[edge(0, 3, EdgeType::Master)]).is_err());
}

#[test]
fn the_result_is_the_same_each_time() {
    let constraints = [
        edge(4, 0, EdgeType::Master),
        edge(5, 1, EdgeType::MasterlistRequirement),
        edge(3, 2, EdgeType::MasterlistLoadAfter),
    ];
    let first = run(&[false; 6], &constraints);
    assert_eq!(first, run(&[false; 6], &constraints));
}
