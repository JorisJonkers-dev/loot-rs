// SPDX-License-Identifier: GPL-3.0-only
//! `api::check`, the copyable error text and the API version, from outside the crate.

mod common;

use common::Fixture;
use loot_rs::api::{
    API_VERSION, CancelToken, EvaluateError, FindingKind, Masterlist, MasterlistError, PluginSlot,
    ProgressHook, SortError, SortOptions, Stage, Userlist, check, copy_text, evaluate, sort,
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
fn check_ends_the_reading_stage_before_it_starts_checking() {
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
    let checking = seen
        .iter()
        .position(|(stage, _, _)| *stage == Stage::Checking)
        .expect("a checking stage");
    assert_eq!(
        seen[checking - 1],
        (Stage::ReadingPlugins, 3, 3),
        "{seen:?}"
    );
}

#[test]
fn check_ticks_the_reading_stage_once_per_plugin_whose_header_was_looked_at() {
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
    let reading: Vec<usize> = seen
        .iter()
        .filter(|(stage, _, _)| *stage == Stage::ReadingPlugins)
        .map(|&(_, done, _)| done)
        .collect();
    assert_eq!(reading, [0, 1, 2, 3], "{seen:?}");
}

#[test]
fn check_reports_a_plugin_listed_twice_once() {
    let fixture = world();
    let masterlist = Masterlist::parse(MASTERLIST).expect("masterlist");
    let mut order = load_order();
    order.push(PluginSlot::new("Base.esp", true));
    let result = check(
        &order,
        &masterlist,
        None,
        &fixture.data(),
        &SortOptions::default(),
    )
    .expect("checks");
    let names: Vec<&str> = result.plugins.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["FalloutNV.esm", "Patch.esp", "Base.esp"]);
    assert_eq!(result.warnings.len(), 1, "{:?}", result.warnings);
    assert!(result.warnings[0].contains("more than once"));
}

#[test]
fn check_lists_a_not_installed_plugin_in_the_order_given_and_not_in_the_plugins() {
    let fixture = world();
    let masterlist = Masterlist::parse(MASTERLIST).expect("masterlist");
    let mut order = vec![PluginSlot::new("Gone.esm", true)];
    order.extend(load_order());
    let result = check(
        &order,
        &masterlist,
        None,
        &fixture.data(),
        &SortOptions::default(),
    )
    .expect("checks");
    assert_eq!(result.not_installed, ["Gone.esm", "Gone.esp"]);
    assert!(
        result
            .plugins
            .iter()
            .all(|plugin| plugin.name != "Gone.esm"),
        "{:?}",
        result.plugins.iter().map(|p| &p.name).collect::<Vec<_>>()
    );
}

#[test]
fn check_warns_about_a_directory_that_is_not_one() {
    let fixture = world();
    let masterlist = Masterlist::parse(MASTERLIST).expect("masterlist");
    let options = SortOptions::default().with_additional_data_dir("/nonexistent/loot-rs/Data");
    let result =
        check(&load_order(), &masterlist, None, &fixture.data(), &options).expect("checks");
    assert!(
        result
            .warnings
            .iter()
            .any(|warning| warning.contains("not a directory")),
        "{:?}",
        result.warnings
    );
}

#[test]
fn check_reports_a_general_message_whose_condition_cannot_be_evaluated() {
    let fixture = world();
    let masterlist =
        Masterlist::parse("globals:\n  - {type: say, content: 'never', condition: 'file(\"a\"'}\n")
            .expect("masterlist");
    let result = check(
        &load_order(),
        &masterlist,
        None,
        &fixture.data(),
        &SortOptions::default(),
    )
    .expect("checks");
    assert_eq!(result.general_messages, []);
    let kinds: Vec<FindingKind> = result.general_findings.iter().map(|f| f.kind).collect();
    assert_eq!(kinds, [FindingKind::ConditionError]);
    assert_eq!(
        result.hard_violations(),
        0,
        "a condition that cannot be evaluated is not a hard violation"
    );
}

