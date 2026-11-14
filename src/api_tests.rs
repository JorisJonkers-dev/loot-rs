//! Unit tests of `api`: sorting a load order over synthetic plugins, no game files.

use super::*;
use crate::env::crc32;
use crate::plugin::{FLAG_ESM, synthetic_plugin};
use crate::support_tests::TempDir;

fn slots(names: &[&str]) -> Vec<PluginSlot> {
    names
        .iter()
        .map(|name| PluginSlot::new(*name, true))
        .collect()
}

/// A `Data` directory holding the named plugins; `.esm` names get the master flag.
fn data(names: &[&str]) -> TempDir {
    let dir = TempDir::new();
    for name in names {
        dir.plugin(name, &[]);
    }
    dir
}

fn run_with(
    yaml: &str,
    dir: &TempDir,
    names: &[&str],
    options: SortOptions,
) -> Result<SortOutput, SortError> {
    let masterlist = Masterlist::parse(yaml).expect("masterlist");
    sort(&slots(names), &masterlist, dir.path(), &options)
}

fn run(yaml: &str, dir: &TempDir, names: &[&str]) -> Result<SortOutput, SortError> {
    run_with(yaml, dir, names, SortOptions::default())
}

fn order(output: &SortOutput) -> Vec<&str> {
    output.order.iter().map(String::as_str).collect()
}

#[test]
fn an_empty_masterlist_keeps_the_order_except_that_masters_go_first() {
    let dir = data(&["A.esp", "B.esm", "C.esp", "D.esm"]);
    let output = run("", &dir, &["A.esp", "B.esm", "C.esp", "D.esm"]).expect("sorts");
    assert_eq!(order(&output), ["B.esm", "D.esm", "A.esp", "C.esp"]);
    assert_eq!(output.plugins.len(), 4);
    assert_eq!(output.plugins[0].name, "B.esm");
    assert_eq!(output.plugins[0].position, 0);
    assert_eq!(output.plugins[0].group, "default");
    assert!(output.dropped_group_rules.is_empty() && output.warnings.is_empty());
}

#[test]
fn the_masters_in_a_header_load_first() {
    let dir = TempDir::new();
    dir.plugin("Base.esp", &[]);
    dir.plugin("Patch.esp", &["BASE.ESP"]);
    dir.plugin("Other.esp", &[]);
    let output = run("", &dir, &["Patch.esp", "Other.esp", "Base.esp"]).expect("sorts");
    // LOOT keeps Patch before Other (their order in the load order) and moves Base to wherever Patch needs it.
    assert_eq!(order(&output), ["Base.esp", "Patch.esp", "Other.esp"]);
}

#[test]
fn after_and_req_entries_load_first_when_their_condition_holds() {
    let dir = data(&["A.esp", "B.esp", "C.esp", "D.esp"]);
    let yaml = r#"
plugins:
  - name: A.esp
    after: [C.esp]
  - name: B.esp
    req: [D.esp]
  - name: C.esp
    after:
      - name: B.esp
        condition: 'file("Nope.esp")'
"#;
    let output = run(yaml, &dir, &["A.esp", "B.esp", "C.esp", "D.esp"]).expect("sorts");
    assert_eq!(order(&output), ["C.esp", "A.esp", "D.esp", "B.esp"]);
}

const GROUPS: &str = r"
groups:
  - name: Early
  - name: Late
    after: [Early]
plugins:
  - name: A.esp
    group: Late
  - name: B.esp
    group: Early
";

#[test]
fn groups_order_plugins_and_are_reported() {
    let dir = data(&["A.esp", "B.esp", "C.esp"]);
    let output = run(GROUPS, &dir, &["A.esp", "B.esp", "C.esp"]).expect("sorts");
    // C is in the default group, which Late does not follow; B (Early) now precedes A (Late).
    assert_eq!(order(&output), ["B.esp", "A.esp", "C.esp"]);
    assert_eq!(output.plugins[1].group, "Late");
    assert!(output.plugins.iter().all(|p| p.findings.is_empty()));
    assert_eq!(output.dropped_group_rules, []);
    assert!(output.changed(&slots(&["A.esp", "B.esp", "C.esp"])));
    assert!(!output.changed(&slots(&["B.esp", "A.esp", "C.esp"])));
}

