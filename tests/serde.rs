//! The `serde` feature writes a sort result with exactly the keys and values of `loot-rs sort --json`.
#![cfg(feature = "serde")]

mod common;

use common::{Fixture, build_data};
use loot_rs::api::{Masterlist, PluginSlot, SortMode, SortOptions, sort};
use loot_rs::render::render_sort_json;

fn without_prefix(json: &str) -> serde_json::Value {
    let mut value: serde_json::Value = serde_json::from_str(json).expect("the CLI writes JSON");
    let object = value.as_object_mut().expect("an object");
    object.remove("schema_version").expect("has schema_version");
    object.remove("contract").expect("has contract");
    value
}

fn world() -> (Fixture, Vec<PluginSlot>, Masterlist) {
    let fixture = Fixture::new();
    build_data(
        &fixture,
        "plugin FalloutNV.esm esm\nplugin A.esp masters=FalloutNV.esm records=1\nplugin B.esp records=1,2\nbsa A.bsa meshes=a.nif",
    );
    let yaml = "groups:\n  - name: default\n  - name: late\n    after: [default]\nglobals:\n  - type: say\n    content: ' Hello '\nplugins:\n  - name: A.esp\n    group: late\n    after: [B.esp]\n    req: [Missing.esp]\n    tag: [Delev, -Relev]\n    msg:\n      - type: warn\n        content: 'Read **this**'\n  - name: B.esp\n    inc: [A.esp]\n";
    let slots = vec![
        PluginSlot::new("A.esp", true),
        PluginSlot::new("FalloutNV.esm", true),
        PluginSlot::new("B.esp", false),
        PluginSlot::new("Gone.esp", true),
    ];
    (fixture, slots, Masterlist::parse(yaml).expect("parses"))
}

#[test]
fn the_serde_output_of_a_full_sort_is_the_cli_json_without_its_two_leading_keys() {
    let (fixture, slots, masterlist) = world();
    let output = sort(
        &slots,
        &masterlist,
        &fixture.data(),
        &SortOptions::default(),
    )
    .expect("sorts");
    let ours = serde_json::to_value(&output).expect("serialises");
    assert_eq!(ours, without_prefix(&render_sort_json(&output)));
}

#[test]
fn the_serde_output_of_a_minimal_change_sort_is_the_cli_json_without_its_two_leading_keys() {
    let (fixture, slots, masterlist) = world();
    let options = SortOptions::default().with_mode(SortMode::MinimalChange);
    let output = sort(&slots, &masterlist, &fixture.data(), &options).expect("sorts");
    assert_ne!(output.moves.len(), 0);
    let ours = serde_json::to_value(&output).expect("serialises");
    assert_eq!(ours, without_prefix(&render_sort_json(&output)));
}
