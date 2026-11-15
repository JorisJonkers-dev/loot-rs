//! Cancellation, progress, error codes, `api::evaluate` and the switches that skip reads, from outside the crate.

mod common;

use common::Fixture;
use loot_rs::api::{
    CancelToken, EvaluateError, Masterlist, PluginSlot, Progress, ProgressHook, SortError,
    SortOptions, Stage, evaluate, sort,
};
use loot_rs::archives::synthetic_bsa;
use loot_rs::env::crc32;
use loot_rs::plugin::synthetic_plugin;
use loot_rs::records::synthetic_records;
use std::fmt::Write as _;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// 1024 plugins that all override the same base records (every pair overlaps), with a chain of `after` rules listed
/// backwards and 40 groups: the heaviest shape of `tests/limits_sort.rs`, as files.
fn heavy_world() -> (Fixture, Vec<PluginSlot>, Masterlist) {
    let fixture = Fixture::new();
    let base: Vec<(u32, u32)> = (1..64).map(|object| (object, 0)).collect();
    fixture.file(
        "FalloutNV.esm",
        &synthetic_records(1, &[], &[(*b"NPC_", &base)]),
    );
    let mut yaml = String::from("groups:\n  - name: default\n");
    for group in 1..40 {
        let _ = writeln!(yaml, "  - name: g{group}\n    after: [g{}]", group - 1);
    }
    yaml = yaml.replace("after: [g0]", "after: [default]");
    yaml.push_str("plugins:\n");
    let mut slots = vec![PluginSlot::new("FalloutNV.esm", true)];
    for index in 0..1023u32 {
        let name = format!("P{index:04}.esp");
        let records: Vec<(u32, u32)> = (1..=1 + index % 50).map(|object| (object, 0)).collect();
        fixture.file(
            &name,
            &synthetic_records(0, &["FalloutNV.esm"], &[(*b"NPC_", &records)]),
        );
        let _ = writeln!(yaml, "  - name: '{name}'\n    group: g{}", index % 40);
        if index > 0 {
            let _ = writeln!(yaml, "    after: ['P{:04}.esp']", index - 1);
        }
        slots.push(PluginSlot::new(name, true));
    }
    slots[1..].reverse();
    (fixture, slots, Masterlist::parse(&yaml).expect("parses"))
}

#[test]
fn a_sort_cancelled_at_its_first_progress_report_stops_within_a_second_and_writes_nothing() {
    let (fixture, slots, masterlist) = heavy_world();
    let before = fixture.snapshot();
    let cancel = CancelToken::new();
    let flag = cancel.clone();
    let options = SortOptions::default()
        .with_cancel(cancel)
        .with_progress(ProgressHook::new(move |_| flag.cancel()));
    let started = Instant::now();
    let result = sort(&slots, &masterlist, &fixture.data(), &options);
    let elapsed = started.elapsed();
    assert_eq!(result, Err(SortError::Cancelled));
    assert!(elapsed < Duration::from_secs(1), "took {elapsed:?}");
    assert_eq!(fixture.snapshot(), before);
}

#[test]
fn a_sort_cancelled_while_it_sorts_stops_with_cancelled() {
    let (fixture, slots, masterlist) = heavy_world();
    let cancel = CancelToken::new();
    let flag = cancel.clone();
    let options = SortOptions::default()
        .with_cancel(cancel)
        .with_progress(ProgressHook::new(move |progress| {
            if progress.stage == Stage::Sorting {
                flag.cancel();
            }
        }));
    assert_eq!(
        sort(&slots, &masterlist, &fixture.data(), &options),
        Err(SortError::Cancelled)
    );
}