#[test]
fn a_group_rule_that_a_requirement_contradicts_is_dropped_and_soft_unless_strict() {
    let dir = data(&["A.esp", "B.esp"]);
    let yaml = format!("{GROUPS}  - name: B.esp\n    after: [A.esp]\n");
    let output = run(&yaml, &dir, &["A.esp", "B.esp"]).expect("sorts");
    assert_eq!(order(&output), ["A.esp", "B.esp"]);
    assert_eq!(
        output.dropped_group_rules,
        [("A.esp".to_string(), "B.esp".to_string())]
    );
    let finding = &output.plugins[0].findings;
    assert_eq!(finding.len(), 1);
    assert_eq!(finding[0].kind, FindingKind::GroupOrder);
    assert!(!finding[0].hard);

    let strict = run_with(
        &yaml,
        &dir,
        &["A.esp", "B.esp"],
        SortOptions::default().with_strict_groups(true),
    )
    .expect("sorts");
    assert!(strict.plugins[0].findings[0].hard);
}

#[test]
fn rules_that_contradict_each_other_are_an_error_naming_two_plugins() {
    let dir = data(&["A.esp", "B.esp"]);
    let yaml = "plugins:\n  - {name: A.esp, after: [B.esp]}\n  - {name: B.esp, after: [A.esp]}\n";
    assert_eq!(
        run(yaml, &dir, &["A.esp", "B.esp"]),
        Err(SortError::Conflict {
            first: "A.esp".into(),
            second: "B.esp".into()
        })
    );
}

#[test]
fn a_plugin_that_must_load_before_a_master_is_an_error() {
    let dir = data(&["Y.esp", "X.esm"]);
    let yaml = "plugins:\n  - {name: X.esm, after: [Y.esp]}\n";
    assert_eq!(
        run(yaml, &dir, &["Y.esp", "X.esm"]),
        Err(SortError::MasterOrder {
            plugin: "Y.esp".into(),
            master: "X.esm".into()
        })
    );
}

#[test]
fn names_match_case_insensitively_and_are_returned_as_given() {
    let dir = data(&["a.esp", "B.ESM"]);
    let output = run(
        "plugins:\n  - {name: A.ESP, after: [b.esm]}\n",
        &dir,
        &["A.ESP", "b.esm"],
    )
    .expect("sorts");
    assert_eq!(order(&output), ["b.esm", "A.ESP"]);
}

#[test]
fn plugins_that_are_not_installed_are_listed_apart_and_duplicates_are_noted() {
    let dir = data(&["A.esp"]);
    let output = run("", &dir, &["Ghost.esp", "A.esp", "a.ESP"]).expect("sorts");
    assert_eq!(order(&output), ["A.esp"]);
    assert_eq!(output.not_installed, ["Ghost.esp"]);
    assert_eq!(output.warnings.len(), 1);
    assert!(output.warnings[0].contains("more than once"));
}

#[test]
fn an_unreadable_header_is_a_warning_and_the_plugin_is_not_a_master() {
    let dir = TempDir::new();
    dir.file("Junk.esm", b"not a plugin");
    dir.plugin("A.esp", &[]);
    let output = run("", &dir, &["A.esp", "Junk.esm"]).expect("sorts");
    assert_eq!(order(&output), ["A.esp", "Junk.esm"]);
    assert!(output.warnings[0].contains("header not readable"));
}

#[test]
fn messages_tags_and_general_messages_come_with_the_result() {
    let dir = data(&["A.esp"]);
    let yaml = r#"
globals:
  - {type: warn, content: 'general'}
plugins:
  - name: A.esp
    msg: [{type: say, content: 'hello'}, {type: error, content: 'no', condition: 'file("Nope.esp")'}]
    tag: [Relev, '-Delev']
    req: [Missing.esp]
"#;
    let output = run(yaml, &dir, &["A.esp"]).expect("sorts");
    assert_eq!(
        output.general_messages,
        [Message {
            kind: MessageKind::Warn,
            text: "general".into()
        }]
    );
    let plugin = &output.plugins[0];
    assert_eq!(plugin.messages.len(), 1);
    assert_eq!(plugin.messages[0].kind, MessageKind::Say);
    assert_eq!(plugin.tags_added, ["Relev"]);
    assert_eq!(plugin.tags_removed, ["Delev"]);
    assert_eq!(plugin.findings.len(), 1);
    assert_eq!(plugin.findings[0].kind, FindingKind::MissingRequirement);
    assert!(plugin.findings[0].hard);
}

