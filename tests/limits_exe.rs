//! The bounds of the executable version reader (cargo feature `executable-versions`): each hostile image must
//! be refused or answered quickly with little memory, under the same address-space limit as `limits.rs`
//! (`task limits`). One test function, so nothing else allocates while it measures. See
//! `docs/RESOURCE-LIMITS.md`.
#![cfg(feature = "executable-versions")]
// The peak resident size is read from /proc (Linux only; the crate forbids `unsafe`, so no counting allocator).
#![cfg(target_os = "linux")]

mod common;

use common::Fixture;
use loot_rs::condition::{ConditionError, eval_str};
use loot_rs::env::FsEnv;
use loot_rs::exe_version::{PeSpec, version_block_bytes};
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

#[test]
fn hostile_executables_are_refused_quickly_and_with_little_memory() {
    let fixture = Fixture::new();
    let mut huge_directories = PeSpec::new([1, 0, 0, 0], "1");
    huge_directories.directories = u32::MAX;
    let mut huge_directory = PeSpec::new([1, 0, 0, 0], "1");
    huge_directory.directory_size = Some(u32::MAX);
    let mut huge_block = PeSpec::new([1, 0, 0, 0], "1");
    huge_block.leaf_size = Some(u32::MAX);
    // A string whose length is 0 and whose key is not ProductVersion: the interpreter alone would never stop.
    let mut endless = PeSpec::new([1, 0, 0, 0], "1");
    endless.block = version_block_bytes([1, 0, 0, 0], "1");
    let string = endless.block.len() - 40;
    endless.block[string + 6] = b'X';
    endless.block[string..string + 2].copy_from_slice(&0u16.to_le_bytes());
    for (name, spec) in [
        ("directories", &huge_directories),
        ("directory", &huge_directory),
        ("block", &huge_block),
        ("endless", &endless),
    ] {
        std::fs::write(fixture.data().join(format!("{name}.dll")), spec.build()).expect("write");
    }
    let env = FsEnv::new(&fixture.data(), Vec::new());
    for name in ["directories", "directory", "block"] {
        for function in ["version", "product_version"] {
            let text = format!(r#"{function}("{name}.dll", "1", ==)"#);
            let (result, peak, time) = measure(|| eval_str(&text, &env));
            println!("{text}: peak {} KiB in {time:?}", peak / 1024);
            assert!(
                matches!(result, Err(ConditionError::Environment(_))),
                "{text}: {result:?}"
            );
            assert!(
                peak < 24 * 1024 * 1024 && time < Duration::from_secs(3),
                "{text}"
            );
        }
    }
    let (result, peak, time) =
        measure(|| eval_str(r#"product_version("endless.dll", "1", ==)"#, &env));
    assert!(
        matches!(result, Err(ConditionError::Environment(_))),
        "{result:?}"
    );
    assert!(peak < 24 * 1024 * 1024 && time < Duration::from_secs(3));
}