#[test]
fn a_cancel_token_made_from_a_shared_flag_stops_a_sort_that_has_not_started() {
    let fixture = Fixture::new();
    fixture.plugin("FalloutNV.esm", &[]);
    let flag = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let options = SortOptions::default().with_cancel(CancelToken::from(flag));
    let masterlist = Masterlist::parse("").expect("parses");
    let result = sort(
        &[PluginSlot::new("FalloutNV.esm", true)],
        &masterlist,
        &fixture.data(),
        &options,
    );
    assert_eq!(result, Err(SortError::Cancelled));
}

#[test]
fn progress_reports_each_stage_in_order_from_zero_with_counts_within_the_total() {
    let fixture = Fixture::new();
    fixture
        .plugin("FalloutNV.esm", &[])
        .plugin("A.esp", &["FalloutNV.esm"]);
    fixture.file("A.bsa", &synthetic_bsa(104, &[("meshes", &["a.nif"])]));
    let seen: Arc<Mutex<Vec<Progress>>> = Arc::default();
    let sink = Arc::clone(&seen);
    let options = SortOptions::default().with_progress(ProgressHook::new(move |progress| {
        sink.lock().expect("lock").push(progress);
    }));
    let slots = [
        PluginSlot::new("A.esp", true),
        PluginSlot::new("FalloutNV.esm", true),
    ];
    sort(
        &slots,
        &Masterlist::parse("").expect("parses"),
        &fixture.data(),
        &options,
    )
    .expect("sorts");
    let seen = seen.lock().expect("lock");
    let mut stages: Vec<Stage> = seen.iter().map(|progress| progress.stage).collect();
    stages.dedup();
    assert_eq!(
        stages,
        [
            Stage::ReadingPlugins,
            Stage::ReadingArchives,
            Stage::ComparingPlugins,
            Stage::Sorting,
            Stage::Checking
        ]
    );
    assert!(seen.iter().all(|progress| progress.done <= progress.total));
    for pair in seen
        .windows(2)
        .filter(|pair| pair[0].stage == pair[1].stage)
    {
        assert!(pair[0].done <= pair[1].done, "{pair:?}");
    }
    for stage in stages {
        let first = seen
            .iter()
            .find(|progress| progress.stage == stage)
            .expect("reported");
        let last = seen
            .iter()
            .rev()
            .find(|progress| progress.stage == stage)
            .expect("reported");
        assert_eq!(first.done, 0, "{stage:?}");
        assert_eq!(last.done, last.total, "{stage:?}");
    }
}

#[test]
fn every_sort_error_has_its_own_stable_code() {
    let errors = [
        (SortError::DataDirectory("x".into()), "data-directory"),
        (SortError::TooManyPlugins(2000), "too-many-plugins"),
        (
            SortError::Conflict {
                first: "A".into(),
                second: "B".into(),
            },
            "conflict",
        ),
        (
            SortError::MasterOrder {
                plugin: "A".into(),
                master: "B".into(),
            },
            "master-order",
        ),
        (SortError::GroupCycle(vec!["a".into()]), "group-cycle"),
        (SortError::TooMuchWork, "too-much-work"),
        (SortError::Internal("x".into()), "internal"),
        (SortError::Cancelled, "cancelled"),
    ];
    for (error, code) in errors {
        assert_eq!(error.code(), code, "{error}");
    }
}

fn evaluation_world() -> Fixture {
    let fixture = Fixture::new();
    fixture.plugin("FalloutNV.esm", &[]).plugin("A.esp", &[]);
    fixture.file("NVSE/nvse_1_4.dll", b"MZ");
    fixture
}

#[test]
fn a_condition_is_evaluated_against_the_load_order_and_the_files() {
    let fixture = evaluation_world();
    let slots = [
        PluginSlot::new("FalloutNV.esm", true),
        PluginSlot::new("A.esp", false),
    ];
    let check =
        |condition: &str| evaluate(condition, &slots, &fixture.data(), &SortOptions::default());
    assert_eq!(check("file(\"NVSE/nvse_1_4.dll\")"), Ok(true));
    assert_eq!(check("active(\"A.esp\")"), Ok(false));
    assert_eq!(check("file(\"A.esp\") and not active(\"A.esp\")"), Ok(true));
    assert_eq!(check(""), Ok(true));
}

