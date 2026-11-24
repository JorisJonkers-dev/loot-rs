//! A userlist merged over the masterlist: user metadata and user rules in the sort, as libloot merges them.

mod common;

use common::{covers, ids};

use common::Fixture;
use loot_rs::api::{
    CycleLink, Masterlist, MoveReason, PluginSlot, RuleSource, SortError, SortOptions, Userlist,
    sort, sort_with,
};

fn slots(names: &[&str]) -> Vec<PluginSlot> {
    names
        .iter()
        .map(|name| PluginSlot::new(*name, true))
        .collect()
}

fn world(names: &[&str]) -> Fixture {
    let fixture = Fixture::new();
    for name in names {
        fixture.plugin(name, &[]);
    }
    fixture
}

#[test]
fn a_user_after_entry_moves_a_plugin_and_is_listed_with_a_user_source() {
    covers(&[ids::sorting::stages::USER_EDGES]);
    let fixture = world(&["A.esp", "B.esp"]);
    let masterlist = Masterlist::parse("").expect("parses");
    let userlist =
        Userlist::parse("plugins:\n  - name: A.esp\n    after: [B.esp]\n").expect("parses");
    let order = slots(&["A.esp", "B.esp"]);
    let without = sort(
        &order,
        &masterlist,
        &fixture.data(),
        &SortOptions::default(),
    )
    .expect("sorts");
    let with = sort_with(
        &order,
        &masterlist,
        Some(&userlist),
        &fixture.data(),
        &SortOptions::default(),
    )
    .expect("sorts");
    assert_eq!(without.order, ["A.esp", "B.esp"]);
    assert_eq!(with.order, ["B.esp", "A.esp"]);
    let rule = with
        .ordering_rules
        .iter()
        .find(|rule| rule.after == "A.esp")
        .expect("a rule");
    assert_eq!(
        (rule.before.as_str(), rule.source),
        ("B.esp", RuleSource::UserAfter)
    );
    assert_eq!(with.explanations[0].reason, MoveReason::UserAfter);
}

#[test]
fn a_user_group_that_loads_after_a_masterlist_group_changes_the_order() {
    let fixture = world(&["A.esp", "B.esp"]);
    let masterlist =
        Masterlist::parse("groups:\n  - name: default\n  - name: early\nplugins:\n  - name: B.esp\n    group: early\n")
            .expect("parses");
    let userlist = Userlist::parse(
        "groups:\n  - name: late\n    after: [early]\nplugins:\n  - name: A.esp\n    group: late\n",
    )
    .expect("parses");
    let order = slots(&["A.esp", "B.esp"]);
    let with = sort_with(
        &order,
        &masterlist,
        Some(&userlist),
        &fixture.data(),
        &SortOptions::default(),
    )
    .expect("sorts");
    assert_eq!(with.order, ["B.esp", "A.esp"]);
    let a = with
        .plugins
        .iter()
        .find(|plugin| plugin.name == "A.esp")
        .expect("A");
    assert_eq!(a.group, "late");
    let rule = with
        .ordering_rules
        .iter()
        .find(|rule| rule.after == "A.esp")
        .expect("a rule");
    assert_eq!(rule.source, RuleSource::UserGroup);
}

#[test]
fn a_cycle_through_a_user_rule_names_both_rules_and_their_sources() {
    let fixture = world(&["A.esp", "B.esp"]);
    let masterlist =
        Masterlist::parse("plugins:\n  - name: B.esp\n    after: [A.esp]\n").expect("parses");
    let userlist =
        Userlist::parse("plugins:\n  - name: A.esp\n    after: [B.esp]\n").expect("parses");
    let result = sort_with(
        &slots(&["A.esp", "B.esp"]),
        &masterlist,
        Some(&userlist),
        &fixture.data(),
        &SortOptions::default(),
    );
    let Err(SortError::RuleCycle(links)) = result else {
        panic!("expected a rule cycle, got {result:?}");
    };
    let rules: Vec<(&str, MoveReason)> = links
        .iter()
        .map(|CycleLink { plugin, rule, .. }| (plugin.as_str(), *rule))
        .collect();
    assert_eq!(rules.len(), 2);
    assert!(rules.contains(&("A.esp", MoveReason::After)), "{rules:?}");
    assert!(
        rules.contains(&("B.esp", MoveReason::UserAfter)),
        "{rules:?}"
    );
    assert_eq!(SortError::RuleCycle(links).code(), "rule-cycle");
}

#[test]
fn user_metadata_comes_before_the_masterlist_s_and_its_group_wins() {
    covers(&[ids::metadata::file::USERLIST]);
    let fixture = world(&["A.esp"]);
    let masterlist = Masterlist::parse(
        "groups:\n  - name: default\n  - name: m\n  - name: u\nplugins:\n  - name: A.esp\n    group: m\n    msg:\n      - type: say\n        content: masterlist\n    tag: [Delev]\n",
    )
    .expect("parses");
    let userlist =
        Userlist::parse("globals:\n  - type: say\n    content: user general\nplugins:\n  - name: A.esp\n    group: u\n    msg:\n      - type: warn\n        content: user\n    tag: [Relev]\n")
            .expect("parses");
    let with = sort_with(
        &slots(&["A.esp"]),
        &masterlist,
        Some(&userlist),
        &fixture.data(),
        &SortOptions::default(),
    )
    .expect("sorts");
    let a = &with.plugins[0];
    assert_eq!(a.group, "u");
    let texts: Vec<&str> = a
        .messages
        .iter()
        .map(|message| message.text.as_str())
        .collect();
    assert_eq!(texts, ["user", "masterlist"]);
    assert_eq!(a.tags_added, ["Relev", "Delev"]);
    assert_eq!(
        with.general_messages
            .last()
            .map(|message| message.text.as_str()),
        Some("user general")
    );
}
