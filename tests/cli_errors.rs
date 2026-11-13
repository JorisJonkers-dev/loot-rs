//! Exit codes and error messages of the binary: what a user sees when the input is wrong.
#![allow(clippy::format_push_string)]

mod common;

use common::Fixture;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

struct Run {
    code: i32,
    stdout: String,
    stderr: String,
}

fn run(args: &[&str]) -> Run {
    let output = Command::new(env!("CARGO_BIN_EXE_loot-rs"))
        .args(args)
        .output()
        .expect("run loot-rs");
    Run {
        code: output.status.code().expect("exit code"),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

fn check(masterlist: &Path, plugins: &Path, data: &Path, extra: &[&str]) -> Run {
    let mut args = vec![
        "check",
        "--masterlist",
        masterlist.to_str().expect("utf8"),
        "--plugins",
        plugins.to_str().expect("utf8"),
        "--data",
        data.to_str().expect("utf8"),
    ];
    args.extend_from_slice(extra);
    run(&args)
}

/// A game with a valid empty masterlist and a two-plugin load order.
fn game() -> (Fixture, std::path::PathBuf, std::path::PathBuf) {
    let fixture = Fixture::new();
    fixture
        .plugin("FalloutNV.esm", &[])
        .plugin("A.esp", &["FalloutNV.esm"]);
    let masterlist = fixture.masterlist("plugins: []\n");
    let plugins = fixture.plugins_txt("FalloutNV.esm\nA.esp\n");
    (fixture, masterlist, plugins)
}

fn assert_error(result: &Run, needle: &str) {
    assert_eq!(result.code, 2, "stderr: {}", result.stderr);
    assert!(
        result.stdout.is_empty(),
        "stdout must stay empty: {}",
        result.stdout
    );
    assert!(result.stderr.starts_with("loot-rs: "), "{}", result.stderr);
    assert!(
        result.stderr.contains(needle),
        "wanted {needle:?} in {}",
        result.stderr
    );
}

// ---- arguments -------------------------------------------------------------------------------

#[test]
fn no_arguments_is_an_error_with_the_usage() {
    let result = run(&[]);
    assert_error(&result, "missing command");
    assert!(
        result.stderr.contains("usage: loot-rs check"),
        "{}",
        result.stderr
    );
    assert!(result.stderr.contains("exit codes: 0"), "{}", result.stderr);
}

#[test]
fn an_unknown_command_is_an_error() {
    assert_error(&run(&["frobnicate"]), "unknown command \"frobnicate\"");
    assert_error(&run(&["--help"]), "unknown command \"--help\"");
}

#[test]
fn each_required_flag_is_named_when_missing() {
    assert_error(&run(&["check"]), "--masterlist is required");
    assert_error(
        &run(&["check", "--masterlist", "m"]),
        "--plugins is required",
    );
    assert_error(
        &run(&["check", "--masterlist", "m", "--plugins", "p"]),
        "--data is required",
    );
}

#[test]
fn a_flag_without_a_value_and_an_unknown_flag_are_errors() {
    assert_error(
        &run(&["check", "--masterlist"]),
        "--masterlist needs a value",
    );
    assert_error(&run(&["check", "--data"]), "--data needs a value");
    assert_error(&run(&["check", "--sort"]), "unknown argument \"--sort\"");
}

// ---- the masterlist --------------------------------------------------------------------------

#[test]
fn a_missing_masterlist_names_the_file() {
    let (fixture, _, plugins) = game();
    let absent = fixture.root.join("nowhere.yaml");
    let result = check(&absent, &plugins, &fixture.data(), &[]);
    assert_error(&result, "nowhere.yaml");
    assert!(result.stderr.contains("No such file"), "{}", result.stderr);
}

#[test]
fn a_masterlist_that_is_a_directory_is_an_error() {
    let (fixture, _, plugins) = game();
    let result = check(&fixture.data(), &plugins, &fixture.data(), &[]);
    assert_error(&result, "Data");
}

#[test]
fn a_masterlist_that_is_not_utf8_is_an_error() {
    let (fixture, _, plugins) = game();
    let path = fixture.root.join("binary.yaml");
    std::fs::write(&path, [0xFF, 0xFE, 0x00, 0x80]).expect("write");
    assert_error(&check(&path, &plugins, &fixture.data(), &[]), "valid UTF-8");
}

#[test]
fn an_unreadable_masterlist_is_an_error() {
    let (fixture, masterlist, plugins) = game();
    std::fs::set_permissions(&masterlist, std::fs::Permissions::from_mode(0o000)).expect("chmod");
    if std::fs::File::open(&masterlist).is_ok() {
        return; // running as root: permissions do not apply
    }
    assert_error(
        &check(&masterlist, &plugins, &fixture.data(), &[]),
        "Permission denied",
    );
}

#[test]
fn invalid_yaml_is_reported_as_a_masterlist_problem() {
    let (fixture, _, plugins) = game();
    let masterlist = fixture.masterlist("plugins: [unclosed\n");
    assert_error(
        &check(&masterlist, &plugins, &fixture.data(), &[]),
        "masterlist: YAML:",
    );
}

#[test]
fn a_masterlist_whose_root_is_not_a_map_is_an_error() {
    let (fixture, _, plugins) = game();
    let masterlist = fixture.masterlist("- a\n- b\n");
    assert_error(
        &check(&masterlist, &plugins, &fixture.data(), &[]),
        "masterlist: the root of the file is not a map",
    );
}

#[test]
fn an_entry_without_a_name_is_an_error() {
    let (fixture, _, plugins) = game();
    let masterlist = fixture.masterlist("plugins:\n  - group: x\n");
    assert_error(
        &check(&masterlist, &plugins, &fixture.data(), &[]),
        "masterlist: plugins: an entry has no name",
    );
}

#[test]
fn a_bad_message_type_names_the_plugin() {
    let (fixture, _, plugins) = game();
    let masterlist = fixture.masterlist(
        "plugins:\n  - name: A.esp\n    msg:\n      - type: shout\n        content: hi\n",
    );
    assert_error(
        &check(&masterlist, &plugins, &fixture.data(), &[]),
        "plugin A.esp: message type must be say, warn or error",
    );
}

#[test]
fn a_message_without_content_is_an_error() {
    let (fixture, _, plugins) = game();
    let masterlist = fixture.masterlist("globals:\n  - type: say\n");
    assert_error(
        &check(&masterlist, &plugins, &fixture.data(), &[]),
        "globals: message has no content",
    );
}

#[test]
fn an_empty_masterlist_is_valid() {
    let (fixture, _, plugins) = game();
    let masterlist = fixture.masterlist("");
    let result = check(&masterlist, &plugins, &fixture.data(), &[]);
    assert_eq!((result.code, result.stderr.as_str()), (0, ""));
}

#[test]
fn aliases_that_would_expand_to_billions_of_nodes_are_refused_quickly() {
    let (fixture, _, plugins) = game();
    let mut text = String::from("prelude:\n  - &a0 [x, x, x, x, x, x, x, x, x, x]\n");
    for level in 1..=12 {
        let previous = format!("*a{}", level - 1);
        text.push_str(&format!(
            "  - &a{level} [{}]\n",
            vec![previous; 10].join(", ")
        ));
    }
    let masterlist = fixture.masterlist(&text);
    let started = std::time::Instant::now();
    assert_error(
        &check(&masterlist, &plugins, &fixture.data(), &[]),
        "expand to more than",
    );
    assert!(started.elapsed().as_secs() < 5);
}

// ---- the plugin list and the Data directory --------------------------------------------------

#[test]
fn a_missing_plugin_list_names_the_file() {
    let (fixture, masterlist, _) = game();
    let absent = fixture.root.join("plugins-gone.txt");
    assert_error(
        &check(&masterlist, &absent, &fixture.data(), &[]),
        "plugins-gone.txt",
    );
}

#[test]
fn a_plugin_list_that_is_a_directory_is_an_error() {
    let (fixture, masterlist, _) = game();
    assert_error(
        &check(&masterlist, &fixture.data(), &fixture.data(), &[]),
        "Data",
    );
}

#[test]
fn a_missing_data_directory_and_a_data_path_that_is_a_file_are_errors() {
    let (fixture, masterlist, plugins) = game();
    let absent = fixture.root.join("NoData");
    assert_error(
        &check(&masterlist, &plugins, &absent, &[]),
        "NoData: not a directory",
    );
    assert_error(
        &check(&masterlist, &plugins, &masterlist, &[]),
        "masterlist.yaml: not a directory",
    );
}

#[test]
fn a_plugin_list_that_is_not_utf8_is_read_lossily_not_refused() {
    let (fixture, masterlist, _) = game();
    let plugins = fixture.root.join("latin1.txt");
    std::fs::write(&plugins, b"FalloutNV.esm\nCaf\xE9.esp\nA.esp\n").expect("write");
    let result = check(&masterlist, &plugins, &fixture.data(), &[]);
    assert_eq!(result.code, 0, "{}", result.stderr);
    assert!(
        result
            .stdout
            .contains("1 listed plugin(s) are not in the Data directory"),
        "{}",
        result.stdout
    );
}

#[test]
fn an_empty_plugin_list_is_valid() {
    let (fixture, masterlist, _) = game();
    let plugins = fixture.plugins_txt("");
    let result = check(&masterlist, &plugins, &fixture.data(), &[]);
    assert_eq!(result.code, 0);
    assert!(
        result.stdout.contains("Checked 0 plugin(s)"),
        "{}",
        result.stdout
    );
}

// ---- plugins that cannot be read -------------------------------------------------------------

#[test]
fn an_unreadable_plugin_is_not_an_error_but_is_not_a_master() {
    let fixture = Fixture::new();
    fixture
        .plugin("FalloutNV.esm", &[])
        .plugin("Locked.esm", &[]);
    let masterlist = fixture.masterlist(
        "plugins:\n  - name: FalloutNV.esm\n    msg:\n      - type: say\n        content: 'locked is a master'\n        condition: 'is_master(\"Locked.esm\")'\n      - type: say\n        content: 'locked is readable'\n        condition: 'readable(\"Locked.esm\")'\n",
    );
    let plugins = fixture.plugins_txt("FalloutNV.esm\nLocked.esm\n");
    let locked = fixture.data().join("Locked.esm");
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).expect("chmod");
    let root = std::fs::File::open(&locked).is_ok();
    let result = check(&masterlist, &plugins, &fixture.data(), &[]);
    assert_eq!(result.code, 0, "{}", result.stderr);
    assert_eq!(result.stderr, "");
    if root {
        assert!(result.stdout.contains("locked is a master"));
    } else {
        assert!(
            !result.stdout.contains("locked is a master"),
            "{}",
            result.stdout
        );
        assert!(
            !result.stdout.contains("locked is readable"),
            "{}",
            result.stdout
        );
    }
}

#[test]
fn a_file_that_is_not_a_plugin_is_not_a_master() {
    let fixture = Fixture::new();
    fixture
        .plugin("FalloutNV.esm", &[])
        .file("Fake.esm", b"this is not a plugin file at all");
    fixture.file("Short.esm", b"TES4");
    let masterlist = fixture.masterlist(
        "plugins:\n  - name: FalloutNV.esm\n    msg:\n      - type: say\n        content: 'fake'\n        condition: 'is_master(\"Fake.esm\") or is_master(\"Short.esm\")'\n      - type: say\n        content: 'real'\n        condition: 'is_master(\"FalloutNV.esm\")'\n",
    );
    let plugins = fixture.plugins_txt("FalloutNV.esm\nFake.esm\nShort.esm\n");
    let result = check(&masterlist, &plugins, &fixture.data(), &[]);
    assert_eq!(result.code, 0, "{}", result.stderr);
    assert!(result.stdout.contains("[say] real"), "{}", result.stdout);
    assert!(!result.stdout.contains("[say] fake"), "{}", result.stdout);
}

#[test]
fn an_unreadable_directory_in_a_regex_condition_is_a_note_not_a_failure() {
    let fixture = Fixture::new();
    fixture.plugin("A.esp", &[]);
    fixture.file("locked/x.dll", b"x");
    let locked = fixture.data().join("locked");
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).expect("chmod");
    let root = std::fs::read_dir(&locked).is_ok();
    let masterlist = fixture.masterlist(
        "plugins:\n  - name: A.esp\n    msg:\n      - type: say\n        content: 'has a dll'\n        condition: 'file(\"locked/.*\\.dll\")'\n",
    );
    let plugins = fixture.plugins_txt("A.esp\n");
    let result = check(&masterlist, &plugins, &fixture.data(), &[]);
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    assert_eq!(result.code, 0, "{}", result.stderr);
    if !root {
        assert!(
            result
                .stdout
                .contains("note condition error: cannot evaluate condition"),
            "{}",
            result.stdout
        );
        assert!(!result.stdout.contains("has a dll"));
    }
}