#[test]
fn cleaning_data_is_matched_by_the_checksum_of_the_installed_file() {
    let dir = TempDir::new();
    let bytes = synthetic_plugin(0, &[], Some("dirty one"));
    dir.file("A.esp", &bytes);
    dir.plugin("B.esp", &[]);
    let crc = crc32(&bytes);
    let yaml = format!(
        "plugins:\n  - name: A.esp\n    dirty:\n      - {{crc: 0x{crc:X}, util: FNVEdit, itm: 3, udr: 2, nav: 1}}\n      - {{crc: 0x1, util: Other, itm: 9}}\n    clean:\n      - {{crc: 0x{:X}, util: FNVEdit}}\n  - name: B.esp\n    dirty: [{{crc: 0x5, util: X}}]\n",
        crc ^ 1
    );
    let output = run(&yaml, &dir, &["A.esp", "B.esp"]).expect("sorts");
    assert_eq!(
        output.dirty,
        [DirtyInfo {
            plugin: "A.esp".into(),
            crc,
            status: CleaningStatus::Dirty,
            utility: "FNVEdit".into(),
            itm: 3,
            udr: 2,
            nav: 1
        }]
    );
    let clean_yaml =
        format!("plugins:\n  - name: A.esp\n    clean: [{{crc: 0x{crc:X}, util: FNVEdit}}]\n");
    let clean = run(&clean_yaml, &dir, &["A.esp"]).expect("sorts");
    assert_eq!(clean.dirty[0].status, CleaningStatus::Clean);
}

#[test]
fn a_condition_that_cannot_be_evaluated_leaves_its_ordering_rule_out_with_a_warning() {
    let dir = data(&["A.esp", "B.esp"]);
    let yaml = "plugins:\n  - name: A.esp\n    after: [{name: B.esp, condition: 'is_executable(\"x.exe\")'}]\n  - name: B.esp\n    after: [{name: A.esp, condition: 'file(('}]\n";
    let output = run(yaml, &dir, &["A.esp", "B.esp"]).expect("sorts");
    assert_eq!(order(&output), ["A.esp", "B.esp"]);
    assert_eq!(output.warnings.len(), 2);
    assert!(output.warnings[0].contains("not evaluated"));
    assert!(output.warnings[1].contains("not valid"));
}

#[test]
fn sorting_a_sorted_order_changes_nothing() {
    let dir = TempDir::new();
    dir.plugin("A.esp", &["B.esp"]);
    dir.plugin("B.esp", &[]);
    dir.plugin("C.esp", &[]);
    dir.file("M.esm", &synthetic_plugin(FLAG_ESM, &[], None));
    let first = run(GROUPS, &dir, &["C.esp", "A.esp", "B.esp", "M.esm"]).expect("sorts");
    let names: Vec<&str> = order(&first);
    let second = run(GROUPS, &dir, &names).expect("sorts");
    assert_eq!(second.order, first.order);
    assert!(!second.changed(&slots(&names)));
}

#[test]
fn bad_inputs_are_errors() {
    let dir = data(&["A.esp"]);
    let masterlist = Masterlist::parse("").expect("masterlist");
    let missing = dir.path().join("nope");
    assert_eq!(
        sort(
            &slots(&["A.esp"]),
            &masterlist,
            &missing,
            &SortOptions::default()
        ),
        Err(SortError::DataDirectory(missing.clone()))
    );
    let many: Vec<PluginSlot> = (0..=MAX_PLUGINS * 4)
        .map(|n| PluginSlot::new(format!("{n}.esp"), true))
        .collect();
    assert_eq!(
        sort(&many, &masterlist, dir.path(), &SortOptions::default()),
        Err(SortError::TooManyPlugins(MAX_PLUGINS * 4 + 1))
    );
    let big = TempDir::new();
    let names: Vec<String> = (0..=MAX_PLUGINS).map(|n| format!("P{n}.esp")).collect();
    for name in &names {
        big.plugin(name, &[]);
    }
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
    assert_eq!(
        run("", &big, &refs),
        Err(SortError::TooManyPlugins(MAX_PLUGINS + 1))
    );
}

