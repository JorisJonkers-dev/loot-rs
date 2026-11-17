// SPDX-License-Identifier: GPL-3.0-only
//! `api::check`, the copyable error text and the API version, from outside the crate.

mod common;

use common::Fixture;
use loot_rs::api::{
    API_VERSION, CancelToken, EvaluateError, FindingKind, Masterlist, MasterlistError, PluginSlot,
    ProgressHook, SortError, SortOptions, Stage, Userlist, check, evaluate, sort,
};
use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};

const MASTERLIST: &str = r"
globals:
  - {type: say, content: 'Masterlist for tests'}
plugins:
  - name: 'Patch.esp'
    after: [Base.esp]
    req: [Missing.esp]
    msg:
      - {type: warn, content: 'Read me'}
    tag: [Relev]
";

fn world() -> Fixture {
    let fixture = Fixture::new();
    fixture
        .plugin("FalloutNV.esm", &[])
        .plugin("Patch.esp", &[])
        .plugin("Base.esp", &[]);
    fixture
}

fn load_order() -> Vec<PluginSlot> {
    [
        ("FalloutNV.esm", true),
        ("Patch.esp", true),
        ("Base.esp", true),
        ("Gone.esp", false),
    ]
    .map(|(name, active)| PluginSlot::new(name, active))
    .to_vec()
}

#[test]
fn check_reports_the_load_order_as_given_with_its_messages_and_findings() {
    let fixture = world();
    let masterlist = Masterlist::parse(MASTERLIST).expect("masterlist");
    let result = check(
        &load_order(),
        &masterlist,
        None,
        &fixture.data(),
        &SortOptions::default(),
    )
    .expect("checks");

    let names: Vec<&str> = result.plugins.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["FalloutNV.esm", "Patch.esp", "Base.esp"]);
    assert_eq!(result.not_installed, ["Gone.esp"]);
    assert_eq!(result.general_messages[0].text, "Masterlist for tests");
    let patch = &result.plugins[1];
    assert_eq!(patch.messages[0].text, "Read me");
    assert_eq!(patch.tags_added, ["Relev"]);
    let kinds: Vec<FindingKind> = patch.findings.iter().map(|f| f.kind).collect();
    assert!(
        kinds.contains(&FindingKind::MissingRequirement),
        "{kinds:?}"
    );
    assert!(kinds.contains(&FindingKind::LoadAfter), "{kinds:?}");
    assert_eq!(result.hard_violations(), 2);
}

#[test]
fn check_finds_nothing_to_report_in_an_order_the_sort_returned() {
    let fixture = world();
    fixture.plugin("Missing.esp", &[]);
    let masterlist = Masterlist::parse(MASTERLIST).expect("masterlist");
    let mut order = load_order();
    order.push(PluginSlot::new("Missing.esp", true));
    let sorted = sort(
        &order,
        &masterlist,
        &fixture.data(),
        &SortOptions::default(),
    )
    .expect("sorts");
    let slots: Vec<PluginSlot> = sorted
        .order
        .iter()
        .map(|name| PluginSlot::new(name.clone(), true))
        .collect();
    let result = check(
        &slots,
        &masterlist,
        None,
        &fixture.data(),
        &SortOptions::default(),
    )
    .expect("checks");
    assert_eq!(result.hard_violations(), 0);
}

#[test]
fn check_merges_the_userlist_over_the_masterlist() {
    let fixture = world();
    let masterlist = Masterlist::parse("plugins: []").expect("masterlist");
    let userlist = Userlist::parse(
        "plugins:\n  - name: Base.esp\n    msg:\n      - {type: say, content: 'Mine'}\n",
    )
    .expect("userlist");
    let result = check(
        &load_order(),
        &masterlist,
        Some(&userlist),
        &fixture.data(),
        &SortOptions::default(),
    )
    .expect("checks");
    assert_eq!(result.plugins[2].messages[0].text, "Mine");
}

#[test]
fn check_reports_reading_and_checking_progress() {
    let fixture = world();
    let masterlist = Masterlist::parse(MASTERLIST).expect("masterlist");
    let seen = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&seen);
    let options = SortOptions::default().with_progress(ProgressHook::new(move |progress| {
        sink.lock()
            .expect("lock")
            .push((progress.stage, progress.done, progress.total));
    }));
    check(&load_order(), &masterlist, None, &fixture.data(), &options).expect("checks");
    let seen = seen.lock().expect("lock").clone();
    assert_eq!(seen.first(), Some(&(Stage::ReadingPlugins, 0, 3)));
    assert!(seen.contains(&(Stage::ReadingPlugins, 3, 3)), "{seen:?}");
    assert_eq!(seen.last(), Some(&(Stage::Checking, 1, 1)));
}

#[test]
fn a_cancelled_check_returns_cancelled() {
    let fixture = world();
    let masterlist = Masterlist::parse(MASTERLIST).expect("masterlist");
    let token = CancelToken::new();
    token.cancel();
    let options = SortOptions::default().with_cancel(token);
    let error =
        check(&load_order(), &masterlist, None, &fixture.data(), &options).expect_err("cancelled");
    assert_eq!(error, SortError::Cancelled);
}

#[test]
fn check_refuses_a_data_directory_that_is_not_one() {
    let masterlist = Masterlist::parse(MASTERLIST).expect("masterlist");
    let error = check(
        &load_order(),
        &masterlist,
        None,
        Path::new("/nonexistent/loot-rs/Data"),
        &SortOptions::default(),
    )
    .expect_err("no data directory");
    assert_eq!(error.code(), "data-directory");
}

#[test]
fn a_sort_error_copies_as_version_code_and_message() {
    let error = SortError::Conflict {
        first: "A.esp".into(),
        second: "B.esp".into(),
    };
    assert_eq!(
        error.copy_text(),
        format!(
            "loot-rs {API_VERSION}\nerror: conflict\nA.esp has to load before B.esp, and B.esp has to load before A.esp"
        )
    );
}

#[test]
fn a_masterlist_error_has_a_code_and_copies_without_a_command_line() {
    let fixture = Fixture::new();
    let error = Masterlist::from_file(&fixture.root.join("missing.yaml")).expect_err("missing");
    assert_eq!(error.code(), "masterlist-io");
    let text = error.copy_text();
    assert!(text.starts_with(&format!("loot-rs {API_VERSION}\nerror: masterlist-io\n")));
    assert!(!text.contains("--"), "{text}");
    let invalid = MasterlistError::Invalid("bad".into());
    assert_eq!(invalid.code(), "masterlist-invalid");
}

#[test]
fn an_evaluate_error_copies_with_its_code() {
    let fixture = world();
    let error = evaluate(
        "nonsense(",
        &load_order(),
        &fixture.data(),
        &SortOptions::default(),
    )
    .expect_err("invalid");
    assert!(matches!(error, EvaluateError::Invalid(_)));
    assert!(error.copy_text().contains("\nerror: invalid\n"));
}

#[test]
fn the_api_version_is_the_released_crate_version() {
    let manifest = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"))
        .expect("Cargo.toml");
    let version = manifest
        .lines()
        .find_map(|line| line.strip_prefix("version = "))
        .expect("version line")
        .trim_matches('"');
    assert_eq!(API_VERSION, version);
}