// ---- exit codes ------------------------------------------------------------------------------

#[test]
fn the_exit_code_is_0_without_and_1_with_a_hard_violation_and_group_findings_need_strict_groups() {
    let (fixture, _, plugins) = game();
    let clean = fixture.masterlist("plugins: []\n");
    assert_eq!(check(&clean, &plugins, &fixture.data(), &[]).code, 0);
    let broken = fixture.masterlist("plugins:\n  - name: A.esp\n    req: ['Gone.esp']\n");
    assert_eq!(check(&broken, &plugins, &fixture.data(), &[]).code, 1);
    let groups = fixture.masterlist(
        "groups:\n  - name: Late\n    after: [default]\nplugins:\n  - {name: FalloutNV.esm, group: Late}\n",
    );
    assert_eq!(check(&groups, &plugins, &fixture.data(), &[]).code, 0);
    assert_eq!(
        check(&groups, &plugins, &fixture.data(), &["--strict-groups"]).code,
        1
    );
}

#[test]
fn reports_go_to_stdout_and_errors_to_stderr() {
    let (fixture, _, plugins) = game();
    let broken = fixture.masterlist("plugins:\n  - name: A.esp\n    req: ['Gone.esp']\n");
    let violation = check(&broken, &plugins, &fixture.data(), &[]);
    assert!(violation.stdout.contains("VIOLATION"));
    assert_eq!(violation.stderr, "");
}

