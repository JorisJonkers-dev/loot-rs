//! End-to-end tests of the `loot-rs` binary on synthetic game directories.

mod common;

use common::Fixture;
use std::process::{Command, Output};

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_loot-rs"))
        .args(args)
        .output()
        .expect("run loot-rs")
}

fn check(
    fixture: &Fixture,
    masterlist: &str,
    plugins: &str,
    extra: &[&str],
) -> (i32, String, String) {
    let masterlist = fixture.masterlist(masterlist);
    let plugins = fixture.plugins_txt(plugins);
    let data = fixture.data();
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
    let output = run(&args);
    (
        output.status.code().expect("exit code"),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

fn game() -> Fixture {
    let fixture = Fixture::new();
    fixture
        .plugin("FalloutNV.esm", &[])
        .plugin("A.esp", &["FalloutNV.esm"])
        .plugin("B.esp", &["FalloutNV.esm"])
        .plugin("Early.esp", &["FalloutNV.esm"])
        .plugin("D.esp", &["FalloutNV.esm"]);
    fixture
}

const ORDER: &str = "FalloutNV.esm\r\nA.esp\r\nB.esp\r\nEarly.esp\r\nD.esp\r\n";

const MASTERLIST: &str = r#"
globals:
  - type: say
    content: 'Global note'
groups:
  - name: Late
    after: [default]
plugins:
  - name: 'A.esp'
    after: ['B.esp']
    req:
      - name: 'Missing.esp'
        display: 'Missing Mod'
        detail: 'Download it.'
    inc: ['D.esp']
    msg:
      - type: warn
        content: 'A applies'
        condition: 'file("B.esp")'
      - type: say
        content: 'never applies'
        condition: 'file("Nothing.esp")'
    tag: ['Relev', {name: '-Delev', condition: 'active("B.esp")'}]
  - name: 'Early.esp'
    group: Late
"#;

#[test]
fn hard_violations_give_exit_code_1_and_are_listed() {
    let fixture = game();
    let (code, out, _) = check(&fixture, MASTERLIST, ORDER, &[]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("Global note"), "{out}");
    assert!(out.contains("[warn] A applies"), "{out}");
    assert!(!out.contains("never applies"), "{out}");
    assert!(out.contains("tags: +Relev -Delev"), "{out}");
    assert!(
        out.contains("VIOLATION missing requirement: requires Missing Mod, which is not present. Download it."),
        "{out}"
    );
    assert!(
        out.contains("VIOLATION incompatible: is incompatible with D.esp"),
        "{out}"
    );
    assert!(
        out.contains(
            "VIOLATION load after: must load after B.esp, but B.esp is later in the load order"
        ),
        "{out}"
    );
    assert!(out.contains("Hard violations: 3."), "{out}");
}

#[test]
fn group_violations_are_soft_unless_strict() {
    let fixture = game();
    let masterlist = "groups:\n  - name: Late\n    after: [default]\nplugins:\n  - name: 'Early.esp'\n    group: Late\n";
    let (code, out, _) = check(&fixture, masterlist, ORDER, &[]);
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains("note group order: (group \"Late\") loads before D.esp (group \"default\")"),
        "{out}"
    );
    assert!(out.contains("Hard violations: 0."), "{out}");

    let (code, out, _) = check(&fixture, masterlist, ORDER, &["--strict-groups"]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("VIOLATION group order"), "{out}");
}

#[test]
fn a_clean_order_exits_0() {
    let fixture = game();
    let order = "FalloutNV.esm\nB.esp\nA.esp\nD.esp\nEarly.esp\n";
    let masterlist = "groups:\n  - name: Late\n    after: [default]\nplugins:\n  - name: 'A.esp'\n    after: ['B.esp']\n  - name: 'Early.esp'\n    group: Late\n";
    let (code, out, _) = check(&fixture, masterlist, order, &[]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("Hard violations: 0."), "{out}");
}

#[test]
fn a_required_plugin_that_loads_later_is_a_load_after_violation() {
    let fixture = game();
    let masterlist = "plugins:\n  - name: 'B.esp'\n    req: ['D.esp']\n";
    let (code, out, _) = check(&fixture, masterlist, ORDER, &[]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("requires D.esp, but D.esp is later"), "{out}");
}

#[test]
fn a_conditional_requirement_is_only_checked_when_its_condition_holds() {
    let fixture = game();
    let masterlist = "plugins:\n  - name: 'A.esp'\n    req:\n      - name: 'Missing.esp'\n        condition: 'file(\"Nothing.esp\")'\n";
    let (code, out, _) = check(&fixture, masterlist, ORDER, &[]);
    assert_eq!(code, 0, "{out}");
}

#[test]
fn regex_entries_regex_conditions_and_case_insensitive_paths_work() {
    let fixture = game();
    fixture
        .file("Meshes/Foo/tree1.nif", b"x")
        .file("Meshes/Foo/tree2.nif", b"x");
    let masterlist = r#"
plugins:
  - name: '[AB]\.esp'
    msg:
      - type: say
        content: 'regex entry matched'
      - type: say
        content: 'two trees'
        condition: 'many("MESHES/foo/tree\d\.nif")'
      - type: say
        content: 'one active D'
        condition: 'active("D.esp") and not many_active("D\.esp")'
      - type: say
        content: 'file path ignores case'
        condition: 'file("meshes/FOO/Tree1.NIF")'
"#;
    let (code, out, _) = check(&fixture, masterlist, ORDER, &[]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(out.matches("regex entry matched").count(), 2, "{out}");
    assert_eq!(out.matches("two trees").count(), 2, "{out}");
    assert_eq!(out.matches("one active D").count(), 2, "{out}");
    assert_eq!(out.matches("file path ignores case").count(), 2, "{out}");
}

#[test]
fn unsupported_functions_are_reported_and_the_item_is_not_applied() {
    let fixture = game();
    let masterlist = "plugins:\n  - name: 'A.esp'\n    msg:\n      - type: error\n        content: 'version gated'\n        condition: 'product_version(\"A.esp\", >=, \"1.0\")'\n";
    let (code, out, _) = check(&fixture, masterlist, ORDER, &[]);
    assert_eq!(code, 0, "{out}");
    assert!(!out.contains("version gated"), "{out}");
    assert!(
        out.contains("unsupported condition function product_version()"),
        "{out}"
    );
    assert!(
        out.contains("1 item(s) use the unsupported condition function product_version()"),
        "{out}"
    );
}

#[test]
fn checksum_and_master_flag_conditions_read_the_files() {
    let fixture = game();
    let crc = loot_rs::env::crc32(&std::fs::read(fixture.data().join("B.esp")).expect("read"));
    let masterlist = format!(
        "plugins:\n  - name: 'A.esp'\n    msg:\n      - type: say\n        content: 'crc matches'\n        condition: 'checksum(\"B.esp\", {crc:08X})'\n      - type: say\n        content: 'is master'\n        condition: 'is_master(\"FalloutNV.esm\") and not is_master(\"B.esp\")'\n"
    );
    let (_, out, _) = check(&fixture, &masterlist, ORDER, &[]);
    assert!(out.contains("crc matches"), "{out}");
    assert!(out.contains("is master"), "{out}");
}

#[test]
fn inactive_plugins_in_the_starred_style_are_not_active_for_conditions() {
    let fixture = game();
    let order = "*FalloutNV.esm\n*A.esp\nB.esp\n";
    let masterlist = "plugins:\n  - name: 'A.esp'\n    msg:\n      - type: say\n        content: 'B is active'\n        condition: 'active(\"B.esp\")'\n      - type: say\n        content: 'B is installed'\n        condition: 'file(\"B.esp\")'\n";
    let (_, out, _) = check(&fixture, masterlist, order, &[]);
    assert!(!out.contains("B is active"), "{out}");
    assert!(out.contains("B is installed"), "{out}");
}

#[test]
fn inactive_plugins_are_still_checked_and_marked_inactive() {
    let fixture = game();
    let masterlist =
        "plugins:\n  - name: 'B.esp'\n    msg:\n      - type: say\n        content: 'note on B'\n";
    let (_, out, _) = check(&fixture, masterlist, "*FalloutNV.esm\nB.esp\n", &[]);
    assert!(
        out.contains("B.esp (position 1, group \"default\", inactive)"),
        "{out}"
    );
    assert!(out.contains("note on B"), "{out}");
}

#[test]
fn group_order_follows_transitive_relations_and_ignores_cycles() {
    let fixture = game();
    // Top after Mid after default: Top loading before a default plugin is a violation.
    let chain = "groups:\n  - name: Mid\n    after: [default]\n  - name: Top\n    after: [Mid]\nplugins:\n  - name: 'A.esp'\n    group: Top\n";
    let (_, out, _) = check(&fixture, chain, ORDER, &[]);
    assert!(out.contains("A.esp (position 1, group \"Top\")"), "{out}");
    assert!(
        out.contains("loads before B.esp (group \"default\")"),
        "{out}"
    );
    // X after Y and Y after X: the relation is ignored for both.
    let cycle = "groups:\n  - name: X\n    after: [Y]\n  - name: Y\n    after: [X]\nplugins:\n  - name: 'A.esp'\n    group: X\n  - name: 'B.esp'\n    group: Y\n";
    let (code, out, _) = check(&fixture, cycle, ORDER, &["--strict-groups"]);
    assert_eq!(code, 0, "{out}");
}

#[test]
fn an_undefined_group_is_noted() {
    let fixture = game();
    let masterlist = "plugins:\n  - name: 'A.esp'\n    group: Nowhere\n";
    let (code, out, _) = check(&fixture, masterlist, ORDER, &[]);
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains("note undefined group: is in group \"Nowhere\""),
        "{out}"
    );
    assert_eq!(
        check(&fixture, masterlist, ORDER, &["--strict-groups"]).0,
        1
    );
}

#[test]
fn listed_plugins_that_are_not_installed_are_noted() {
    let fixture = game();
    let (_, out, _) = check(&fixture, "plugins: []\n", "FalloutNV.esm\nGhost.esp\n", &[]);
    assert!(
        out.contains("1 listed plugin(s) are not in the Data directory"),
        "{out}"
    );
    assert!(out.contains("Ghost.esp"), "{out}");
}

#[test]
fn nothing_is_written() {
    let fixture = game();
    fixture.masterlist(MASTERLIST);
    fixture.plugins_txt(ORDER);
    let before = fixture.snapshot();
    let (code, _, _) = check(&fixture, MASTERLIST, ORDER, &[]);
    assert_eq!(code, 1);
    assert_eq!(fixture.snapshot(), before);
}

#[test]
fn bad_arguments_and_unreadable_input_exit_2() {
    let fixture = game();
    assert_eq!(run(&[]).status.code(), Some(2));
    assert_eq!(run(&["sort"]).status.code(), Some(2));
    assert_eq!(run(&["check", "--masterlist", "x"]).status.code(), Some(2));
    let plugins = fixture.plugins_txt(ORDER);
    let data = fixture.data();
    let output = run(&[
        "check",
        "--masterlist",
        "/nonexistent/masterlist.yaml",
        "--plugins",
        plugins.to_str().expect("utf8"),
        "--data",
        data.to_str().expect("utf8"),
    ]);
    assert_eq!(output.status.code(), Some(2));
    let (code, _, err) = check(&fixture, "a: [unclosed", ORDER, &[]);
    assert_eq!(code, 2);
    assert!(err.contains("masterlist: YAML"), "{err}");
}

fn sort_with(fixture: &Fixture, extra: &[&str]) -> (i32, String, String) {
    let masterlist = fixture.masterlist("plugins:\n  - name: 'A.esp'\n    after: ['B.esp']\n");
    let plugins = fixture.plugins_txt("FalloutNV.esm\nD.esp\nA.esp\nB.esp\nEarly.esp\n");
    let data = fixture.data();
    let mut args = vec![
        "sort",
        "--masterlist",
        masterlist.to_str().expect("utf8"),
        "--plugins",
        plugins.to_str().expect("utf8"),
        "--data",
        data.to_str().expect("utf8"),
    ];
    args.extend_from_slice(extra);
    let output = run(&args);
    (
        output.status.code().expect("exit code"),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

#[test]
fn sort_mode_minimal_moves_one_plugin_and_names_the_constraint_in_text_and_json() {
    let fixture = game();
    let (code, text, _) = sort_with(&fixture, &["--mode", "minimal"]);
    assert_eq!(code, 0, "{text}");
    assert!(
        text.contains("Minimal-change sort: 1 plugin(s) moved"),
        "{text}"
    );
    assert!(text.contains("after constraint with"), "{text}");
    let (_, json, _) = sort_with(&fixture, &["--mode", "minimal", "--json"]);
    assert!(
        json.contains("\"mode\":\"minimal\",\"moves\":[{\"plugin\":"),
        "{json}"
    );
    assert!(json.contains("\"reason\":\"after\""), "{json}");
    let sorted = json.find("\"sorted\":").expect("sorted");
    let (a, b) = (
        json.find("\"A.esp\"").expect("A"),
        json.find("\"B.esp\"").expect("B"),
    );
    assert!(sorted < b && b < a, "B.esp is listed before A.esp: {json}");
}

#[test]
fn sort_json_without_a_mode_has_neither_mode_nor_moves_and_a_bad_mode_is_refused() {
    let fixture = game();
    let (_, json, _) = sort_with(&fixture, &["--json"]);
    assert!(!json.contains("\"moves\""), "{json}");
    assert!(!json.contains("\"mode\""), "{json}");
    let (code, _, error) = sort_with(&fixture, &["--mode", "quick"]);
    assert_eq!(code, 2);
    assert!(error.contains("--mode needs full or minimal"), "{error}");
}

#[test]
fn a_prelude_file_replaces_the_masterlist_s_prelude_for_check_and_sort() {
    let fixture = Fixture::new();
    fixture.plugin("A.esp", &[]);
    let masterlist = fixture.masterlist(
        "prelude:\n  common: &old\n    - Delev\nplugins:\n  - name: A.esp\n    tag: *new\n",
    );
    let prelude = fixture.root.join("prelude.yaml");
    std::fs::write(&prelude, "common: &new\n  - Relev\n").expect("write prelude");
    let plugins = fixture.plugins_txt("*A.esp\n");
    let data = fixture.data();
    for command in ["check", "sort"] {
        let base = [
            command,
            "--masterlist",
            masterlist.to_str().expect("utf-8"),
            "--plugins",
            plugins.to_str().expect("utf-8"),
            "--data",
            data.to_str().expect("utf-8"),
        ];
        assert_eq!(
            run(&base).status.code(),
            Some(2),
            "{command} without the prelude"
        );
        let mut with = base.to_vec();
        with.extend(["--prelude", prelude.to_str().expect("utf-8")]);
        let output = run(&with);
        assert_eq!(output.status.code(), Some(0), "{command} with the prelude");
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("Relev"),
            "{command}"
        );
    }
}
