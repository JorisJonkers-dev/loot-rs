//! `loot_rs::api` used the way a mod manager uses it: from outside the crate, through its public items only.

mod common;

use common::Fixture;
use loot_rs::api::{
    CleaningStatus, FindingKind, Masterlist, MessageKind, MoveReason, PluginSlot, SortError,
    SortMode, SortOptions, sort,
};
use loot_rs::env::crc32;
use loot_rs::plugin::synthetic_plugin;
use std::fs;

const MASTERLIST: &str = r"
globals:
  - {type: say, content: 'Masterlist for tests'}
groups:
  - name: early
  - name: late
    after: [early]
plugins:
  - name: 'Patch\.esp'
    group: late
    after: [Base.esp]
    msg:
      - {type: warn, content: 'Needs {0}', subs: [Base.esp]}
    tag: [Relev]
  - name: 'Base.esp'
    group: early
    dirty:
      - {crc: 0xDEADBEEF, util: 'FNVEdit', itm: 2}
";

fn world() -> Fixture {
    let fixture = Fixture::new();
    fixture
        .plugin("FalloutNV.esm", &[])
        .plugin("Patch.esp", &["Base.esp"])
        .plugin("Base.esp", &[])
        .plugin("Other.esp", &[]);
    fixture
}

fn load_order() -> Vec<PluginSlot> {
    [
        ("Patch.esp", true),
        ("Other.esp", false),
        ("Base.esp", true),
        ("FalloutNV.esm", true),
        ("Gone.esp", true),
    ]
    .map(|(name, active)| PluginSlot::new(name, active))
    .to_vec()
}

#[test]
fn a_mod_manager_gets_the_order_the_messages_and_what_was_left_out() {
    let fixture = world();
    let masterlist = Masterlist::parse(MASTERLIST).expect("masterlist");
    let result = sort(
        &load_order(),
        &masterlist,
        &fixture.data(),
        &SortOptions::default(),
    )
    .expect("sorts");

    assert_eq!(
        result.order,
        ["FalloutNV.esm", "Base.esp", "Patch.esp", "Other.esp"]
    );
    assert_eq!(result.not_installed, ["Gone.esp"]);
    assert!(result.changed(&load_order()));
    assert_eq!(result.general_messages.len(), 1);
    assert_eq!(result.general_messages[0].kind, MessageKind::Say);

    let patch = result
        .plugins
        .iter()
        .find(|p| p.name == "Patch.esp")
        .expect("present");
    assert_eq!(patch.position, 2);
    assert!(patch.active);
    assert_eq!(patch.group, "late");
    assert_eq!(patch.messages[0].text, "Needs Base.esp");
    assert_eq!(patch.tags_added, ["Relev"]);
    assert_eq!(patch.findings, []);
    let other = result
        .plugins
        .iter()
        .find(|p| p.name == "Other.esp")
        .expect("present");
    assert!(!other.active);
    assert!(result.dirty.is_empty(), "the checksum does not match");
}

#[test]
fn dirty_information_is_reported_for_the_installed_file_only() {
    let fixture = world();
    let bytes = synthetic_plugin(0, &[], None);
    fs::write(fixture.data().join("Base.esp"), &bytes).expect("write");
    let yaml = MASTERLIST.replace("0xDEADBEEF", &format!("0x{:X}", crc32(&bytes)));
    let masterlist = Masterlist::parse(&yaml).expect("masterlist");
    let result = sort(
        &load_order(),
        &masterlist,
        &fixture.data(),
        &SortOptions::default(),
    )
    .expect("sorts");
    assert_eq!(result.dirty.len(), 1);
    assert_eq!(result.dirty[0].plugin, "Base.esp");
    assert_eq!(result.dirty[0].status, CleaningStatus::Dirty);
    assert_eq!(result.dirty[0].utility, "FNVEdit");
    assert_eq!(result.dirty[0].itm, 2);
}

#[test]
fn nothing_is_written_and_the_same_input_gives_the_same_result() {
    let fixture = world();
    let before: Vec<_> = fs::read_dir(fixture.data())
        .expect("list")
        .map(|e| {
            let e = e.expect("entry");
            (e.file_name(), fs::read(e.path()).expect("read"))
        })
        .collect();
    let masterlist = Masterlist::parse(MASTERLIST).expect("masterlist");
    let first = sort(
        &load_order(),
        &masterlist,
        &fixture.data(),
        &SortOptions::default(),
    )
    .expect("sorts");
    let second = sort(
        &load_order(),
        &masterlist,
        &fixture.data(),
        &SortOptions::default(),
    )
    .expect("sorts");
    assert_eq!(first, second);
    let mut after: Vec<_> = fs::read_dir(fixture.data())
        .expect("list")
        .map(|e| {
            let e = e.expect("entry");
            (e.file_name(), fs::read(e.path()).expect("read"))
        })
        .collect();
    let mut before = before;
    before.sort();
    after.sort();
    assert_eq!(before, after);
}

