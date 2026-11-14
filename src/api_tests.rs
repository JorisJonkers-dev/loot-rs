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

#[allow(clippy::needless_pass_by_value)]
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

fn records_plugin(dir: &TempDir, name: &str, masters: &[&str], form_ids: &[u32]) {
    let records: Vec<(u32, u32)> = form_ids.iter().map(|&id| (id, 0)).collect();
    let flags = u32::from(name.to_lowercase().ends_with(".esm")) * FLAG_ESM;
    dir.file(
        name,
        &crate::records::synthetic_records(flags, masters, &[(*b"NPC_", &records)]),
    );
}

#[test]
fn the_plugin_that_changes_more_of_the_same_records_loads_first() {
    let dir = TempDir::new();
    dir.plugin("FalloutNV.esm", &[]);
    records_plugin(&dir, "A.esp", &["FalloutNV.esm"], &[0x0000_0001]);
    records_plugin(
        &dir,
        "B.esp",
        &["FalloutNV.esm"],
        &[0x0000_0001, 0x0000_0002, 0x0000_0003],
    );
    // Both change record 1, B changes three records and A one: B goes first, though A was listed first.
    let output = run("", &dir, &["FalloutNV.esm", "A.esp", "B.esp"]).expect("sorts");
    assert_eq!(order(&output), ["FalloutNV.esm", "B.esp", "A.esp"]);
    // Sorting that again changes nothing.
    let again = run("", &dir, &["FalloutNV.esm", "B.esp", "A.esp"]).expect("sorts");
    assert_eq!(order(&again), order(&output));
}

#[test]
fn plugins_that_change_different_records_keep_their_order() {
    let dir = TempDir::new();
    dir.plugin("FalloutNV.esm", &[]);
    records_plugin(&dir, "A.esp", &["FalloutNV.esm"], &[0x0000_0001]);
    records_plugin(
        &dir,
        "B.esp",
        &["FalloutNV.esm"],
        &[0x0000_0002, 0x0000_0003, 0x0000_0004],
    );
    let output = run("", &dir, &["FalloutNV.esm", "A.esp", "B.esp"]).expect("sorts");
    assert_eq!(order(&output), ["FalloutNV.esm", "A.esp", "B.esp"]);
}

#[test]
fn a_rule_wins_over_the_overlap_and_a_plugin_whose_records_cannot_be_read_is_noted() {
    let dir = TempDir::new();
    dir.plugin("FalloutNV.esm", &[]);
    records_plugin(&dir, "A.esp", &["FalloutNV.esm"], &[0x0000_0001]);
    records_plugin(
        &dir,
        "B.esp",
        &["FalloutNV.esm"],
        &[0x0000_0001, 0x0000_0002, 0x0000_0003],
    );
    let yaml = "plugins:\n  - name: B.esp\n    after: [A.esp]\n";
    let output = run(yaml, &dir, &["FalloutNV.esm", "A.esp", "B.esp"]).expect("sorts");
    assert_eq!(order(&output), ["FalloutNV.esm", "A.esp", "B.esp"]);
    assert_eq!(output.warnings.len(), 0);

    // A plugin with a good header and records that are cut off counts as one without records.
    let mut bytes = crate::records::synthetic_records(0, &[], &[(*b"NPC_", &[(0x800, 16)])]);
    bytes.truncate(bytes.len() - 4);
    dir.file("Cut.esp", &bytes);
    let output = run("", &dir, &["FalloutNV.esm", "Cut.esp"]).expect("sorts");
    assert_eq!(order(&output), ["FalloutNV.esm", "Cut.esp"]);
    assert_eq!(output.warnings.len(), 1);
    assert!(output.warnings[0].starts_with("Cut.esp: records not readable"));
}