#[test]
fn check_keeps_the_plugin_results_in_the_order_given() {
    let fixture = world();
    let masterlist = Masterlist::parse(MASTERLIST).expect("masterlist");
    let order = [
        PluginSlot::new("Base.esp", true),
        PluginSlot::new("Patch.esp", true),
        PluginSlot::new("FalloutNV.esm", true),
    ];
    let result = check(
        &order,
        &masterlist,
        None,
        &fixture.data(),
        &SortOptions::default(),
    )
    .expect("checks");
    let names: Vec<(&str, usize)> = result
        .plugins
        .iter()
        .map(|plugin| (plugin.name.as_str(), plugin.position))
        .collect();
    assert_eq!(
        names,
        [("Base.esp", 0), ("Patch.esp", 1), ("FalloutNV.esm", 2)]
    );
}

#[test]
fn check_merges_the_userlist_group_over_the_masterlist_group() {
    let fixture = world();
    let masterlist = Masterlist::parse(
        "groups:\n  - {name: Late, after: [default]}\nplugins:\n  - {name: Base.esp, group: Late}\n",
    )
    .expect("masterlist");
    let userlist =
        Userlist::parse("plugins:\n  - {name: Base.esp, group: default}\n").expect("userlist");
    let result = check(
        &load_order(),
        &masterlist,
        Some(&userlist),
        &fixture.data(),
        &SortOptions::default(),
    )
    .expect("checks");
    assert_eq!(result.plugins[2].group, "default");
}

