//! The bounds of the sorter on the largest input it accepts: each case below has 1024 plugins and must finish
//! quickly, in little memory, either with an order or with an error. See `docs/RESOURCE-LIMITS.md`.

use loot_rs::records::{MAX_RECORDS_PER_PLUGIN, NameIds, RecordsError, read_records_from};
use loot_rs::sort::{GraphError, GroupInput, MAX_PLUGINS, Overlaps, PluginInput, sort};
use std::io::{self, Read, Seek, SeekFrom};
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
    a_plugin_with_endless_records_is_refused_at_the_limit_in_little_memory();
}

/// A plugin that never ends: a header, one group, then records of 24 bytes without data, for ever.
struct Endless(u64);

impl Endless {
    fn byte(position: u64) -> u8 {
        let within = |offset: u64, bytes: &[u8]| {
            bytes
                .get(usize::try_from(offset).unwrap_or(usize::MAX))
                .copied()
        };
        match position {
            0..=23 => within(position, b"TES4\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\x0f\0\0\0"),
            24..=47 => within(position - 24, b"GRUP\0\0\0\0NPC_\0\0\0\0\0\0\0\0\0\0\0\0"),
            _ => {
                let (index, offset) = ((position - 48) / 24, (position - 48) % 24);
                match offset {
                    0..=3 => within(offset, b"NPC_"),
                    12..=15 => Some(
                        u32::try_from(index + 0x800)
                            .unwrap_or(u32::MAX)
                            .to_le_bytes()[usize::try_from(offset - 12).unwrap_or(0)],
                    ),
                    _ => Some(0),
                }
            }
        }
        .unwrap_or(0)
    }
}

impl Read for Endless {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        for byte in buffer.iter_mut() {
            *byte = Self::byte(self.0);
            self.0 += 1;
        }
        Ok(buffer.len())
    }
}

impl Seek for Endless {
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        if let SeekFrom::Current(offset) = from {
            self.0 = self.0.saturating_add_signed(offset);
        }
        Ok(self.0)
    }
}

/// Called from the test above: the measurements use the whole process, so they must not run side by side.
fn a_plugin_with_endless_records_is_refused_at_the_limit_in_little_memory() {
    let masters: Vec<String> = Vec::new();
    let (result, peak, elapsed) = measure(|| {
        read_records_from(
            Endless(0),
            u64::MAX / 2,
            "Endless.esp",
            &masters,
            &mut NameIds::default(),
            &mut usize::MAX.clone(),
        )
    });
    println!("endless records: peak {} KiB in {elapsed:?}", peak / 1024);
    assert!(matches!(result, Err(RecordsError::TooMany)), "{result:?}");
    // The keys of the records that were kept: eight bytes each, plus what the vector grew by.
    assert!(peak < MAX_RECORDS_PER_PLUGIN * 8 * 3, "peak {peak} bytes");
    assert!(elapsed < Duration::from_secs(120), "took {elapsed:?}");
}
