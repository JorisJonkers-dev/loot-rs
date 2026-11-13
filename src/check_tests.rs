//! Unit tests of `check`.

use super::*;
use crate::env::{Slot, parse_plugins_txt};
use crate::metadata::parse;
use crate::support_tests::TempDir;

/// A game directory with the given installed plugins and a plugin list.
fn game(installed: &[&str], order: &str) -> (TempDir, FsEnv) {
    let dir = TempDir::new();
    let data = dir.path().join("Data");
    std::fs::create_dir_all(&data).expect("data");
    for name in installed {
        dir.plugin(&format!("Data/{name}"), &[]);
    }
    let env = FsEnv::new(&data, parse_plugins_txt(order));
    (dir, env)
}

fn check(yaml: &str, installed: &[&str], order: &str, strict: bool) -> (Report, TempDir) {
    let (dir, env) = game(installed, order);
    let metadata = parse(yaml).expect("masterlist parses");
    let options = Options {
        strict_groups: strict,
    };
    (run(&metadata, &env, options), dir)
}

fn plugin<'a>(report: &'a Report, name: &str) -> &'a PluginReport {
    report
        .plugins
        .iter()
        .find(|plugin| plugin.name == name)
        .expect("plugin in the report")
}

fn kinds(plugin: &PluginReport) -> Vec<(FindingKind, bool)> {
    plugin
        .findings
        .iter()
        .map(|finding| (finding.kind, finding.hard))
        .collect()
}

#[test]
fn finding_kinds_have_labels() {
    let labels: Vec<&str> = [
        FindingKind::MissingRequirement,
        FindingKind::Incompatible,
        FindingKind::LoadAfter,
        FindingKind::GroupOrder,
        FindingKind::UndefinedGroup,
        FindingKind::Unsupported,
        FindingKind::ConditionError,
    ]
    .into_iter()
    .map(FindingKind::label)
    .collect();
    assert_eq!(
        labels,
        [
            "missing requirement",
            "incompatible",
            "load after",
            "group order",
            "undefined group",
            "unsupported",
            "condition error"
        ]
    );
}

#[test]
fn messages_apply_when_their_condition_holds() {
    let yaml = r#"
globals:
  - {type: say, content: 'global'}
  - {type: say, content: 'hidden global', condition: 'file("Nothing.esp")'}
plugins:
  - name: A.esp
    msg:
      - {type: warn, content: 'always'}
      - {type: error, content: 'when B', condition: 'file("B.esp")'}
      - {type: say, content: 'when not B', condition: 'not file("B.esp")'}
"#;
    let (report, _dir) = check(yaml, &["A.esp", "B.esp"], "A.esp\nB.esp\n", false);
    assert_eq!(report.globals.len(), 1);
    assert_eq!(report.globals[0].content, "global");
    let texts: Vec<(&str, MessageKind)> = plugin(&report, "A.esp")
        .messages
        .iter()
        .map(|m| (m.content.as_str(), m.kind))
        .collect();
    assert_eq!(
        texts,
        [
            ("always", MessageKind::Warn),
            ("when B", MessageKind::Error)
        ]
    );
    assert_eq!(report.hard_count(), 0);
}

#[test]
fn a_missing_requirement_is_hard_and_names_the_display_text_and_detail() {
    let yaml = "plugins:\n  - name: A.esp\n    req:\n      - {name: 'Gone.esp', display: 'The Gone Mod', detail: 'Get it.'}\n";
    let (report, _dir) = check(yaml, &["A.esp"], "A.esp\n", false);
    let found = &plugin(&report, "A.esp").findings;
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].kind, FindingKind::MissingRequirement);
    assert!(found[0].hard);
    assert_eq!(
        found[0].text,
        "requires The Gone Mod, which is not present. Get it."
    );
    assert_eq!(report.hard_count(), 1);
}

#[test]
fn a_present_requirement_is_fine_but_one_that_loads_later_is_a_load_after_violation() {
    let yaml = "plugins:\n  - name: A.esp\n    req: ['B.esp']\n";
    let (ok, _d1) = check(yaml, &["A.esp", "B.esp"], "B.esp\nA.esp\n", false);
    assert_eq!(ok.hard_count(), 0);
    let (late, _d2) = check(yaml, &["A.esp", "B.esp"], "A.esp\nB.esp\n", false);
    let found = &plugin(&late, "A.esp").findings;
    assert_eq!(
        kinds(plugin(&late, "A.esp")),
        [(FindingKind::LoadAfter, true)]
    );
    assert_eq!(
        found[0].text,
        "requires B.esp, but B.esp is later in the load order"
    );
}

