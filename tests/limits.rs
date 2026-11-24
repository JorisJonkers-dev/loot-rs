//! The hard bounds of the library, proven with a counting allocator: every hostile input below must fail fast
#![allow(clippy::format_push_string)]
// The peak resident size is read from /proc (Linux only; the crate forbids `unsafe`, so no counting allocator).
#![cfg(target_os = "linux")]
//! with an error while using little memory (the rise in peak resident size, from `/proc/self/status`). One test function, so nothing else allocates while it measures.
//! See `docs/RESOURCE-LIMITS.md`.

mod common;

use common::{covers, ids};

use common::Fixture;
use loot_rs::cli::{MAX_PLUGINS_FILE, main_with};
use loot_rs::condition::parse as parse_condition;
use loot_rs::metadata::{MAX_MASTERLIST_BYTES, parse as parse_masterlist};
use loot_rs::plugin::{parse_header, read_header};
use std::time::{Duration, Instant};

/// A field of `/proc/self/status` in bytes (Linux; the crate forbids `unsafe`, so no counting allocator).
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

/// Run `work`; return how far the process's peak resident memory rose above where it started, and how long
/// it took. The peak is reset first by writing 5 to `/proc/self/clear_refs`.
fn measure<T>(work: impl FnOnce() -> T) -> (T, usize, Duration) {
    std::fs::write("/proc/self/clear_refs", "5").expect("reset the peak resident size");
    let start = status_kib("VmRSS:");
    let began = Instant::now();
    let result = work();
    let elapsed = began.elapsed();
    (result, status_kib("VmHWM:").saturating_sub(start), elapsed)
}

const BUDGET_BYTES: usize = 24 * 1024 * 1024;
const BUDGET_TIME: Duration = Duration::from_secs(3);

fn within(name: &str, peak: usize, elapsed: Duration) {
    println!("{name}: peak {} KiB in {elapsed:?}", peak / 1024);
    assert!(peak < BUDGET_BYTES, "{name}: peak {peak} bytes");
    assert!(elapsed < BUDGET_TIME, "{name}: took {elapsed:?}");
}

fn laughs(levels: usize) -> String {
    use std::fmt::Write;
    let mut text = String::from("prelude:\n  - &a0 [x, x, x, x, x, x, x, x, x, x]\n");
    for level in 1..=levels {
        let previous = format!("*a{}", level - 1);
        let _ = writeln!(text, "  - &a{level} [{}]", vec![previous; 10].join(", "));
    }
    text + "globals: []\n"
}

#[test]
fn hostile_input_is_refused_quickly_and_with_little_memory() {
    covers(&[ids::tool::cli::RESOURCE_LIMITS]);
    // A billion-laughs document: ten levels of ten aliases.
    let text = laughs(12);
    let (result, peak, time) = measure(|| parse_masterlist(&text));
    assert!(
        result
            .expect_err("refused")
            .0
            .contains("expand to more than")
    );
    within("billion laughs", peak, time);

    // The same through merge keys, which clone maps when they expand.
    let mut merge = String::from("prelude:\n  - &m0 {a: 1, b: 2, c: 3}\n");
    for level in 1..=40 {
        merge.push_str(&format!(
            "  - &m{level} {{<<: [*m{0}, *m{0}, *m{0}, *m{0}], k{level}: 1}}\n",
            level - 1
        ));
    }
    let (result, peak, time) = measure(|| parse_masterlist(&merge));
    assert!(result.is_err());
    within("merge-key bomb", peak, time);

    // A million nested brackets.
    let deep = format!("a: {}", "[".repeat(1_000_000));
    let (result, peak, time) = measure(|| parse_masterlist(&deep));
    assert!(result.expect_err("refused").0.contains("nested"));
    within("deeply nested YAML", peak, time);

    // A masterlist over the size limit.
    let big = "#".repeat(MAX_MASTERLIST_BYTES + 1);
    let (result, peak, time) = measure(|| parse_masterlist(&big));
    assert!(
        result
            .expect_err("refused")
            .0
            .contains("more than the limit")
    );
    within("oversized masterlist", peak, time);

    // Deeply nested, very long and very chained conditions.
    for (name, text) in [
        (
            "nested parentheses",
            format!("{}file(\"a\"){}", "(".repeat(8_000), ")".repeat(8_000)),
        ),
        ("not chain", format!("{}file(\"a\")", "not ".repeat(4_000))),
        ("and chain", format!("{}f()", "f()and".repeat(2_700))),
        ("long text", "a".repeat(5_000_000)),
    ] {
        let (result, peak, time) = measure(|| parse_condition(&text));
        assert!(result.is_err(), "{name}");
        within(name, peak, time);
    }

    // A plugin header whose size field is huge: in bytes, and in a small file.
    let mut claim = b"TES4".to_vec();
    claim.extend_from_slice(&u32::MAX.to_le_bytes());
    claim.extend_from_slice(&[0; 16]);
    let (result, peak, time) = measure(|| parse_header(&claim));
    assert!(result.is_err());
    within("huge length field in bytes", peak, time);
    let mut four_mib = b"TES4".to_vec();
    four_mib.extend_from_slice(&(4u32 * 1024 * 1024).to_le_bytes());
    four_mib.extend_from_slice(&[0; 16]);
    let fixture = Fixture::new();
    fixture.file("claims-a-lot.esp", &four_mib);
    let path = fixture.data().join("claims-a-lot.esp");
    let (result, peak, time) = measure(|| read_header(&path));
    assert!(result.is_err());
    assert!(
        peak < 2 * 1024 * 1024,
        "the size field must be checked against the file length before allocating: {peak}"
    );
    within("huge length field in a small file", peak, time);

    // A plugins.txt over the limit, through the command line.
    let masterlist = fixture.masterlist("plugins: []\n");
    let list = fixture.root.join("plugins.txt");
    std::fs::write(
        &list,
        vec![b'a'; usize::try_from(MAX_PLUGINS_FILE).expect("fits") * 4],
    )
    .expect("write");
    let args: Vec<String> = [
        "check",
        "--masterlist",
        masterlist.to_str().expect("utf8"),
        "--plugins",
        list.to_str().expect("utf8"),
        "--data",
        fixture.data().to_str().expect("utf8"),
    ]
    .iter()
    .map(ToString::to_string)
    .collect();
    let ((code, err), peak, time) = measure(|| {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = main_with(&args, &mut out, &mut err);
        (code, String::from_utf8_lossy(&err).into_owned())
    });
    assert_eq!(code, 2);
    assert!(err.contains("larger than the limit"), "{err}");
    assert!(peak < 4 * 1024 * 1024, "{peak}");
    within("oversized plugins.txt", peak, time);
}
