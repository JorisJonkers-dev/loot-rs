//! Property tests of the minimal-change sort (`loot_rs::sort::sort_minimal`) on synthetic worlds: the result
//! satisfies every hard constraint that the full sort (the libloot port) satisfies, a valid order is returned
//! unchanged, and the number of moves is the smallest possible (checked by brute force on small worlds).

mod common;

use loot_rs::sort::{
    EdgeType, GraphError, GroupInput, NoOverlaps, PluginInput, Repaired, sort as sort_full,
    sort_minimal,
};
use proptest::prelude::*;

#[derive(Debug, Clone)]
struct World {
    n: usize,
    is_master: Vec<bool>,
    group: Vec<usize>,
    /// `(plugin, other)`: the plugin loads after the other.
    masters: Vec<(usize, usize)>,
    requirements: Vec<(usize, usize)>,
    load_after: Vec<(usize, usize)>,
    given: Vec<usize>,
}

fn world() -> impl Strategy<Value = World> {
    (2usize..9).prop_flat_map(|n| {
        let pairs = proptest::collection::vec((0..n, 0..n), 0..12);
        (
            Just(n),
            proptest::collection::vec(any::<bool>(), n),
            proptest::collection::vec(0usize..4, n),
            (pairs.clone(), pairs.clone(), pairs),
            Just((0..n).collect::<Vec<usize>>()).prop_shuffle(),
        )
            .prop_map(
                |(n, mut is_master, group, (masters, requirements, load_after), given)| {
                    // `p0.esm` is the plugin the game loads first, so it is a master.
                    is_master[0] = true;
                    // Rules point from a higher id to a lower one, so they never form a cycle, and never ask a
                    // plugin without the master flag to load before one with it.
                    let keep = |pairs: Vec<(usize, usize)>| -> Vec<(usize, usize)> {
                        pairs
                            .into_iter()
                            .filter(|&(plugin, other)| {
                                other < plugin && (!is_master[plugin] || is_master[other])
                            })
                            .collect()
                    };
                    World {
                        n,
                        group,
                        masters: keep(masters),
                        requirements: keep(requirements),
                        load_after: keep(load_after),
                        is_master,
                        given,
                    }
                },
            )
    })
}

fn groups() -> Vec<GroupInput> {
    let group = |name: &str, after: &[&str]| GroupInput {
        name: name.to_string(),
        after: after.iter().map(ToString::to_string).collect(),
    };
    vec![
        group("A", &[]),
        group("B", &["A"]),
        group("C", &["A", "B"]),
        group("default", &["C"]),
    ]
}

fn name(world: &World, id: usize) -> String {
    format!("p{id}.{}", if world.is_master[id] { "esm" } else { "esp" })
}

fn inputs(world: &World, ids: &[usize]) -> Vec<PluginInput> {
    let listed = |pairs: &[(usize, usize)], id: usize| -> Vec<String> {
        pairs
            .iter()
            .filter(|&&(plugin, _)| plugin == id)
            .map(|&(_, other)| name(world, other))
            .collect()
    };
    ids.iter()
        .map(|&id| PluginInput {
            name: name(world, id),
            is_master: world.is_master[id],
            masters: listed(&world.masters, id),
            requirements: listed(&world.requirements, id),
            load_after: listed(&world.load_after, id),
            group: ["default", "A", "B", "C"][world.group[id]].to_string(),
            override_records: 0,
            assets: 0,
            ..PluginInput::default()
        })
        .collect()
}

fn early(world: &World) -> Vec<String> {
    vec![name(world, 0)]
}

/// Both sorts of `ids` given in that order: the repaired order and the full one, as ids, with the edges of the
/// full sort as `(before, after, kind)` over ids.
struct Both {
    minimal: Repaired,
    full_order: Vec<usize>,
    full_edges: Vec<(usize, usize, EdgeType)>,
}

fn both(world: &World, ids: &[usize], with_groups: bool) -> Result<Both, GraphError> {
    let plugins = inputs(world, ids);
    let full = sort_full(&plugins, &groups(), &early(world), &NoOverlaps)?;
    let minimal = sort_minimal(&plugins, &groups(), &early(world), &NoOverlaps, with_groups)?;
    Ok(Both {
        minimal,
        full_order: full.order.iter().map(|&index| ids[index]).collect(),
        full_edges: full
            .edges
            .iter()
            .map(|edge| (ids[edge.from], ids[edge.to], edge.kind))
            .collect(),
    })
}

/// The hard constraints over ids, from the world alone.
fn hard(world: &World) -> Vec<(usize, usize)> {
    let mut rules: Vec<(usize, usize)> = world
        .masters
        .iter()
        .chain(&world.requirements)
        .chain(&world.load_after)
        .map(|&(plugin, other)| (other, plugin))
        .collect();
    for a in 0..world.n {
        for b in 0..world.n {
            if world.is_master[a] && !world.is_master[b] {
                rules.push((a, b));
            }
        }
    }
    if world.is_master[0] {
        rules.extend((1..world.n).filter(|&b| world.is_master[b]).map(|b| (0, b)));
    }
    rules
}

fn ids_of(world: &World, repaired: &Repaired) -> Vec<usize> {
    repaired
        .order
        .iter()
        .map(|&index| world.given[index])
        .collect()
}

