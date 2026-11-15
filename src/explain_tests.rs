use super::*;

fn edge(from: usize, to: usize, kind: EdgeType) -> SortedEdge {
    SortedEdge { from, to, kind }
}

#[test]
fn a_plugin_pulled_before_an_earlier_one_by_its_own_edge_is_explained_by_that_edge() {
    // Given A B C; C must load before A (`after` on A): result C A B.
    let edges = [
        edge(2, 0, EdgeType::MasterlistLoadAfter),
        edge(0, 1, EdgeType::TieBreak),
    ];
    let moved = explain(&[2, 0, 1], &edges, &[false; 3]);
    let c = moved.iter().find(|m| m.plugin == 2).expect("C moved");
    assert_eq!(
        c.reasons,
        [Reason {
            cause: Cause::Edge(EdgeType::MasterlistLoadAfter),
            other: 0,
            later: false,
            via: None
        }]
    );
    let a = moved.iter().find(|m| m.plugin == 0).expect("A moved");
    assert_eq!(a.reasons[0].other, 2);
    assert!(a.reasons[0].later);
}

#[test]
fn a_plugin_pushed_along_by_others_is_explained_by_a_path_through_the_edge_that_did_it() {
    // B has no edge against the given order; C crossed it through C -> A -> B.
    let edges = [
        edge(2, 0, EdgeType::MasterlistLoadAfter),
        edge(0, 1, EdgeType::TieBreak),
    ];
    let moved = explain(&[2, 0, 1], &edges, &[false; 3]);
    let b = moved.iter().find(|m| m.plugin == 1).expect("B moved");
    assert_eq!(
        b.reasons,
        [Reason {
            cause: Cause::Edge(EdgeType::MasterlistLoadAfter),
            other: 2,
            later: true,
            via: Some((2, 0))
        }]
    );
}

#[test]
fn a_master_that_moved_before_a_plugin_is_explained_by_the_master_flag() {
    let moved = explain(&[1, 0], &[], &[false, true]);
    assert_eq!(moved.len(), 2);
    assert_eq!(moved[0].reasons[0].cause, Cause::MasterFlag);
    assert_eq!(
        moved[1].reasons[0],
        Reason {
            cause: Cause::MasterFlag,
            other: 0,
            later: false,
            via: None
        }
    );
}

#[test]
fn plugins_that_kept_their_place_are_not_listed() {
    let edges = [
        edge(0, 1, EdgeType::TieBreak),
        edge(1, 2, EdgeType::TieBreak),
    ];
    assert_eq!(explain(&[0, 1, 2], &edges, &[false; 3]), Vec::new());
}
