//! Unit tests of `validate`: the messages LOOT generates for an install.

use super::*;
use crate::env::Slot;
use crate::metadata::{FileRef, Tag};
use crate::support_tests::TempDir;

fn header(masters: &[&str], version: f32, description: &str) -> PluginHeader {
    PluginHeader {
        flags: 0,
        form_version: 15,
        version: Some(version),
        record_count: None,
        author: None,
        description: Some(description.to_string()),
        masters: masters.iter().map(ToString::to_string).collect(),
    }
}

fn file(name: &str) -> FileRef {
    FileRef {
        name: name.to_string(),
        display: None,
        detail: None,
        condition: None,
        constraint: None,
    }
}

fn entry() -> PluginEntry {
    crate::metadata::parse("plugins:\n  - name: A.esp\n")
        .expect("parses")
        .plugins
        .remove(0)
}

fn world(files: &[&str], slots: &[(&str, bool)]) -> (TempDir, FsEnv) {
    let dir = TempDir::new();
    for name in files {
        dir.file(name, b"x");
    }
    let env = FsEnv::new(
        dir.path(),
        slots
            .iter()
            .map(|&(name, active)| Slot {
                name: name.to_string(),
                active,
            })
            .collect(),
    );
    (dir, env)
}

fn messages(env: &FsEnv, header: &PluginHeader, entry: &PluginEntry, active: bool) -> Vec<String> {
    let groups: HashSet<&str> = ["default", "late"].into();
    let entries = [entry];
    plugin_messages(
        env,
        &Plugin {
            name: "A.esp",
            active,
            header: Some(header),
            entries: &entries,
            groups: &groups,
            group: entry.group.as_deref(),
            dirty: &[],
        },
    )
    .into_iter()
    .map(|message| message.content)
    .collect()
}

#[test]
fn markdown_escaping_puts_a_backslash_before_every_ascii_punctuation_character() {
    assert_eq!(escape_markdown("A.esp (x)"), "A\\.esp \\(x\\)");
    assert_eq!(escape_markdown("plain 42"), "plain 42");
    assert_eq!(escape_markdown("é-é"), "é\\-é");
}

#[test]
fn a_missing_or_inactive_master_of_an_active_plugin_is_an_error_and_of_an_inactive_one_nothing() {
    let (_dir, env) = world(
        &["A.esp", "Off.esm"],
        &[("A.esp", true), ("Off.esm", false)],
    );
    let head = header(&["Gone.esm", "Off.esm"], 1.34, "");
    let text = messages(&env, &head, &entry(), true);
    assert_eq!(
        text,
        [
            "This plugin requires \\\"Gone\\.esm\\\" to be installed\\, but it is missing\\.",
            "This plugin requires \\\"Off\\.esm\\\" to be active\\, but it is inactive\\.",
        ]
    );
    assert_eq!(messages(&env, &head, &entry(), false), Vec::<String>::new());
}

#[test]
fn the_filter_tag_in_the_description_or_the_masterlist_silences_the_master_checks() {
    let (_dir, env) = world(&["A.esp"], &[("A.esp", true)]);
    let by_description = header(&["Gone.esm"], 1.34, "x {{BASH:Filter}}");
    assert_eq!(
        messages(&env, &by_description, &entry(), true),
        Vec::<String>::new()
    );
    let mut tagged = entry();
    tagged.tag.push(Tag {
        name: "-Filter".to_string(),
        condition: None,
    });
    let plain = header(&["Gone.esm"], 1.34, "");
    assert_eq!(messages(&env, &plain, &tagged, true), Vec::<String>::new());
}

#[test]
fn a_plugin_that_is_its_own_master_and_a_low_header_version_are_reported() {
    let (_dir, env) = world(&["A.esp"], &[("A.esp", true)]);
    let head = header(&["a.ESP"], 1.31, "");
    assert_eq!(
        messages(&env, &head, &entry(), true),
        [
            "This plugin has itself as a master\\.",
            "This plugin has a header version of 1\\.31\\, which is less than the game\\'s minimum supported header version of 1\\.32\\.",
        ]
    );
    assert_eq!(
        messages(&env, &header(&[], 1.32, ""), &entry(), true),
        Vec::<String>::new()
    );
}

#[test]
fn a_group_the_masterlist_does_not_define_is_an_error_only_when_the_masterlist_names_it() {
    let (_dir, env) = world(&["A.esp"], &[("A.esp", true)]);
    let mut grouped = entry();
    grouped.group = Some("missing".to_string());
    assert_eq!(
        messages(&env, &header(&[], 1.34, ""), &grouped, true),
        ["This plugin belongs to the group \\\"missing\\\"\\, which does not exist\\."]
    );
    grouped.group = Some("late".to_string());
    assert_eq!(
        messages(&env, &header(&[], 1.34, ""), &grouped, true),
        Vec::<String>::new()
    );
}

#[test]
fn requirements_and_incompatibilities_say_what_is_wrong_once_per_displayed_name() {
    let (_dir, env) = world(
        &["A.esp", "Here.esp", "There.dll", "Off.esp"],
        &[("A.esp", true), ("Here.esp", true), ("Off.esp", false)],
    );
    let mut with_files = entry();
    with_files.req = vec![
        file("Missing.esp"),
        FileRef {
            display: Some("[Nice Mod](http://x)".to_string()),
            detail: Some("Get it.".to_string()),
            ..file("Other.esp")
        },
        file("Missing.esp"),
        file("Here.esp"),
    ];
    with_files.inc = vec![
        file("Here.esp"),
        file("Off.esp"),
        FileRef {
            detail: Some("Remove it.".to_string()),
            ..file("There.dll")
        },
        file("Absent.esp"),
    ];
    let text = messages(&env, &header(&[], 1.34, ""), &with_files, true);
    assert_eq!(
        text,
        [
            "This plugin requires \"Missing\\.esp\" to be installed, but it is missing.",
            "This plugin requires [Nice Mod](http://x) to be installed, but it is missing. Get it.",
            "This plugin is incompatible with \"Here\\.esp\", but both are present.",
            "This plugin is incompatible with \"There\\.dll\", but both are present. Remove it.",
        ]
    );
    // An inactive plugin is not held to its requirements.
    assert_eq!(
        messages(&env, &header(&[], 1.34, ""), &with_files, false),
        Vec::<String>::new()
    );
}