#[test]
fn the_plugin_that_loads_more_of_the_same_archive_files_goes_first() {
    let dir = TempDir::new();
    dir.plugin("FalloutNV.esm", &[]);
    dir.plugin("A.esp", &[]);
    dir.plugin("B.esp", &[]);
    dir.file(
        "A.bsa",
        &crate::archives::synthetic_bsa(104, &[("meshes", &["x.nif"])]),
    );
    dir.file(
        "B - Main.bsa",
        &crate::archives::synthetic_bsa(104, &[("meshes", &["x.nif", "y.nif", "z.nif"])]),
    );
    let output = run("", &dir, &["FalloutNV.esm", "A.esp", "B.esp"]).expect("sorts");
    assert_eq!(order(&output), ["FalloutNV.esm", "B.esp", "A.esp"]);

    // Different files in the same folder do not overlap.
    dir.file(
        "B - Main.bsa",
        &crate::archives::synthetic_bsa(104, &[("meshes", &["y.nif", "z.nif"])]),
    );
    let output = run("", &dir, &["FalloutNV.esm", "A.esp", "B.esp"]).expect("sorts");
    assert_eq!(order(&output), ["FalloutNV.esm", "A.esp", "B.esp"]);
}

#[test]
fn records_win_over_archive_files_and_an_unreadable_archive_is_noted() {
    let dir = TempDir::new();
    dir.plugin("FalloutNV.esm", &[]);
    records_plugin(
        &dir,
        "A.esp",
        &["FalloutNV.esm"],
        &[0x0000_0001, 0x0000_0002],
    );
    records_plugin(&dir, "B.esp", &["FalloutNV.esm"], &[0x0000_0001]);
    // B loads more files than A, and both have them in common, but they also have records in common and A
    // changes more of them.
    dir.file(
        "A.bsa",
        &crate::archives::synthetic_bsa(104, &[("f", &["1"])]),
    );
    dir.file(
        "B.bsa",
        &crate::archives::synthetic_bsa(104, &[("f", &["1", "2", "3"])]),
    );
    dir.file(
        "B - Broken.bsa",
        b"BSA\0 broken but longer than a header, really it is",
    );
    let output = run("", &dir, &["FalloutNV.esm", "B.esp", "A.esp"]).expect("sorts");
    assert_eq!(order(&output), ["FalloutNV.esm", "A.esp", "B.esp"]);
    assert_eq!(output.warnings.len(), 1);
    assert!(output.warnings[0].starts_with("B.esp: archive B - Broken.bsa not read"));
}

#[test]
fn messages_that_loot_generates_come_apart_from_the_masterlist_ones() {
    let dir = TempDir::new();
    dir.plugin("FalloutNV.esm", &[]);
    dir.plugin("Off.esm", &[]);
    dir.plugin("Patch.esp", &["Off.esm", "Gone.esm"]);
    dir.file(
        "Tagged.esp",
        &synthetic_plugin(0, &[], Some("A mod.\n{{BASH:Delev, Relev}}")),
    );
    let yaml = "plugins:\n  - name: Patch.esp\n    msg:\n      - {type: say, content: 'From the masterlist'}\n";
    let masterlist = Masterlist::parse(yaml).expect("masterlist");
    let load_order = vec![
        PluginSlot::new("FalloutNV.esm", true),
        PluginSlot::new("Off.esm", false),
        PluginSlot::new("Patch.esp", true),
        PluginSlot::new("Tagged.esp", true),
    ];
    let output = sort(
        &load_order,
        &masterlist,
        dir.path(),
        &SortOptions::default(),
    )
    .expect("sorts");
    let patch = output
        .plugins
        .iter()
        .find(|p| p.name == "Patch.esp")
        .expect("present");
    assert_eq!(patch.messages.len(), 1);
    assert_eq!(patch.messages[0].text, "From the masterlist");
    let generated: Vec<(MessageKind, &str)> = patch
        .generated_messages
        .iter()
        .map(|message| (message.kind, message.text.as_str()))
        .collect();
    assert_eq!(
        generated,
        [
            (
                MessageKind::Error,
                "This plugin requires \\\"Off\\.esm\\\" to be active\\, but it is inactive\\."
            ),
            (
                MessageKind::Error,
                "This plugin requires \\\"Gone\\.esm\\\" to be installed\\, but it is missing\\."
            ),
        ]
    );
    let tagged = output
        .plugins
        .iter()
        .find(|p| p.name == "Tagged.esp")
        .expect("present");
    assert_eq!(tagged.bash_tags, ["Delev", "Relev"]);
    assert_eq!(tagged.generated_messages, []);
    assert_eq!(output.generated_general_messages, []);
}

