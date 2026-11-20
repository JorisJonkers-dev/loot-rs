// SPDX-License-Identifier: GPL-3.0-only
//! Golden tests against the text the LOOT application itself shows.
//!
//! Every directory under `tests/golden_app/` is a synthetic case (`data.txt`, `plugins.txt`, `masterlist.yaml`) with
//! `loot-app.txt`: what LOOT's "Game > Copy Content" put on the clipboard when the application was run on that case
//! (`scripts/loot-app-capture/capture.sh`, `docs/LOOT-APP-CAPTURE.md`). The messages of every card, and the general
//! messages, must be what `loot-rs sort --json` reports (masterlist messages first, then the generated ones).
//!
//! The one line `loot-rs` does not produce is "You have not sorted your load order this session." (a note about
//! the application's session, see `docs/COMPATIBILITY.md`); it is dropped from the application's list.
//!
//! `sort_cycle` is a sort LOOT refuses (a cycle of `after` rules); `loot-rs` refuses it too, with its own text, and
//! both name the same two plugins (checked below).
//!
//! `self_master` cannot be sorted (a plugin that must load before itself), so it is checked through
//! [`loot_rs::validate::plugin_messages`] instead.

mod common;

use common::{Fixture, build_data};
use loot_rs::check::AppliedMessage;
use loot_rs::env::{FsEnv, parse_plugins_txt};
use loot_rs::metadata::MessageKind;
use loot_rs::plugin::read_header;
use loot_rs::validate::{Plugin, plugin_messages};
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const NOT_SORTED: &str = "You have not sorted your load order this session\\.";

fn cases() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden_app");
    let mut dirs: Vec<PathBuf> = fs::read_dir(root)
        .expect("tests/golden_app exists")
        .map(|entry| entry.expect("entry").path())
        .filter(|path| path.is_dir())
        .collect();
    dirs.sort();
    dirs
}

fn kind_name(kind: &str) -> &'static str {
    match kind {
        "Note" => "say",
        "Warning" => "warn",
        "Error" => "error",
        other => panic!("unknown message kind {other:?}"),
    }
}

/// `(general messages, messages by plugin)` of a `loot-app.txt`, as `kind: text` exactly as the application copied them (Markdown escapes included).
fn parse_app(text: &str) -> (Vec<String>, BTreeMap<String, Vec<String>>) {
    let mut general = Vec::new();
    let mut plugins: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut section = String::new();
    let mut in_messages = false;
    for line in text.lines().skip(2) {
        if let Some(heading) = line.strip_prefix("# ") {
            section = heading.to_string();
            in_messages = false;
            if section != "General Information" {
                plugins.entry(section.clone()).or_default();
            }
        } else if let Some(heading) = line.strip_prefix("## ") {
            in_messages = heading == "Messages";
        } else if in_messages && let Some(item) = line.strip_prefix("- ") {
            let (kind, body) = item.split_once(": ").expect("kind: text");
            let entry = format!("{}: {}", kind_name(kind), body);
            if section == "General Information" {
                general.push(entry);
            } else {
                plugins.get_mut(&section).expect("plugin").push(entry);
            }
        }
    }
    (general, plugins)
}

fn messages_of(value: &serde_json::Value, key: &str) -> Vec<String> {
    value[key]
        .as_array()
        .expect("array")
        .iter()
        .map(|m| {
            format!(
                "{}: {}",
                m["type"].as_str().expect("type"),
                m["text"].as_str().expect("text")
            )
        })
        .collect()
}