#[test]
fn a_masterlist_is_read_from_text_and_from_a_file_with_errors_for_bad_input() {
    let warned = Masterlist::parse("plugins:\n  - name: 'Bad(?=x)\\.esp'\n").expect("parses");
    assert_eq!(warned.warnings().len(), 1);
    assert!(matches!(
        Masterlist::parse("- not a map"),
        Err(MasterlistError::Invalid(_))
    ));
    let dir = TempDir::new();
    let good = dir.file("ok.yaml", b"plugins: []\n");
    assert!(Masterlist::from_file(&good).is_ok());
    for (path, why) in [
        (dir.path().join("missing.yaml"), "missing"),
        (dir.file("bin.yaml", &[0xFF, 0xFE, 0x00]), "utf-8"),
        (
            dir.file("huge.yaml", &vec![b'#'; metadata::MAX_MASTERLIST_BYTES + 1]),
            "limit",
        ),
    ] {
        let error = Masterlist::from_file(&path).expect_err(why);
        assert!(matches!(error, MasterlistError::Io(_)), "{why}");
    }
    assert!(matches!(
        Masterlist::from_file(&dir.file("bad.yaml", b"[")),
        Err(MasterlistError::Invalid(_))
    ));
}

#[test]
fn errors_read_as_sentences() {
    for text in [
        MasterlistError::Io("x".into()).to_string(),
        MasterlistError::Invalid("x".into()).to_string(),
        SortError::DataDirectory("d".into()).to_string(),
        SortError::TooManyPlugins(5000).to_string(),
        SortError::Conflict {
            first: "A".into(),
            second: "B".into(),
        }
        .to_string(),
        SortError::MasterOrder {
            plugin: "A".into(),
            master: "B".into(),
        }
        .to_string(),
        SortError::TooMuchWork.to_string(),
    ] {
        assert!(text.len() > 5, "{text}");
    }
}

#[test]
fn the_rules_that_were_kept_say_where_they_come_from() {
    let dir = TempDir::new();
    dir.plugin("Base.esp", &[]);
    dir.plugin("Patch.esp", &["Base.esp"]);
    dir.plugin("C.esp", &[]);
    dir.plugin("D.esp", &[]);
    dir.plugin("E.esp", &[]);
    let yaml = r"
groups:
  - name: Early
  - name: Late
    after: [Early]
plugins:
  - {name: C.esp, after: [D.esp]}
  - {name: D.esp, req: [Base.esp]}
  - {name: E.esp, group: Late}
  - {name: Base.esp, group: Early}
";
    let output = run(
        yaml,
        &dir,
        &["Patch.esp", "Base.esp", "C.esp", "D.esp", "E.esp"],
    )
    .expect("sorts");
    let rule = |before: &str, after: &str, source: RuleSource| OrderingRule {
        before: before.into(),
        after: after.into(),
        source,
    };
    for wanted in [
        rule("Base.esp", "Patch.esp", RuleSource::HeaderMaster),
        rule("D.esp", "C.esp", RuleSource::After),
        rule("Base.esp", "D.esp", RuleSource::Req),
        rule("Base.esp", "E.esp", RuleSource::Group),
    ] {
        assert!(
            output.ordering_rules.contains(&wanted),
            "{wanted:?} in {:?}",
            output.ordering_rules
        );
    }
    for source in [
        RuleSource::HeaderMaster,
        RuleSource::After,
        RuleSource::Req,
        RuleSource::Group,
    ] {
        assert!(source.label().len() > 2);
    }
    let position = |name: &str| {
        output
            .order
            .iter()
            .position(|n| n == name)
            .expect("present")
    };
    assert!(
        output
            .ordering_rules
            .iter()
            .all(|r| position(&r.before) < position(&r.after))
    );
}