#[test]
fn more_than_255_active_plugins_are_a_warning_about_the_load_order() {
    let dir = TempDir::new();
    let mut load_order = Vec::new();
    for index in 0..256 {
        let name = format!("P{index:03}.esp");
        dir.plugin(&name, &[]);
        load_order.push(PluginSlot::new(name, true));
    }
    let masterlist = Masterlist::parse("").expect("masterlist");
    let output = sort(
        &load_order,
        &masterlist,
        dir.path(),
        &SortOptions::default(),
    )
    .expect("sorts");
    assert_eq!(output.generated_general_messages.len(), 1);
    assert_eq!(output.generated_general_messages[0].kind, MessageKind::Warn);
    assert!(
        output.generated_general_messages[0]
            .text
            .starts_with("You have 256 active plugins")
    );
    // The same plugins, one of them inactive: no warning.
    load_order[0] = PluginSlot::new("P000.esp", false);
    let output = sort(
        &load_order,
        &masterlist,
        dir.path(),
        &SortOptions::default(),
    )
    .expect("sorts");
    assert_eq!(output.generated_general_messages, []);
}

fn output_with_order(names: &[&str]) -> SortOutput {
    SortOutput {
        order: names.iter().map(|name| (*name).to_owned()).collect(),
        ..SortOutput::default()
    }
}

#[test]
fn a_plugin_that_is_not_installed_goes_after_the_installed_plugin_that_preceded_it() {
    let output = output_with_order(&["A.esm", "B.esp", "C.esp"]);
    let load_order = slots(&["A.esm", "Gone.esp", "C.esp", "B.esp"]);
    assert_eq!(
        output.order_with_not_installed(&load_order),
        ["A.esm", "Gone.esp", "B.esp", "C.esp"]
    );
}

#[test]
fn plugins_that_are_not_installed_and_lead_the_load_order_stay_first_in_their_order() {
    let output = output_with_order(&["A.esm", "B.esp"]);
    let load_order = slots(&["Gone1.esp", "Gone2.esp", "B.esp", "A.esm", "Gone3.esp"]);
    assert_eq!(
        output.order_with_not_installed(&load_order),
        ["Gone1.esp", "Gone2.esp", "A.esm", "Gone3.esp", "B.esp"]
    );
}

#[test]
fn placing_not_installed_plugins_matches_names_without_regard_to_case() {
    let output = output_with_order(&["A.esm", "B.esp"]);
    let load_order = slots(&["a.ESM", "Gone.esp", "b.esp"]);
    assert_eq!(
        output.order_with_not_installed(&load_order),
        ["A.esm", "Gone.esp", "B.esp"]
    );
}

#[test]
fn placing_with_nothing_missing_returns_the_sorted_order() {
    let output = output_with_order(&["B.esp", "A.esm"]);
    assert_eq!(
        output.order_with_not_installed(&slots(&["A.esm", "B.esp"])),
        ["B.esp", "A.esm"]
    );
}

#[test]
fn message_and_finding_kinds_have_the_labels_of_the_output() {
    assert_eq!(MessageKind::Say.label(), "say");
    assert_eq!(MessageKind::Warn.label(), "warn");
    assert_eq!(MessageKind::Error.label(), "error");
    assert_eq!(
        FindingKind::MissingRequirement.label(),
        "missing requirement"
    );
    assert_eq!(FindingKind::Incompatible.label(), "incompatible");
    assert_eq!(FindingKind::LoadAfter.label(), "load after");
    assert_eq!(FindingKind::GroupOrder.label(), "group order");
    assert_eq!(FindingKind::UndefinedGroup.label(), "undefined group");
    assert_eq!(FindingKind::Unsupported.label(), "unsupported");
    assert_eq!(FindingKind::ConditionError.label(), "condition error");
}

#[test]
fn the_sort_json_starts_with_the_integer_schema_version() {
    let json = crate::render::render_sort_json(&SortOutput::default());
    assert!(
        json.starts_with(&format!(
            "{{\"schema_version\":{SCHEMA_VERSION},\"contract\":\"loot-sort\",\"sorted\":"
        )),
        "{json}"
    );
    assert_eq!(SCHEMA_VERSION, 1);
}