#[test]
fn a_bash_tags_file_that_removes_what_the_masterlist_adds_is_reported() {
    let (dir, env) = world(&["A.esp"], &[("A.esp", true)]);
    dir.file(
        "BashTags/A.txt",
        b"# a comment\n\nDelev, -Relev # trailing\nStats\n-Names\n",
    );
    let mut tagged = entry();
    for name in ["Relev", "-Delev", "-Stats", "Names", "Graphics"] {
        tagged.tag.push(Tag {
            name: name.to_string(),
            condition: None,
        });
    }
    assert_eq!(
        messages(&env, &header(&[], 1.34, ""), &tagged, true),
        [
            "This plugin has a BashTags file that will override the suggestions made by LOOT for the following Bash Tags\\: Delev\\, Names\\, Relev\\, Stats\\."
        ]
    );
    // No conflict, no message; no file, no message.
    let mut agreeing = entry();
    agreeing.tag.push(Tag {
        name: "Delev".to_string(),
        condition: None,
    });
    assert_eq!(
        messages(&env, &header(&[], 1.34, ""), &agreeing, true),
        Vec::<String>::new()
    );
    let (_other, bare) = world(&["A.esp"], &[("A.esp", true)]);
    assert_eq!(
        messages(&bare, &header(&[], 1.34, ""), &tagged, true),
        Vec::<String>::new()
    );
}

#[test]
fn cleaning_data_becomes_a_warning_in_every_combination_of_counts() {
    let data = |itm, udr, nav, detail: Option<&str>| Cleaning {
        crc: Some(1),
        util: "FNVEdit".to_string(),
        itm,
        udr,
        nav,
        detail: detail.map(ToString::to_string),
    };
    let text = |d: &Cleaning| dirty_message(d).content;
    assert_eq!(
        text(&data(2, 1, 3, None)),
        "FNVEdit found 2 ITM records, 1 deleted reference and 3 deleted navmeshes."
    );
    assert_eq!(text(&data(0, 0, 0, None)), "FNVEdit found dirty edits.");
    assert_eq!(
        text(&data(0, 2, 1, None)),
        "FNVEdit found 2 deleted references and 1 deleted navmesh."
    );
    assert_eq!(
        text(&data(1, 0, 2, None)),
        "FNVEdit found 1 ITM record and 2 deleted navmeshes."
    );
    assert_eq!(
        text(&data(5, 1, 0, None)),
        "FNVEdit found 5 ITM records and 1 deleted reference."
    );
    assert_eq!(text(&data(5, 0, 0, None)), "FNVEdit found 5 ITM records.");
    assert_eq!(
        text(&data(0, 1, 0, None)),
        "FNVEdit found 1 deleted reference."
    );
    assert_eq!(
        text(&data(0, 0, 4, Some("Clean it."))),
        "FNVEdit found 4 deleted navmeshes. Clean it."
    );
    assert_eq!(dirty_message(&data(1, 0, 0, None)).kind, MessageKind::Warn);

    let (_dir, env) = world(&["A.esp"], &[("A.esp", true)]);
    let groups: HashSet<&str> = ["default"].into();
    let found = data(1, 0, 0, None);
    let dirty = [&found];
    let head = header(&[], 1.34, "");
    let entries: [&PluginEntry; 0] = [];
    let text: Vec<String> = plugin_messages(
        &env,
        &Plugin {
            name: "A.esp",
            active: true,
            header: Some(&head),
            entries: &entries,
            groups: &groups,
            group: None,
            dirty: &dirty,
        },
    )
    .into_iter()
    .map(|message| message.content)
    .collect();
    assert_eq!(text, ["FNVEdit found 1 ITM record."]);
}

#[test]
fn a_plugin_without_a_readable_header_gets_no_master_messages() {
    let (_dir, env) = world(&["A.esp"], &[("A.esp", true)]);
    let groups: HashSet<&str> = ["default"].into();
    let entries: [&PluginEntry; 0] = [];
    let text = plugin_messages(
        &env,
        &Plugin {
            name: "A.esp",
            active: true,
            header: None,
            entries: &entries,
            groups: &groups,
            group: None,
            dirty: &[],
        },
    );
    assert_eq!(text, Vec::<AppliedMessage>::new());
}

#[test]
fn too_many_active_plugins_are_a_warning_for_the_whole_load_order() {
    assert_eq!(active_plugin_count(255), None);
    let message = active_plugin_count(256).expect("a warning");
    assert_eq!(message.kind, MessageKind::Warn);
    assert_eq!(
        message.content,
        "You have 256 active plugins but the game only supports up to 255\\."
    );
}

#[test]
fn the_tags_of_a_description_are_those_between_the_braces() {
    assert_eq!(
        description_tags("Text\n{{BASH:C.Climate, Delev ,Relev}} more"),
        ["C.Climate", "Delev", "Relev"]
    );
    assert_eq!(
        description_tags("no tags {{BASH:unclosed"),
        Vec::<String>::new()
    );
    assert_eq!(description_tags(""), Vec::<String>::new());
}