#[test]
fn sort_with_rules_that_contradict_each_other_is_exit_code_two_naming_both_plugins() {
    let fixture = Fixture::new();
    fixture.plugin("A.esp", &[]).plugin("B.esp", &[]);
    let masterlist = fixture.masterlist(
        "plugins:\n  - {name: A.esp, after: [B.esp]}\n  - {name: B.esp, after: [A.esp]}\n",
    );
    let plugins = fixture.plugins_txt("A.esp\nB.esp\n");
    let mut args = vec!["sort", "--masterlist", masterlist.to_str().expect("utf8")];
    args.extend(["--plugins", plugins.to_str().expect("utf8")]);
    let data = fixture.data();
    args.extend(["--data", data.to_str().expect("utf8")]);
    let result = run(&args);
    assert_eq!(result.code, 2);
    assert_eq!(result.stdout, "");
    assert!(result.stderr.contains("cannot sort"), "{}", result.stderr);
    assert!(result.stderr.contains("A.esp") && result.stderr.contains("B.esp"));
}

#[test]
fn sort_reports_a_bad_masterlist_like_check_does() {
    let fixture = Fixture::new();
    fixture.plugin("A.esp", &[]);
    let masterlist = fixture.masterlist("- not a map\n");
    let plugins = fixture.plugins_txt("A.esp\n");
    let data = fixture.data();
    let result = run(&[
        "sort",
        "--masterlist",
        masterlist.to_str().expect("utf8"),
        "--plugins",
        plugins.to_str().expect("utf8"),
        "--data",
        data.to_str().expect("utf8"),
    ]);
    assert_eq!(result.code, 2);
    assert!(result.stderr.contains("masterlist"), "{}", result.stderr);
}