#[test]
fn read_plugin_header_gives_the_masters_and_flags() {
    let dir = TempDir::new();
    dir.plugin("Patch.esp", &["Base.esm"]);
    let header = read_plugin_header(&dir.path().join("Patch.esp")).expect("header");
    assert_eq!(header.masters, ["Base.esm"]);
    assert!(!header.is_master());
}

#[test]
fn a_game_directory_makes_a_parent_path_condition_hold() {
    let game = TempDir::new();
    game.file("nvse_1_4.dll", b"x");
    let staging = TempDir::new();
    staging.plugin("A.esp", &[]);
    let yaml = "plugins:\n  - name: A.esp\n    msg:\n      - type: say\n        content: nvse is here\n        condition: 'file(\"../nvse_1_4.dll\")'\n";
    let texts = |output: &SortOutput| -> Vec<String> {
        output.plugins[0]
            .messages
            .iter()
            .map(|m| m.text.clone())
            .collect()
    };
    let without = run(yaml, &staging, &["A.esp"]).expect("sorts");
    assert_eq!(texts(&without), Vec::<String>::new());
    let options = SortOptions::default().with_game_dir(game.path());
    let with = run_with(yaml, &staging, &["A.esp"], options).expect("sorts");
    assert_eq!(texts(&with), ["nvse is here"]);
}

#[test]
fn a_plugin_only_in_an_additional_directory_is_installed_and_sorted() {
    let data_dir = TempDir::new();
    data_dir.plugin("Base.esm", &[]);
    let extra = TempDir::new();
    extra.plugin("Patch.esp", &["Base.esm"]);
    let options = SortOptions::default().with_additional_data_dir(extra.path());
    let output = run_with("", &data_dir, &["Patch.esp", "Base.esm"], options).expect("sorts");
    assert_eq!(order(&output), ["Base.esm", "Patch.esp"]);
    assert_eq!(output.not_installed, Vec::<String>::new());
    assert!(output.warnings.is_empty(), "{:?}", output.warnings);
}

#[test]
fn a_ghosted_plugin_is_sorted_as_the_plugin() {
    let dir = TempDir::new();
    dir.plugin("Base.esm", &[]);
    dir.plugin("Patch.esp.ghost", &["Base.esm"]);
    let output = run("", &dir, &["Patch.esp", "Base.esm"]).expect("sorts");
    assert_eq!(order(&output), ["Base.esm", "Patch.esp"]);
}

#[test]
fn a_case_collision_in_a_directory_is_a_warning() {
    let dir = data(&["A.esp"]);
    dir.file("x.dll", b"1");
    dir.file("X.dll", b"2");
    let output = run("", &dir, &["A.esp"]).expect("sorts");
    assert!(
        output
            .warnings
            .iter()
            .any(|w| w.contains("differ only in case")),
        "{:?}",
        output.warnings
    );
}

#[test]
fn a_missing_additional_directory_is_a_warning() {
    let dir = data(&["A.esp"]);
    let options = SortOptions::default().with_additional_data_dir("/nonexistent-loot-rs-dir");
    let output = run_with("", &dir, &["A.esp"], options).expect("sorts");
    assert!(
        output
            .warnings
            .iter()
            .any(|w| w.contains("not a directory"))
    );
}

#[test]
fn a_message_guarded_by_a_plugin_version_is_applied_from_the_description() {
    let dir = TempDir::new();
    dir.file(
        "TaleOfTwoWastelands.esm",
        &synthetic_plugin(FLAG_ESM, &[], Some("Version: 3.2.0")),
    );
    dir.plugin("A.esp", &[]);
    let yaml = "plugins:\n  - name: A.esp\n    msg:\n      - type: say\n        content: new enough\n        condition: 'version(\"TaleOfTwoWastelands.esm\", \"3.2.0\", >=)'\n      - type: say\n        content: too new\n        condition: 'version(\"TaleOfTwoWastelands.esm\", \"4.0\", >=)'\n";
    let output = run(yaml, &dir, &["TaleOfTwoWastelands.esm", "A.esp"]).expect("sorts");
    let a = output
        .plugins
        .iter()
        .find(|plugin| plugin.name == "A.esp")
        .expect("A.esp");
    let texts: Vec<&str> = a.messages.iter().map(|m| m.text.as_str()).collect();
    assert_eq!(texts, ["new enough"]);
    assert!(a.findings.is_empty(), "{:?}", a.findings);
}