#[test]
fn every_message_is_what_the_loot_application_showed() {
    let mut checked = 0;
    for dir in cases() {
        let name = dir
            .file_name()
            .expect("name")
            .to_string_lossy()
            .into_owned();
        if name == "self_master" || name == "sort_cycle" {
            continue;
        }
        let read = |file: &str| fs::read_to_string(dir.join(file)).unwrap_or_default();
        let (mut general, app) = parse_app(&read("loot-app.txt"));
        general.retain(|line| line != &format!("warn: {NOT_SORTED}"));
        let fixture = Fixture::new();
        build_data(&fixture, &read("data.txt"));
        let masterlist = fixture.masterlist(&read("masterlist.yaml"));
        let plugins = fixture.plugins_txt(&read("plugins.txt"));
        let output = Command::new(env!("CARGO_BIN_EXE_loot-rs"))
            .args(["sort", "--json"])
            .args(["--masterlist", masterlist.to_str().expect("utf8")])
            .args(["--plugins", plugins.to_str().expect("utf8")])
            .args(["--data", fixture.data().to_str().expect("utf8")])
            .output()
            .expect("run loot-rs");
        // Exit 1 means hard violations were found, which these cases have on purpose.
        assert!(
            matches!(output.status.code(), Some(0 | 1)),
            "case {name}: {output:?}"
        );
        let json: serde_json::Value = serde_json::from_slice(&output.stdout).expect("json");
        let mut ours_general = messages_of(&json, "general_messages");
        ours_general.extend(messages_of(&json, "generated_general_messages"));
        assert_eq!(ours_general, general, "case {name}: general messages");
        for plugin in json["plugins"].as_array().expect("plugins") {
            let plugin_name = plugin["name"].as_str().expect("name");
            let mut ours = messages_of(plugin, "messages");
            ours.extend(messages_of(plugin, "generated_messages"));
            let theirs = app.get(plugin_name).cloned().unwrap_or_default();
            assert_eq!(ours, theirs, "case {name}: {plugin_name}");
            checked += 1;
        }
    }
    assert!(checked > 0);
}

#[test]
fn a_plugin_that_is_its_own_master_reads_as_the_loot_application_says() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden_app/self_master");
    let read = |file: &str| fs::read_to_string(dir.join(file)).expect("read");
    let (_, app) = parse_app(&read("loot-app.txt"));
    let fixture = Fixture::new();
    build_data(&fixture, &read("data.txt"));
    let env = FsEnv::new(&fixture.data(), parse_plugins_txt(&read("plugins.txt")));
    let header = read_header(&fixture.data().join("SelfMaster.esp")).expect("header");
    let groups = HashSet::new();
    let generated = plugin_messages(
        &env,
        &Plugin {
            name: "SelfMaster.esp",
            active: true,
            header: Some(&header),
            entries: &[],
            groups: &groups,
            group: None,
            dirty: &[],
        },
    );
    let ours: Vec<String> = generated
        .iter()
        .map(|m: &AppliedMessage| {
            let kind = match m.kind {
                MessageKind::Say => "say",
                MessageKind::Warn => "warn",
                MessageKind::Error => "error",
            };
            format!("{kind}: {}", m.content)
        })
        .collect();
    assert_eq!(ours, app["SelfMaster.esp"]);
    // Sorting refuses the same plugin: it would have to load before itself.
    let output = Command::new(env!("CARGO_BIN_EXE_loot-rs"))
        .arg("sort")
        .args([
            "--masterlist",
            dir.join("masterlist.yaml").to_str().expect("utf8"),
        ])
        .args(["--plugins", dir.join("plugins.txt").to_str().expect("utf8")])
        .args(["--data", fixture.data().to_str().expect("utf8")])
        .output()
        .expect("run");
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn a_cycle_is_refused_by_both_and_names_the_same_plugins() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden_app/sort_cycle");
    let read = |file: &str| fs::read_to_string(dir.join(file)).expect("read");
    let app = read("loot-app.txt");
    assert!(app.contains("Cyclic interaction detected between \"A\\.esp\" and \"B\\.esp\""));
    let fixture = Fixture::new();
    build_data(&fixture, &read("data.txt"));
    let output = Command::new(env!("CARGO_BIN_EXE_loot-rs"))
        .arg("sort")
        .args([
            "--masterlist",
            dir.join("masterlist.yaml").to_str().expect("utf8"),
        ])
        .args(["--plugins", dir.join("plugins.txt").to_str().expect("utf8")])
        .args(["--data", fixture.data().to_str().expect("utf8")])
        .output()
        .expect("run");
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8(output.stderr).expect("utf8");
    assert!(
        stderr.contains("A.esp") && stderr.contains("B.esp"),
        "{stderr}"
    );
}