#[test]
fn a_requirement_with_a_false_condition_is_not_in_force() {
    let yaml = "plugins:\n  - name: A.esp\n    req:\n      - {name: 'Gone.esp', condition: 'file(\"Trigger.esp\")'}\n";
    let (off, _d1) = check(yaml, &["A.esp"], "A.esp\n", false);
    assert_eq!(off.hard_count(), 0);
    let (on, _d2) = check(
        yaml,
        &["A.esp", "Trigger.esp"],
        "A.esp\nTrigger.esp\n",
        false,
    );
    assert_eq!(on.hard_count(), 1);
}

#[test]
fn a_constraint_decides_whether_the_required_file_counts_as_present() {
    let yaml = "plugins:\n  - name: A.esp\n    req:\n      - {name: 'B.esp', constraint: 'active(\"C.esp\")'}\n";
    let (report, _dir) = check(
        yaml,
        &["A.esp", "B.esp", "C.esp"],
        "B.esp\nA.esp\nC.esp\n",
        false,
    );
    assert_eq!(
        report.hard_count(),
        0,
        "the constraint holds: B counts as present"
    );
    let (report, _dir) = check(
        yaml,
        &["A.esp", "B.esp", "C.esp"],
        "B.esp\nA.esp\n*C.esp\n",
        false,
    );
    assert_eq!(report.hard_count(), 0, "starred list: C is active");
    let (report, _dir) = check(yaml, &["A.esp", "B.esp"], "B.esp\nA.esp\n", false);
    assert_eq!(
        report.hard_count(),
        1,
        "the constraint fails: B is treated as absent"
    );
}

#[test]
fn an_incompatible_plugin_that_is_present_is_hard() {
    let yaml = "plugins:\n  - name: A.esp\n    inc:\n      - {name: 'B.esp', display: 'Mod B', detail: 'Pick one.'}\n      - {name: 'C.esp', condition: 'file(\"Nothing\")'}\n";
    let (report, _dir) = check(
        yaml,
        &["A.esp", "B.esp", "C.esp"],
        "A.esp\nB.esp\nC.esp\n",
        false,
    );
    let found = &plugin(&report, "A.esp").findings;
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].kind, FindingKind::Incompatible);
    assert_eq!(
        found[0].text,
        "is incompatible with Mod B, which is present. Pick one."
    );
    let (absent, _d) = check(yaml, &["A.esp"], "A.esp\n", false);
    assert_eq!(absent.hard_count(), 0);
}

#[test]
fn after_is_violated_only_when_the_other_plugin_is_installed_and_later() {
    let yaml = "plugins:\n  - name: A.esp\n    after: ['B.esp', {name: 'C.esp', condition: 'file(\"Nothing\")'}, 'D.esp']\n";
    let (report, _dir) = check(
        yaml,
        &["A.esp", "B.esp", "C.esp"],
        "A.esp\nB.esp\nC.esp\nD.esp\n",
        false,
    );
    let found = &plugin(&report, "A.esp").findings;
    assert_eq!(
        found.len(),
        1,
        "C's condition is false and D is not installed: {found:?}"
    );
    assert_eq!(found[0].kind, FindingKind::LoadAfter);
    assert!(found[0].text.starts_with("must load after B.esp"));
    let (ok, _d) = check(yaml, &["A.esp", "B.esp"], "B.esp\nA.esp\n", false);
    assert_eq!(ok.hard_count(), 0);
}

#[test]
fn tags_are_added_removed_cancelled_and_deduplicated() {
    let yaml = r#"
plugins:
  - name: A.esp
    tag:
      - Relev
      - Relev
      - Delev
      - '-Delev'
      - {name: Names, condition: 'file("B.esp")'}
      - {name: Stats, condition: 'file("Nothing")'}
      - '-Actors'
      - '-Actors'
"#;
    let (report, _dir) = check(yaml, &["A.esp", "B.esp"], "A.esp\nB.esp\n", false);
    let entry = plugin(&report, "A.esp");
    assert_eq!(entry.tags_add, ["Relev", "Names"]);
    assert_eq!(entry.tags_remove, ["Delev", "Actors"]);
}