#[test]
fn a_condition_that_cannot_be_evaluated_says_why_with_a_code() {
    let fixture = evaluation_world();
    let slots = [PluginSlot::new("FalloutNV.esm", true)];
    let check =
        |condition: &str| evaluate(condition, &slots, &fixture.data(), &SortOptions::default());
    let unsupported = check("product_version(\"A.esp\", ==, \"1\")").expect_err("unsupported");
    assert_eq!(
        unsupported,
        EvaluateError::Unsupported("product_version".to_string())
    );
    assert_eq!(unsupported.code(), "unsupported");
    assert_eq!(check("file(").expect_err("invalid").code(), "invalid");
    let missing = evaluate(
        "",
        &slots,
        &fixture.root.join("nope"),
        &SortOptions::default(),
    );
    assert_eq!(missing.expect_err("no directory").code(), "data-directory");
}

#[test]
fn skipping_checksums_leaves_the_cleaning_data_out() {
    let fixture = Fixture::new();
    let bytes = synthetic_plugin(0, &[], None);
    fixture.file("Mod.esp", &bytes);
    let yaml = format!(
        "plugins:\n  - name: Mod.esp\n    dirty:\n      - {{crc: 0x{:X}, util: x, itm: 1}}\n",
        crc32(&bytes)
    );
    let masterlist = Masterlist::parse(&yaml).expect("parses");
    let slots = [PluginSlot::new("Mod.esp", true)];
    let full = sort(
        &slots,
        &masterlist,
        &fixture.data(),
        &SortOptions::default(),
    )
    .expect("sorts");
    let skipped = sort(
        &slots,
        &masterlist,
        &fixture.data(),
        &SortOptions::default().with_skip_checksums(true),
    )
    .expect("sorts");
    assert_eq!(full.dirty.len(), 1);
    assert!(skipped.dirty.is_empty() && skipped.plugins[0].generated_messages.is_empty());
    assert_eq!(full.order, skipped.order);
}

#[test]
fn skipping_archives_drops_the_asset_overlap_from_the_order() {
    // B loads more files than A and they share one, so B loads first; without archive tables the given order stays.
    let fixture = Fixture::new();
    fixture.plugin("A.esp", &[]).plugin("B.esp", &[]);
    fixture.file("A.bsa", &synthetic_bsa(104, &[("meshes", &["x.nif"])]));
    fixture.file(
        "B.bsa",
        &synthetic_bsa(104, &[("meshes", &["x.nif", "y.nif"])]),
    );
    let slots = [
        PluginSlot::new("A.esp", true),
        PluginSlot::new("B.esp", true),
    ];
    let masterlist = Masterlist::parse("").expect("parses");
    let full = sort(
        &slots,
        &masterlist,
        &fixture.data(),
        &SortOptions::default(),
    )
    .expect("sorts");
    let order_only = sort(
        &slots,
        &masterlist,
        &fixture.data(),
        &SortOptions::default().order_only(),
    )
    .expect("sorts");
    assert_eq!(full.order, ["B.esp", "A.esp"]);
    assert_eq!(order_only.order, ["A.esp", "B.esp"]);
}

#[cfg(feature = "executable-versions")]
#[test]
fn skipping_executable_versions_makes_their_conditions_unsupported() {
    let fixture = evaluation_world();
    let slots = [PluginSlot::new("FalloutNV.esm", true)];
    let options = SortOptions::default().with_skip_executable_versions(true);
    let result = evaluate(
        "version(\"NVSE/nvse_1_4.dll\", \"1.0\", >=)",
        &slots,
        &fixture.data(),
        &options,
    );
    assert_eq!(
        result,
        Err(EvaluateError::Unsupported("version".to_string()))
    );
}
