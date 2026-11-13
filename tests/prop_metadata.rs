//! Property-based tests of the masterlist reader.

mod common;

use loot_rs::metadata::{MessageKind, parse};
use proptest::prelude::*;
use std::fmt::Write;

/// Text that is safe inside single quotes and survives YAML unchanged.
fn text() -> impl Strategy<Value = String> {
    "[A-Za-z0-9][A-Za-z0-9 .,!-]{0,18}[A-Za-z0-9]"
}

fn plugin_name() -> impl Strategy<Value = String> {
    "[A-Za-z0-9]{1,10}\\.es[pm]"
}

fn condition() -> impl Strategy<Value = String> {
    proptest::sample::select(vec![
        "file(\"a.esp\")",
        "active(\"b.esp\") and not file(\"c.esp\")",
        "version(\"x.esp\", ==, \"1.0\")",
    ])
    .prop_map(String::from)
}

#[derive(Debug, Clone)]
struct MessageModel {
    kind: &'static str,
    content: String,
    condition: Option<String>,
}

#[derive(Debug, Clone)]
struct EntryModel {
    name: String,
    group: Option<String>,
    after: Vec<String>,
    req: Vec<(String, Option<String>)>,
    inc: Vec<String>,
    msg: Vec<MessageModel>,
    tag: Vec<String>,
}

fn message() -> impl Strategy<Value = MessageModel> {
    (
        proptest::sample::select(vec!["say", "warn", "error"]),
        text(),
        proptest::option::of(condition()),
    )
        .prop_map(|(kind, content, condition)| MessageModel {
            kind,
            content,
            condition,
        })
}

fn entry() -> impl Strategy<Value = EntryModel> {
    (
        plugin_name(),
        proptest::option::of("[A-Za-z]{1,8}"),
        proptest::collection::vec(plugin_name(), 0..3),
        proptest::collection::vec((plugin_name(), proptest::option::of(text())), 0..3),
        proptest::collection::vec(plugin_name(), 0..3),
        proptest::collection::vec(message(), 0..3),
        proptest::collection::vec("-?[A-Za-z]{1,8}", 0..3),
    )
        .prop_map(|(name, group, after, req, inc, msg, tag)| EntryModel {
            name,
            group,
            after,
            req,
            inc,
            msg,
            tag,
        })
}