#[test]
fn check_without_a_userlist_keeps_the_masterlist_group() {
    let fixture = world();
    let masterlist = Masterlist::parse(
        "groups:\n  - {name: Late, after: [default]}\nplugins:\n  - {name: Base.esp, group: Late}\n",
    )
    .expect("masterlist");
    let result = check(
        &load_order(),
        &masterlist,
        None,
        &fixture.data(),
        &SortOptions::default(),
    )
    .expect("checks");
    assert_eq!(result.plugins[2].group, "Late");
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
fn check_cancelled_from_the_progress_hook_returns_cancelled() {
    let fixture = world();
    let masterlist = Masterlist::parse(MASTERLIST).expect("masterlist");
    let token = CancelToken::new();
    let cancel = token.clone();
    let options = SortOptions::default()
        .with_cancel(token)
        .with_progress(ProgressHook::new(move |_| cancel.cancel()));
    let error =
        check(&load_order(), &masterlist, None, &fixture.data(), &options).expect_err("cancelled");
    assert_eq!(error, SortError::Cancelled);
}

#[test]
fn check_reports_the_metadata_entries_it_skipped_as_warnings() {
    let fixture = world();
    let masterlist =
        Masterlist::parse("plugins:\n  - name: 'Bad(?=x)\\.esp'\n").expect("masterlist");
    let userlist = Userlist::parse("plugins:\n  - name: 'Worse(?=x)\\.esp'\n").expect("userlist");
    let result = check(
        &load_order(),
        &masterlist,
        Some(&userlist),
        &fixture.data(),
        &SortOptions::default(),
    )
    .expect("checks");
    assert_eq!(result.warnings.len(), 2, "{:?}", result.warnings);
    assert!(result.warnings[0].contains("Bad"), "{:?}", result.warnings);
    assert!(
        result.warnings[1].starts_with("userlist:"),
        "{:?}",
        result.warnings
    );
}

#[test]
fn check_reports_a_plugin_whose_header_cannot_be_read_as_a_warning() {
    let fixture = world();
    fixture.file("Junk.esp", b"not a plugin");
    let masterlist = Masterlist::parse(MASTERLIST).expect("masterlist");
    let order = [PluginSlot::new("Junk.esp", true)];
    let result = check(
        &order,
        &masterlist,
        None,
        &fixture.data(),
        &SortOptions::default(),
    )
    .expect("checks");
    assert_eq!(result.plugins.len(), 1);
    assert!(
        result.warnings[0].contains("header not readable"),
        "{:?}",
        result.warnings
    );
}

#[test]
fn check_leaves_the_cleaning_data_out_with_skip_checksums() {
    let fixture = world();
    let bytes = fs::read(fixture.data().join("Base.esp")).expect("read the plugin");
    let crc = loot_rs::env::crc32(&bytes);
    let masterlist = Masterlist::parse(&format!(
        "plugins:\n  - name: Base.esp\n    dirty: [{{crc: 0x{crc:X}, util: FNVEdit}}]\n"
    ))
    .expect("masterlist");
    let result = check(
        &load_order(),
        &masterlist,
        None,
        &fixture.data(),
        &SortOptions::default().with_skip_checksums(true),
    )
    .expect("checks");
    assert!(result.dirty.is_empty(), "{:?}", result.dirty);
}

#[test]
fn check_reports_the_cleaning_data_of_a_dirty_plugin() {
    let fixture = world();
    let bytes = fs::read(fixture.data().join("Base.esp")).expect("read the plugin");
    let crc = loot_rs::env::crc32(&bytes);
    let masterlist = Masterlist::parse(&format!(
        "plugins:\n  - name: Base.esp\n    dirty: [{{crc: 0x{crc:X}, util: FNVEdit, itm: 1}}]\n"
    ))
    .expect("masterlist");
    let result = check(
        &load_order(),
        &masterlist,
        None,
        &fixture.data(),
        &SortOptions::default(),
    )
    .expect("checks");
    assert_eq!(result.dirty.len(), 1, "{:?}", result.dirty);
    assert_eq!(result.dirty[0].plugin, "Base.esp");
    assert_eq!(result.dirty[0].crc, crc);
}

#[test]
fn check_uses_the_message_language_asked_for() {
    let fixture = world();
    let masterlist = Masterlist::parse(
        "plugins:\n  - name: Base.esp\n    msg:\n      - type: say\n        content:\n          - lang: en\n            text: 'English'\n          - lang: de\n            text: 'Deutsch'\n",
    )
    .expect("masterlist");
    let result = check(
        &load_order(),
        &masterlist,
        None,
        &fixture.data(),
        &SortOptions::default().with_languages(["de"]),
    )
    .expect("checks");
    assert_eq!(result.plugins[2].messages[0].text, "Deutsch");
    assert_eq!(result.plugins[2].messages[0].lang, "de");
}

#[test]
fn check_counts_a_group_finding_as_hard_only_with_strict_groups() {
    let fixture = world();
    let masterlist =
        Masterlist::parse("plugins:\n  - {name: Base.esp, group: Nowhere}\n").expect("masterlist");
    let soft = check(
        &load_order(),
        &masterlist,
        None,
        &fixture.data(),
        &SortOptions::default(),
    )
    .expect("checks");
    let strict = check(
        &load_order(),
        &masterlist,
        None,
        &fixture.data(),
        &SortOptions::default().with_strict_groups(true),
    )
    .expect("checks");
    assert_eq!(soft.hard_violations(), 0);
    assert_eq!(strict.hard_violations(), 1);
}

#[test]
fn check_merges_the_userlist_general_messages_after_the_masterlist_s() {
    let fixture = world();
    let masterlist =
        Masterlist::parse("globals:\n  - {type: say, content: 'Mine'}\n").expect("masterlist");
    let userlist =
        Userlist::parse("globals:\n  - {type: warn, content: 'Yours'}\n").expect("userlist");
    let result = check(
        &load_order(),
        &masterlist,
        Some(&userlist),
        &fixture.data(),
        &SortOptions::default(),
    )
    .expect("checks");
    let texts: Vec<&str> = result
        .general_messages
        .iter()
        .map(|message| message.text.as_str())
        .collect();
    assert_eq!(texts, ["Mine", "Yours"]);
}

#[test]
fn check_merges_the_userlist_requirements_over_the_masterlist_s() {
    let fixture = world();
    let masterlist = Masterlist::parse("plugins:\n  - {name: Base.esp, req: [Missing.esp]}\n")
        .expect("masterlist");
    let userlist =
        Userlist::parse("plugins:\n  - {name: Base.esp, req: [Gone.esp]}\n").expect("userlist");
    let result = check(
        &load_order(),
        &masterlist,
        Some(&userlist),
        &fixture.data(),
        &SortOptions::default(),
    )
    .expect("checks");
    let texts: Vec<&str> = result.plugins[2]
        .findings
        .iter()
        .map(|finding| finding.text.as_str())
        .collect();
    assert_eq!(texts.len(), 2, "{texts:?}");
    // The userlist's entries come before the masterlist's, as in `sort_with`.
    assert!(texts[0].contains("Gone.esp"), "{texts:?}");
    assert!(texts[1].contains("Missing.esp"), "{texts:?}");
}

#[test]
fn check_does_not_generate_a_group_message_for_a_group_the_userlist_defines() {
    let fixture = world();
    let masterlist =
        Masterlist::parse("plugins:\n  - {name: Base.esp, group: Mine}\n").expect("masterlist");
    let userlist = Userlist::parse("groups:\n  - {name: Mine}\n").expect("userlist");
    let result = check(
        &load_order(),
        &masterlist,
        Some(&userlist),
        &fixture.data(),
        &SortOptions::default(),
    )
    .expect("checks");
    assert!(
        result.plugins[2].generated_messages.is_empty(),
        "{:?}",
        result.plugins[2].generated_messages
    );
}

#[test]
fn check_skips_the_stages_that_only_a_sort_has() {
    let fixture = world();
    let masterlist = Masterlist::parse(MASTERLIST).expect("masterlist");
    let seen = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&seen);
    let options = SortOptions::default().with_progress(ProgressHook::new(move |progress| {
        sink.lock().expect("lock").push(progress.stage);
    }));
    check(&load_order(), &masterlist, None, &fixture.data(), &options).expect("checks");
    let seen = seen.lock().expect("lock").clone();
    assert!(!seen.contains(&Stage::ReadingArchives), "{seen:?}");
    assert!(
        !seen.contains(&Stage::ComparingPlugins) && !seen.contains(&Stage::Sorting),
        "{seen:?}"
    );
}

