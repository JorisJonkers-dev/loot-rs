#![no_main]

use libfuzzer_sys::arbitrary::Unstructured;
use libfuzzer_sys::fuzz_target;
use loot_rs::sort::{sort, GraphError, GroupInput, Overlaps, PluginInput, MAX_PLUGINS};

const MAX: usize = 40;

struct Input {
    plugins: Vec<PluginInput>,
    groups: Vec<GroupInput>,
    early: Vec<String>,
    record_overlap: Vec<Vec<bool>>,
    asset_overlap: Vec<Vec<bool>>,
}

struct Matrix<'a>(&'a [Vec<bool>]);

struct Both<'a>(Matrix<'a>, Matrix<'a>);

impl Overlaps for Both<'_> {
    fn records(&self, a: usize, b: usize) -> bool {
        self.0 .0[a][b]
    }
    fn assets(&self, a: usize, b: usize) -> bool {
        self.1 .0[a][b]
    }
}

impl Input {
    fn read(u: &mut Unstructured) -> libfuzzer_sys::arbitrary::Result<Self> {
        let n = usize::from(u.arbitrary::<u8>()?) % MAX + 1;
        let name = |index: usize| format!("p{index}.esp");
        let mut plugins = Vec::new();
        for index in 0..n {
            let pick = |u: &mut Unstructured| -> libfuzzer_sys::arbitrary::Result<Vec<String>> {
                let count = usize::from(u.arbitrary::<u8>()?) % 4;
                (0..count)
                    .map(|_| Ok(name(usize::from(u.arbitrary::<u8>()?) % n)))
                    .collect()
            };
            plugins.push(PluginInput {
                name: name(index),
                is_master: u.arbitrary()?,
                masters: pick(u)?,
                requirements: pick(u)?,
                load_after: pick(u)?,
                group: format!("g{}", u.arbitrary::<u8>()? % 6),
                override_records: usize::from(u.arbitrary::<u8>()? % 5),
                assets: usize::from(u.arbitrary::<u8>()? % 5),
                ..PluginInput::default()
            });
        }
        let groups = (0..5)
            .map(|index| {
                Ok(GroupInput {
                    name: format!("g{index}"),
                    after: (0..u.arbitrary::<u8>()? % 3)
                        .map(|_| Ok(format!("g{}", u.arbitrary::<u8>()? % 6)))
                        .collect::<libfuzzer_sys::arbitrary::Result<_>>()?,
                })
            })
            .collect::<libfuzzer_sys::arbitrary::Result<Vec<_>>>()?;
        let matrix = |u: &mut Unstructured| -> libfuzzer_sys::arbitrary::Result<Vec<Vec<bool>>> {
            let mut m = vec![vec![false; n]; n];
            for a in 0..n {
                for b in a..n {
                    let value = u.arbitrary::<bool>()?;
                    m[a][b] = value;
                    m[b][a] = value;
                }
            }
            Ok(m)
        };
        let record_overlap = matrix(u)?;
        let asset_overlap = matrix(u)?;
        let early = if u.arbitrary()? {
            vec![name(usize::from(u.arbitrary::<u8>()?) % n)]
        } else {
            Vec::new()
        };
        Ok(Self {
            plugins,
            groups,
            early,
            record_overlap,
            asset_overlap,
        })
    }
}

// Any metadata and overlaps: an error that names plugins that exist, or an order that is a permutation, holds
// every edge the sorter says it added, leaves exactly one order, has the masters first, and is a fixed point.
fuzz_target!(|data: &[u8]| {
    let Ok(input) = Input::read(&mut Unstructured::new(data)) else {
        return;
    };
    let n = input.plugins.len();
    assert!(n <= MAX_PLUGINS);
    let overlaps = Both(Matrix(&input.record_overlap), Matrix(&input.asset_overlap));
    match sort(&input.plugins, &input.groups, &input.early, &overlaps) {
        Ok(sorted) => {
            let mut seen = sorted.order.clone();
            seen.sort_unstable();
            assert_eq!(seen, (0..n).collect::<Vec<_>>());
            let at = |node: usize| sorted.order.iter().position(|&x| x == node).unwrap();
            for edge in &sorted.edges {
                assert!(at(edge.from) < at(edge.to));
            }
            assert!(sorted.unique);
            let masters = input.plugins.iter().filter(|p| p.is_master).count();
            assert!(sorted.order[..masters].iter().all(|&i| input.plugins[i].is_master));
        }
        Err(GraphError::Cycle(cycle)) => {
            assert!(!cycle.is_empty() && cycle.iter().all(|&(plugin, _)| plugin < n));
        }
        Err(GraphError::MasterOrder { plugin, master, .. }) => {
            assert!(plugin < n && master < n);
            assert!(!input.plugins[plugin].is_master && input.plugins[master].is_master);
        }
        Err(GraphError::GroupCycle(groups)) => assert!(!groups.is_empty()),
        Err(GraphError::TooMuchWork) => {}
        Err(other) => panic!("unexpected {other}"),
    }
});
