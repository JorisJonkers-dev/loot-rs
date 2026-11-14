//! The bounds of the sorter on the largest input it accepts: each case below has 1024 plugins and must finish
//! quickly, in little memory, either with an order or with an error. See `docs/RESOURCE-LIMITS.md`.

use loot_rs::sort::{GraphError, GroupInput, MAX_PLUGINS, Overlaps, PluginInput, sort};
use std::time::{Duration, Instant};

fn status_kib(field: &str) -> usize {
    let status = std::fs::read_to_string("/proc/self/status").expect("/proc/self/status");
    let line = status
        .lines()
        .find(|l| l.starts_with(field))
        .expect("field");
    line.split_whitespace()
        .nth(1)
        .and_then(|n| n.parse::<usize>().ok())
        .expect("number")
        * 1024
}

fn measure<T>(work: impl FnOnce() -> T) -> (T, usize, Duration) {
    std::fs::write("/proc/self/clear_refs", "5").expect("reset the peak resident size");
    let start = status_kib("VmRSS:");
    let began = Instant::now();
    let result = work();
    (
        result,
        status_kib("VmHWM:").saturating_sub(start),
        began.elapsed(),
    )
}

struct All;

impl Overlaps for All {
    fn records(&self, _: usize, _: usize) -> bool {
        true
    }
    fn assets(&self, _: usize, _: usize) -> bool {
        true
    }
}

fn plugins(count: usize) -> Vec<PluginInput> {
    (0..count)
        .map(|index| PluginInput::new(&format!("{index:05}.esp")))
        .collect()
}

#[test]
fn the_largest_load_order_is_sorted_or_refused_quickly_and_in_little_memory() {
    let groups: Vec<GroupInput> = (0..40)
        .map(|index| GroupInput {
            name: format!("g{index:02}"),
            after: if index == 0 {
                vec![]
            } else {
                vec![format!("g{:02}", index - 1), format!("g{:02}", index / 2)]
            },
        })
        .collect();
    let cases: Vec<(&str, Vec<PluginInput>)> = vec![
        ("no metadata", plugins(MAX_PLUGINS)),
        ("every plugin changes a different number of records", {
            let mut list = plugins(MAX_PLUGINS);
            for (index, plugin) in list.iter_mut().enumerate() {
                plugin.override_records = MAX_PLUGINS - index;
                plugin.assets = index % 7;
            }
            list
        }),
        ("a long chain of load-after rules, listed backwards", {
            let mut list = plugins(MAX_PLUGINS);
            for (index, plugin) in list.iter_mut().enumerate().skip(1) {
                plugin.load_after = vec![format!("{:05}.esp", index - 1)];
            }
            list.reverse();
            list
        }),
        ("many groups, every plugin in one", {
            let mut list = plugins(MAX_PLUGINS);
            for (index, plugin) in list.iter_mut().enumerate() {
                plugin.group = format!("g{:02}", (index * 7) % 40);
            }
            list
        }),
    ];
    for (name, list) in cases {
        let (result, peak, elapsed) = measure(|| sort(&list, &groups, &[], &All));
        println!(
            "{name}: peak {} KiB in {elapsed:?}: {:?}",
            peak / 1024,
            result.as_ref().map(|s| s.order.len())
        );
        assert!(
            matches!(result, Ok(_) | Err(GraphError::TooMuchWork)),
            "{name}: {result:?}"
        );
        assert!(peak < 64 * 1024 * 1024, "{name}: peak {peak} bytes");
        assert!(
            elapsed < Duration::from_secs(120),
            "{name}: took {elapsed:?}"
        );
    }
}