#[test]
fn every_matching_entry_applies_in_file_order_and_the_first_group_wins() {
    let yaml = r"
groups:
  - name: Early
  - name: Late
    after: [Early]
plugins:
  - name: 'A.*\.esp'
    group: Early
    msg: [{type: say, content: 'from the regex entry'}]
  - name: A1.esp
    group: Late
    msg: [{type: say, content: 'from the exact entry'}]
";
    let (report, _dir) = check(yaml, &["A1.esp"], "A1.esp\n", false);
    let entry = plugin(&report, "A1.esp");
    assert_eq!(entry.entries_matched, 2);
    assert_eq!(entry.group, "Early");
    let texts: Vec<&str> = entry.messages.iter().map(|m| m.content.as_str()).collect();
    assert_eq!(texts, ["from the regex entry", "from the exact entry"]);
}

#[test]
fn plugins_without_entries_are_in_the_default_group_and_listed_when_installed_only() {
    let (report, _dir) = check("plugins: []\n", &["A.esp"], "A.esp\nGone.esp\n", false);
    assert_eq!(report.plugins.len(), 1);
    assert_eq!(plugin(&report, "A.esp").group, "default");
    assert_eq!(plugin(&report, "A.esp").entries_matched, 0);
    assert_eq!(report.not_installed, ["Gone.esp"]);
}

#[test]
fn inactive_plugins_are_reported_with_their_position() {
    let (report, _dir) = check(
        "plugins: []\n",
        &["A.esp", "B.esp"],
        "*A.esp\nB.esp\n",
        false,
    );
    assert!(plugin(&report, "A.esp").active);
    assert!(!plugin(&report, "B.esp").active);
    assert_eq!(plugin(&report, "B.esp").position, 1);
}

const GROUPS: &str = r"
groups:
  - name: Early
  - name: Mid
    after: [Early]
  - name: Late
    after: [Mid]
plugins:
  - {name: 'E.esp', group: Early}
  - {name: 'M.esp', group: Mid}
  - {name: 'L.esp', group: Late}
";

#[test]
fn a_group_that_loads_before_one_it_must_follow_is_a_soft_finding_by_default() {
    let (report, _dir) = check(
        GROUPS,
        &["E.esp", "M.esp", "L.esp"],
        "L.esp\nM.esp\nE.esp\n",
        false,
    );
    assert_eq!(report.hard_count(), 0);
    let late = plugin(&report, "L.esp");
    assert_eq!(late.findings.len(), 2, "L precedes M and (transitively) E");
    assert!(
        late.findings
            .iter()
            .all(|f| f.kind == FindingKind::GroupOrder && !f.hard)
    );
    assert_eq!(
        late.findings[0].text,
        "(group \"Late\") loads before M.esp (group \"Mid\"), but \"Late\" loads after \"Mid\""
    );
    assert_eq!(plugin(&report, "M.esp").findings.len(), 1);
    assert_eq!(plugin(&report, "E.esp").findings, []);
}

#[test]
fn strict_groups_make_group_violations_hard() {
    let (report, _dir) = check(
        GROUPS,
        &["E.esp", "M.esp", "L.esp"],
        "L.esp\nM.esp\nE.esp\n",
        true,
    );
    assert_eq!(report.hard_count(), 3);
}

#[test]
fn the_correct_group_order_has_no_findings() {
    let (report, _dir) = check(
        GROUPS,
        &["E.esp", "M.esp", "L.esp"],
        "E.esp\nM.esp\nL.esp\n",
        true,
    );
    assert_eq!(report.findings().count(), 0);
}

#[test]
fn mutually_dependent_groups_are_not_reported_in_either_order() {
    let yaml = "groups:\n  - {name: X, after: [Y]}\n  - {name: Y, after: [X]}\nplugins:\n  - {name: 'x.esp', group: X}\n  - {name: 'y.esp', group: Y}\n";
    let (report, _dir) = check(yaml, &["x.esp", "y.esp"], "x.esp\ny.esp\n", true);
    assert_eq!(report.findings().count(), 0);
    let (report, _dir) = check(yaml, &["x.esp", "y.esp"], "y.esp\nx.esp\n", true);
    assert_eq!(report.findings().count(), 0);
}

#[test]
fn an_undefined_group_is_noted_and_hard_only_when_strict() {
    let yaml = "plugins:\n  - {name: 'a.esp', group: Nowhere}\n";
    let (soft, _d1) = check(yaml, &["a.esp"], "a.esp\n", false);
    assert_eq!(
        kinds(plugin(&soft, "a.esp")),
        [(FindingKind::UndefinedGroup, false)]
    );
    let (hard, _d2) = check(yaml, &["a.esp"], "a.esp\n", true);
    assert_eq!(
        kinds(plugin(&hard, "a.esp")),
        [(FindingKind::UndefinedGroup, true)]
    );
}