#[test]
fn check_reports_the_reading_stage_with_nothing_to_read_when_no_plugin_is_installed() {
    let fixture = Fixture::new();
    let masterlist = Masterlist::parse(MASTERLIST).expect("masterlist");
    let seen = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&seen);
    let options = SortOptions::default().with_progress(ProgressHook::new(move |progress| {
        sink.lock()
            .expect("lock")
            .push((progress.stage, progress.done, progress.total));
    }));
    let order = [PluginSlot::new("Gone.esp", true)];
    let result = check(&order, &masterlist, None, &fixture.data(), &options).expect("checks");
    assert_eq!(result.not_installed, ["Gone.esp"]);
    let seen = seen.lock().expect("lock").clone();
    assert_eq!(seen.first(), Some(&(Stage::ReadingPlugins, 0, 0)));
    assert_eq!(seen.last(), Some(&(Stage::Checking, 1, 1)));
}

#[test]
fn check_refuses_more_listed_plugins_than_the_limit_allows() {
    let fixture = Fixture::new();
    let masterlist = Masterlist::parse(MASTERLIST).expect("masterlist");
    let many: Vec<PluginSlot> = (0..=loot_rs::sort::MAX_PLUGINS * 4)
        .map(|n| PluginSlot::new(format!("{n}.esp"), true))
        .collect();
    let error = check(
        &many,
        &masterlist,
        None,
        &fixture.data(),
        &SortOptions::default(),
    )
    .expect_err("too many");
    assert_eq!(error.code(), "too-many-plugins");
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
#[cfg(unix)]
fn copy_text_writes_the_home_directory_of_the_message_as_a_tilde() {
    let home = std::env::var("HOME").expect("HOME is set for the tests");
    let home = home.trim_end_matches('/');
    let message = format!("{home}/Data: not a directory");
    assert_eq!(
        copy_text("data-directory", &message),
        format!("loot-rs {API_VERSION}\nerror: data-directory\n~/Data: not a directory")
    );
}

#[test]
#[cfg(unix)]
fn copy_text_leaves_a_path_that_only_starts_like_the_home_directory_alone() {
    let home = std::env::var("HOME").expect("HOME is set for the tests");
    let message = format!("{}2/Data: not a directory", home.trim_end_matches('/'));
    assert_eq!(
        copy_text("data-directory", &message),
        format!("loot-rs {API_VERSION}\nerror: data-directory\n{message}")
    );
}

#[test]
#[cfg(unix)]
fn copy_text_writes_a_home_directory_that_ends_with_a_slash_as_a_single_tilde() {
    let home = std::env::var("HOME").expect("HOME is set for the tests");
    let home = home.trim_end_matches('/');
    assert_eq!(
        copy_text("data-directory", &format!("{home}/Data: not a directory")),
        format!("loot-rs {API_VERSION}\nerror: data-directory\n~/Data: not a directory")
    );
    assert_eq!(
        copy_text("data-directory", &format!("{home}/")),
        format!("loot-rs {API_VERSION}\nerror: data-directory\n~/")
    );
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