fn yaml(entries: &[EntryModel]) -> String {
    let mut out = String::from("plugins:\n");
    for e in entries {
        let _ = writeln!(out, "  - name: '{}'", e.name);
        if let Some(group) = &e.group {
            let _ = writeln!(out, "    group: '{group}'");
        }
        if !e.after.is_empty() {
            let _ = writeln!(
                out,
                "    after: [{}]",
                e.after
                    .iter()
                    .map(|n| format!("'{n}'"))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        if !e.req.is_empty() {
            out.push_str("    req:\n");
            for (name, display) in &e.req {
                let _ = writeln!(out, "      - name: '{name}'");
                if let Some(display) = display {
                    let _ = writeln!(out, "        display: '{display}'");
                }
            }
        }
        if !e.inc.is_empty() {
            let _ = writeln!(
                out,
                "    inc: [{}]",
                e.inc
                    .iter()
                    .map(|n| format!("'{n}'"))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        if !e.msg.is_empty() {
            out.push_str("    msg:\n");
            for m in &e.msg {
                let _ = writeln!(
                    out,
                    "      - type: {}\n        content: '{}'",
                    m.kind, m.content
                );
                if let Some(condition) = &m.condition {
                    let _ = writeln!(out, "        condition: '{condition}'");
                }
            }
        }
        if !e.tag.is_empty() {
            let _ = writeln!(
                out,
                "    tag: [{}]",
                e.tag
                    .iter()
                    .map(|n| format!("'{n}'"))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
    }
    out
}

fn kind(word: &str) -> MessageKind {
    match word {
        "say" => MessageKind::Say,
        "warn" => MessageKind::Warn,
        _ => MessageKind::Error,
    }
}

proptest! {
    #![proptest_config(common::proptest_config(128))]

    // ---- arbitrary input never panics --------------------------------------------------------

    #[test]
    fn arbitrary_text_never_panics(text in ".{0,300}") {
        let _ = parse(&text);
    }

    #[test]
    fn arbitrary_bytes_read_lossily_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..300)) {
        let _ = parse(&String::from_utf8_lossy(&bytes));
    }

    #[test]
    fn yaml_shaped_soup_never_panics(tokens in proptest::collection::vec(
        proptest::sample::select(vec![
            "plugins:", "groups:", "globals:", "prelude:", "bash_tags:", "- ", "  ", "\n", "name: ", "x.esp",
            "msg:", "type: say", "type: warn", "content: ", "'", "\"", "[", "]", "{", "}", ", ", ": ", "&a ", "*a",
            "<<: ", "<<: *a", "after:", "req:", "inc:", "tag:", "dirty:", "clean:", "crc: 0x1", "condition: ", "#",
            "---", "...", "!!str ", "|", ">", "?", "%", "\t", "\u{feff}", "é",
        ]),
        0..60,
    )) {
        let _ = parse(&tokens.concat());
    }

    #[test]
    fn a_parse_error_names_what_is_wrong(text in ".{0,100}") {
        if let Err(error) = parse(&text) {
            prop_assert!(!error.to_string().is_empty());
        }
    }

    // ---- what is written is what is read -----------------------------------------------------

    #[test]
    fn a_generated_masterlist_is_read_back_field_by_field(entries in proptest::collection::vec(entry(), 0..6)) {
        let metadata = parse(&yaml(&entries)).expect("a generated masterlist parses");
        prop_assert_eq!(metadata.plugins.len(), entries.len());
        prop_assert!(metadata.warnings.is_empty());
        for (read, written) in metadata.plugins.iter().zip(&entries) {
            prop_assert_eq!(read.pattern.text(), written.name.as_str());
            prop_assert_eq!(read.group.as_deref(), written.group.as_deref());
            let after: Vec<&str> = read.after.iter().map(|f| f.name.as_str()).collect();
            prop_assert_eq!(after, written.after.iter().map(String::as_str).collect::<Vec<_>>());
            let req: Vec<(&str, Option<&str>)> = read.req.iter().map(|f| (f.name.as_str(), f.display.as_deref())).collect();
            prop_assert_eq!(req, written.req.iter().map(|(n, d)| (n.as_str(), d.as_deref())).collect::<Vec<_>>());
            let inc: Vec<&str> = read.inc.iter().map(|f| f.name.as_str()).collect();
            prop_assert_eq!(inc, written.inc.iter().map(String::as_str).collect::<Vec<_>>());
            prop_assert_eq!(read.msg.len(), written.msg.len());
            for (m, w) in read.msg.iter().zip(&written.msg) {
                prop_assert_eq!(m.kind, kind(w.kind));
                prop_assert_eq!(&m.content, &w.content);
                prop_assert_eq!(m.condition.as_deref(), w.condition.as_deref());
            }
            let tags: Vec<&str> = read.tag.iter().map(|t| t.name.as_str()).collect();
            prop_assert_eq!(tags, written.tag.iter().map(String::as_str).collect::<Vec<_>>());
        }
    }

    // ---- anchors, aliases and merge keys -----------------------------------------------------

    #[test]
    fn an_alias_reads_the_same_as_the_node_it_points_to(m in message()) {
        let condition = m.condition.as_ref().map_or(String::new(), |c| format!(", condition: '{c}'"));
        let text = format!(
            "prelude:\n  - &m {{type: {}, content: '{}'{condition}}}\nglobals:\n  - *m\n  - *m\n",
            m.kind, m.content
        );
        let metadata = parse(&text).expect("parses");
        prop_assert_eq!(metadata.globals.len(), 2);
        prop_assert_eq!(&metadata.globals[0], &metadata.globals[1]);
        prop_assert_eq!(&metadata.globals[0].content, &m.content);
        prop_assert_eq!(metadata.globals[0].kind, kind(m.kind));
    }

    #[test]
    fn a_merge_key_supplies_what_the_node_does_not_say_and_explicit_keys_win(
        base in message(), own_kind in proptest::option::of(proptest::sample::select(vec!["say", "warn", "error"])),
        own_content in proptest::option::of(text()),
    ) {
        let mut text = format!("prelude:\n  - &base\n    type: {}\n    content: '{}'\n", base.kind, base.content);
        if let Some(c) = &base.condition { let _ = writeln!(text, "    condition: '{c}'"); }
        text.push_str("globals:\n  - <<: *base\n");
        if let Some(k) = own_kind { let _ = writeln!(text, "    type: {k}"); }
        if let Some(c) = &own_content { let _ = writeln!(text, "    content: '{c}'"); }
        let read = &parse(&text).expect("parses").globals[0];
        prop_assert_eq!(read.kind, kind(own_kind.unwrap_or(base.kind)));
        prop_assert_eq!(&read.content, own_content.as_ref().unwrap_or(&base.content));
        prop_assert_eq!(read.condition.as_deref(), base.condition.as_deref());
    }

    #[test]
    fn with_a_list_of_merge_sources_the_earlier_one_wins(first in text(), second in text(), third in text()) {
        let text = format!(
            "prelude:\n  - &a {{type: say, content: '{first}'}}\n  - &b {{type: warn, content: '{second}'}}\n  - &c {{type: error, content: '{third}', condition: 'file(\"c\")'}}\nglobals:\n  - <<: [*a, *b, *c]\n"
        );
        let read = &parse(&text).expect("parses").globals[0];
        prop_assert_eq!(&read.content, &first);
        prop_assert_eq!(read.kind, MessageKind::Say);
        prop_assert_eq!(read.condition.as_deref(), Some("file(\"c\")"), "only the last source has a condition");
    }

    // ---- the plugin name decides whether an entry is a regular expression --------------------

    #[test]
    fn a_name_with_a_regex_marker_matches_whole_names_only(stem in "[A-Za-z0-9]{1,8}", suffix in "[A-Za-z0-9]{1,4}") {
        let text = format!("plugins:\n  - name: '{stem}.*\\.esp'\n");
        let metadata = parse(&text).expect("parses");
        let pattern = &metadata.plugins[0].pattern;
        let upper = format!("{}{suffix}.ESP", stem.to_uppercase());
        let wrong_end = format!("{stem}{suffix}.espx");
        let wrong_start = format!("x{stem}{suffix}.esp");
        prop_assert!(pattern.matches(&upper));
        prop_assert!(!pattern.matches(&wrong_end));
        prop_assert!(!pattern.matches(&wrong_start));
    }
}