fn position(order: &[usize], id: usize) -> usize {
    order.iter().position(|&x| x == id).expect("present")
}

proptest! {
    #![proptest_config(common::proptest_config(400))]

    #[test]
    fn the_result_is_a_permutation_that_satisfies_every_hard_constraint(world in world(), with_groups in any::<bool>()) {
        let both = both(&world, &world.given, with_groups).expect("acyclic by construction");
        let order = ids_of(&world, &both.minimal);
        let mut seen = order.clone();
        seen.sort_unstable();
        prop_assert_eq!(seen, (0..world.n).collect::<Vec<_>>());
        for (before, after) in hard(&world) {
            prop_assert!(position(&order, before) < position(&order, after), "{before} before {after}");
        }
    }

    #[test]
    fn every_constraint_the_full_sort_satisfies_holds_in_the_result(world in world(), with_groups in any::<bool>()) {
        let both = both(&world, &world.given, with_groups).expect("acyclic by construction");
        let order = ids_of(&world, &both.minimal);
        // The full sort satisfies all of its edges; the hard kinds (and the group edges when asked) must hold here.
        for &(before, after, kind) in &both.full_edges {
            let hard_kind = matches!(
                kind,
                EdgeType::Hardcoded | EdgeType::Master | EdgeType::MasterlistRequirement | EdgeType::MasterlistLoadAfter
            ) || (with_groups && kind == EdgeType::MasterlistGroup);
            if hard_kind {
                prop_assert!(position(&both.full_order, before) < position(&both.full_order, after));
                prop_assert!(position(&order, before) < position(&order, after), "{kind:?} {before} -> {after}");
            }
        }
    }

    #[test]
    fn an_order_that_violates_nothing_is_returned_unchanged(world in world(), with_groups in any::<bool>()) {
        let first = both(&world, &world.given, with_groups).expect("acyclic by construction");
        // The full sort's order violates no constraint.
        let from_full = World { given: first.full_order.clone(), ..world.clone() };
        let again = both(&from_full, &from_full.given, with_groups).expect("same world");
        prop_assert_eq!(again.minimal.moves.len(), 0);
        prop_assert_eq!(again.minimal.order.clone(), (0..world.n).collect::<Vec<_>>());
        // So is the repaired order, repaired again.
        let repaired = World { given: ids_of(&world, &first.minimal), ..world.clone() };
        let twice = both(&repaired, &repaired.given, with_groups).expect("same world");
        prop_assert_eq!(twice.minimal.moves.len(), 0);
        prop_assert_eq!(twice.minimal.order.clone(), (0..world.n).collect::<Vec<_>>());
    }

    #[test]
    fn the_number_of_moves_is_the_smallest_possible(world in world(), with_groups in any::<bool>()) {
        let both = both(&world, &world.given, with_groups).expect("acyclic by construction");
        prop_assert!(both.minimal.exact, "small worlds are searched exhaustively");
        // Required relation over ids: the world's, the group edges of the full sort when asked, closed.
        let n = world.n;
        let mut required = vec![vec![false; n]; n];
        for (a, b) in hard(&world) {
            required[a][b] = true;
        }
        if with_groups {
            for &(a, b, kind) in &both.full_edges {
                if kind == EdgeType::MasterlistGroup {
                    required[a][b] = true;
                }
            }
        }
        for k in 0..n {
            for a in 0..n {
                for b in 0..n {
                    if required[a][k] && required[k][b] {
                        required[a][b] = true;
                    }
                }
            }
        }
        let at = |id: usize| position(&world.given, id);
        let mut most = 0;
        for mask in 0u32..(1 << n) {
            let kept: Vec<usize> = (0..n).filter(|&id| mask >> id & 1 == 1).collect();
            let fine = kept.iter().all(|&a| kept.iter().all(|&b| !(required[a][b] && at(a) > at(b))));
            if fine {
                most = most.max(kept.len());
            }
        }
        prop_assert_eq!(both.minimal.moves.len(), n - most);
    }

    #[test]
    fn no_more_plugins_move_than_the_full_sort_displaces(world in world(), with_groups in any::<bool>()) {
        let both = both(&world, &world.given, with_groups).expect("acyclic by construction");
        // The plugins whose relative order is the same in the given and the full order violate nothing among
        // themselves, so they can all stay: the moves are at most the rest.
        let n = world.n;
        let mut table = vec![vec![0usize; n + 1]; n + 1];
        for i in (0..n).rev() {
            for j in (0..n).rev() {
                table[i][j] = if world.given[i] == both.full_order[j] {
                    table[i + 1][j + 1] + 1
                } else {
                    table[i + 1][j].max(table[i][j + 1])
                };
            }
        }
        prop_assert!(both.minimal.moves.len() <= n - table[0][0]);
    }

    #[test]
    fn each_move_names_a_constraint_that_the_given_order_violated(world in world()) {
        let both = both(&world, &world.given, false).expect("acyclic by construction");
        for moved in &both.minimal.moves {
            prop_assert_eq!(moved.from, moved.plugin);
            let (a, b) = (world.given[moved.plugin], world.given[moved.other]);
            let violated = hard(&world).iter().any(|&(before, after)| {
                (before, after) == (a, b) || (before, after) == (b, a)
            }) || moved.indirect;
            prop_assert!(violated, "{moved:?}");
        }
    }
}