// With `executable-versions`, `product_version("a.exe", ...)` on a missing file is false, not unsupported.
#[cfg(not(feature = "executable-versions"))]
#[test]
fn an_unsupported_condition_is_recorded_and_the_item_is_not_applied() {
    let yaml = r#"
globals:
  - {type: say, content: 'g', condition: 'version("a.esp", ==, "1")'}
plugins:
  - name: A.esp
    msg:
      - {type: say, content: 'm', condition: 'description_contains("A.esp", "x")'}
    tag:
      - {name: T, condition: 'is_executable("x.exe")'}
    req:
      - {name: Gone.esp, condition: 'product_version("a.exe", >, "1")'}
    after:
      - {name: B.esp, condition: 'version("b.esp", <, "2")'}
"#;
    let (report, _dir) = check(yaml, &["A.esp", "B.esp"], "A.esp\nB.esp\n", false);
    assert_eq!(report.globals, []);
    let entry = plugin(&report, "A.esp");
    assert!(entry.messages.is_empty() && entry.tags_add.is_empty());
    assert_eq!(report.hard_count(), 0, "unsupported never fails the check");
    assert_eq!(
        report
            .findings()
            .filter(|f| f.kind == FindingKind::Unsupported)
            .count(),
        5
    );
    assert!(report.findings().all(|f| !f.hard));
    assert_eq!(
        report.unsupported_functions(),
        [
            ("description_contains".to_string(), 1),
            ("is_executable".to_string(), 1),
            ("product_version".to_string(), 1),
            ("version".to_string(), 2),
        ]
    );
}

#[test]
fn an_invalid_condition_and_an_invalid_file_regex_are_condition_errors() {
    let yaml = r#"
plugins:
  - name: A.esp
    msg:
      - {type: say, content: 'm', condition: 'file("a"'}
    req:
      - {name: '(broken.*', display: 'Broken'}
"#;
    let (report, _dir) = check(yaml, &["A.esp"], "A.esp\n", false);
    let entry = plugin(&report, "A.esp");
    assert_eq!(
        kinds(entry),
        [
            (FindingKind::ConditionError, false),
            (FindingKind::ConditionError, false)
        ]
    );
    assert!(entry.findings[0].text.contains("invalid condition"));
    assert!(entry.findings[0].text.contains("item not applied"));
    assert_eq!(report.unsupported_functions(), []);
}

#[test]
fn general_findings_come_before_plugin_findings_in_the_findings_iterator() {
    let yaml = "globals:\n  - {type: say, content: g, condition: 'file(\"a\"'}\nplugins:\n  - {name: 'a.esp', group: Nowhere}\n";
    let (report, _dir) = check(yaml, &["a.esp"], "a.esp\n", false);
    let order: Vec<FindingKind> = report.findings().map(|f| f.kind).collect();
    assert_eq!(
        order,
        [FindingKind::ConditionError, FindingKind::UndefinedGroup]
    );
    assert_eq!(report.general.len(), 1);
}

#[test]
fn metadata_warnings_are_carried_into_the_report() {
    let (report, _dir) = check("plugins:\n  - name: 'Bad(?=x)\\.esp'\n", &[], "", false);
    assert_eq!(report.metadata_warnings.len(), 1);
}

#[test]
fn an_empty_load_order_gives_an_empty_report() {
    let (_dir, env) = game(&[], "");
    let report = run(&Metadata::default(), &env, Options::default());
    assert!(report.plugins.is_empty() && report.globals.is_empty());
    assert_eq!(report.not_installed, Vec::<String>::new());
    assert_eq!(report.hard_count(), 0);
}

#[test]
fn slots_are_read_in_load_order() {
    let (_dir, env) = game(&["B.esp", "A.esp"], "B.esp\nA.esp\n");
    let report = run(&Metadata::default(), &env, Options::default());
    let names: Vec<&str> = report.plugins.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["B.esp", "A.esp"]);
    assert_eq!(
        env.slots(),
        [
            Slot {
                name: "B.esp".into(),
                active: true
            },
            Slot {
                name: "A.esp".into(),
                active: true
            },
        ]
    );
}