#[test]
fn errors_can_be_matched_and_printed_without_naming_every_variant() {
    let fixture = world();
    let masterlist = Masterlist::parse("plugins:\n  - {name: Base.esp, after: [Patch.esp]}\n")
        .expect("masterlist");
    // Patch.esp lists Base.esp as a master, and the masterlist wants the opposite.
    let error = sort(
        &load_order(),
        &masterlist,
        &fixture.data(),
        &SortOptions::default(),
    )
    .expect_err("conflict");
    match &error {
        SortError::Conflict { first, second } => assert_ne!(first, second),
        other => panic!("unexpected {other}"),
    }
    assert!(error.to_string().contains("has to load before"));
    let boxed: Box<dyn std::error::Error> = Box::new(error);
    assert!(boxed.to_string().len() > 5);
}

#[test]
fn the_kinds_of_findings_are_exhaustive_for_this_version_but_callers_keep_a_wildcard() {
    let fixture = world();
    let masterlist = Masterlist::parse("plugins:\n  - {name: Other.esp, req: [Missing.esp]}\n")
        .expect("masterlist");
    let result = sort(
        &load_order(),
        &masterlist,
        &fixture.data(),
        &SortOptions::default(),
    )
    .expect("sorts");
    let finding = &result
        .plugins
        .iter()
        .find(|p| p.name == "Other.esp")
        .expect("present")
        .findings[0];
    let label = match finding.kind {
        FindingKind::MissingRequirement => "missing",
        FindingKind::Incompatible | FindingKind::LoadAfter | FindingKind::GroupOrder => {
            "order or conflict"
        }
        _ => "other",
    };
    assert_eq!(label, "missing");
    assert!(finding.hard);
}

#[test]
fn a_mod_manager_reads_a_plugin_header_and_places_what_is_not_installed() {
    let fixture = world();
    let masterlist = Masterlist::parse(MASTERLIST).expect("masterlist");
    let load_order = load_order();
    let result = sort(
        &load_order,
        &masterlist,
        &fixture.data(),
        &SortOptions::default(),
    )
    .expect("sort");
    let full = result.order_with_not_installed(&load_order);
    assert_eq!(full.len(), load_order.len());
    assert!(full.contains(&"Gone.esp".to_owned()));
    let header =
        loot_rs::api::read_plugin_header(&fixture.data().join("Patch.esp")).expect("header");
    assert_eq!(header.masters, ["Base.esp"]);
    assert_eq!(MessageKind::Warn.label(), "warn");
    assert_eq!(FindingKind::LoadAfter.label(), "load after");
}

#[test]
fn the_default_mode_is_the_full_sort_and_reports_no_moves() {
    let fixture = world();
    let masterlist = Masterlist::parse(MASTERLIST).expect("masterlist");
    let options = SortOptions::default();
    assert_eq!(options.mode, SortMode::Full);
    let result = sort(&load_order(), &masterlist, &fixture.data(), &options).expect("sort");
    assert_eq!(result.mode, SortMode::Full);
    assert_eq!(result.moves.len(), 0);
}

#[test]
fn minimal_change_moves_only_the_plugins_a_hard_constraint_forces_and_says_why() {
    let fixture = world();
    let masterlist = Masterlist::parse(MASTERLIST).expect("masterlist");
    let options = SortOptions::default().with_mode(SortMode::MinimalChange);
    let result = sort(&load_order(), &masterlist, &fixture.data(), &options).expect("sort");
    assert_eq!(result.mode, SortMode::MinimalChange);
    // FalloutNV.esm has to come first and Base.esp before Patch.esp: two moves, Other.esp stays where it was.
    assert_eq!(result.moves.len(), 2, "{:?}", result.moves);
    let at = |name: &str| {
        result
            .order
            .iter()
            .position(|n| n == name)
            .expect("present")
    };
    assert_eq!(at("FalloutNV.esm"), 0);
    assert!(at("Base.esp") < at("Patch.esp"));
    let first = result
        .moves
        .iter()
        .find(|m| m.plugin == "FalloutNV.esm")
        .expect("moved");
    assert_eq!(
        (first.from, first.to, first.reason),
        (3, 0, MoveReason::MasterFlag)
    );
    assert!(result.moves.iter().all(|m| m.from != m.to));
    let sorted = result.order.clone();
    // The result is a valid order, so sorting it again moves nothing.
    let again: Vec<PluginSlot> = sorted
        .iter()
        .map(|name| PluginSlot::new(name.clone(), true))
        .collect();
    let second = sort(&again, &masterlist, &fixture.data(), &options).expect("sort");
    assert_eq!(second.order, sorted);
    assert_eq!(second.moves.len(), 0);
    assert!(!second.changed(&again));
}
