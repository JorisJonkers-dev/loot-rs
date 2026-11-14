//! Unit tests of `render`.

use super::*;
use crate::check::{Finding, FindingKind, PluginReport};
use crate::metadata::MessageKind;

fn message(kind: MessageKind, content: &str) -> AppliedMessage {
    AppliedMessage {
        kind,
        content: content.into(),
    }
}

fn finding(kind: FindingKind, text: &str, hard: bool) -> Finding {
    Finding {
        kind,
        text: text.into(),
        hard,
    }
}

fn plugin(name: &str, position: usize) -> PluginReport {
    PluginReport {
        name: name.into(),
        position,
        active: true,
        group: "default".into(),
        messages: Vec::new(),
        tags_add: Vec::new(),
        tags_remove: Vec::new(),
        findings: Vec::new(),
        entries_matched: 0,
    }
}

#[test]
fn an_empty_report_is_one_summary_line() {
    assert_eq!(
        render(&Report::default()),
        "Checked 0 plugin(s), 0 with masterlist entries. Hard violations: 0.\n"
    );
}

#[test]
fn quiet_plugins_are_not_listed_but_are_counted() {
    let mut report = Report::default();
    report.plugins.push(plugin("Quiet.esp", 0));
    let mut matched = plugin("Matched.esp", 1);
    matched.entries_matched = 1;
    report.plugins.push(matched);
    let out = render(&report);
    assert!(!out.contains("Quiet.esp"), "{out}");
    assert!(out.ends_with("Checked 2 plugin(s), 1 with masterlist entries. Hard violations: 0.\n"));
}

#[test]
fn global_messages_come_first_with_their_kind() {
    let mut report = Report::default();
    report.globals.push(message(MessageKind::Warn, "careful"));
    report
        .globals
        .push(message(MessageKind::Say, "line one\nline two"));
    let out = render(&report);
    assert!(
        out.starts_with(
            "Global messages\n  [warn] careful\n  [say] line one\n        line two\n\n"
        ),
        "{out}"
    );
}

#[test]
fn a_plugin_block_has_a_header_messages_tags_and_findings() {
    let mut entry = plugin("A.esp", 3);
    entry.group = "Late".into();
    entry.active = false;
    entry
        .messages
        .push(message(MessageKind::Error, "  padded  "));
    entry.tags_add = vec!["Relev".into(), "Names".into()];
    entry.tags_remove = vec!["Delev".into()];
    entry.findings.push(finding(
        FindingKind::MissingRequirement,
        "requires X, which is not present.",
        true,
    ));
    entry.findings.push(finding(
        FindingKind::Unsupported,
        "unsupported condition function version()",
        false,
    ));
    let report = Report {
        plugins: vec![entry],
        ..Report::default()
    };
    assert_eq!(
        render(&report),
        "A.esp (position 3, group \"Late\", inactive)\n\
         \x20 [error] padded\n\
         \x20 tags: +Relev +Names -Delev\n\
         \x20 VIOLATION missing requirement: requires X, which is not present.\n\
         \x20 note unsupported: unsupported condition function version()\n\
         \n\
         note: 1 item(s) use the unsupported condition function version() and were not applied\n\
         Checked 1 plugin(s), 0 with masterlist entries. Hard violations: 1.\n"
    );
}

#[test]
fn an_active_plugin_has_no_inactive_marker() {
    let mut entry = plugin("A.esp", 0);
    entry.tags_add = vec!["Relev".into()];
    let report = Report {
        plugins: vec![entry],
        ..Report::default()
    };
    assert!(render(&report).starts_with("A.esp (position 0, group \"default\")\n  tags: +Relev\n"));
}

#[test]
fn group_order_findings_are_cut_after_five_per_plugin() {
    let mut entry = plugin("A.esp", 0);
    for index in 0..8 {
        entry.findings.push(finding(
            FindingKind::GroupOrder,
            &format!("g{index}"),
            false,
        ));
    }
    entry.findings.push(finding(
        FindingKind::Incompatible,
        "after the group lines",
        true,
    ));
    let out = render(&Report {
        plugins: vec![entry],
        ..Report::default()
    });
    assert_eq!(out.matches("note group order: g").count(), 5, "{out}");
    assert!(
        out.contains("note group order: and 3 more group-order findings for this plugin"),
        "{out}"
    );
    assert!(
        out.contains("VIOLATION incompatible: after the group lines"),
        "{out}"
    );
    assert!(!out.contains("g5"), "{out}");
}

#[test]
fn exactly_five_group_order_findings_are_all_shown_without_a_summary() {
    let mut entry = plugin("A.esp", 0);
    for index in 0..5 {
        entry.findings.push(finding(
            FindingKind::GroupOrder,
            &format!("g{index}"),
            false,
        ));
    }
    let out = render(&Report {
        plugins: vec![entry],
        ..Report::default()
    });
    assert_eq!(out.matches("note group order: g").count(), 5);
    assert!(!out.contains("more group-order"));
}

#[test]
fn general_findings_warnings_and_missing_plugins_are_noted() {
    let report = Report {
        general: vec![finding(FindingKind::ConditionError, "bad condition", false)],
        metadata_warnings: vec!["plugin X: regular expression skipped".into()],
        not_installed: vec!["A.esp".into(), "B.esp".into()],
        ..Report::default()
    };
    assert_eq!(
        render(&report),
        "note condition error: bad condition\n\
         note masterlist: plugin X: regular expression skipped\n\
         note: 2 listed plugin(s) are not in the Data directory and were not checked: A.esp, B.esp\n\
         Checked 0 plugin(s), 0 with masterlist entries. Hard violations: 0.\n"
    );
}

#[test]
fn unsupported_functions_are_summarised_by_name_in_sorted_order() {
    let mut entry = plugin("A.esp", 0);
    for text in [
        "unsupported condition function version() (in `x`), item not applied",
        "unsupported condition function is_executable() (in `y`), item not applied",
        "unsupported condition function version() (in `z`), item not applied",
    ] {
        entry
            .findings
            .push(finding(FindingKind::Unsupported, text, false));
    }
    let out = render(&Report {
        plugins: vec![entry],
        ..Report::default()
    });
    let executable = out
        .find("2 item(s) use the unsupported condition function version()")
        .expect("version line");
    let version = out
        .find("1 item(s) use the unsupported condition function is_executable()")
        .expect("executable line");
    assert!(version < executable, "sorted by name: {out}");
}

#[test]
fn json_of_an_empty_report() {
    assert_eq!(
        render_json(&Report::default()),
        "{\"schema_version\":1,\"contract\":\"loot-check\",\"general_messages\":[],\"plugins\":[],\"general_findings\":[],\"not_installed\":[],\"warnings\":[],\"hard_violations\":0}\n"
    );
}

#[test]
fn json_strings_are_escaped() {
    let mut out = String::new();
    json_string(
        &mut out,
        "quote \" backslash \\ newline \n return \r tab \t bell \u{7} é",
    );
    assert_eq!(
        out,
        "\"quote \\\" backslash \\\\ newline \\n return \\r tab \\t bell \\u0007 é\""
    );
}

#[test]
fn json_of_a_full_report_lists_everything_in_load_order() {
    let mut entry = plugin("A.esp", 2);
    entry.active = false;
    entry.group = "Late".into();
    entry
        .messages
        .push(message(MessageKind::Warn, "  careful \"now\"  "));
    entry.tags_add = vec!["Relev".into()];
    entry.tags_remove = vec!["Delev".into()];
    entry.findings.push(finding(
        FindingKind::Incompatible,
        "is incompatible with B.esp",
        true,
    ));
    let report = Report {
        globals: vec![message(MessageKind::Say, "hello")],
        plugins: vec![entry, plugin("B.esp", 3)],
        general: vec![finding(FindingKind::ConditionError, "bad", false)],
        not_installed: vec!["Gone.esp".into()],
        metadata_warnings: vec!["skipped".into()],
    };
    assert_eq!(
        render_json(&report),
        concat!(
            "{\"schema_version\":1,\"contract\":\"loot-check\",\"general_messages\":[{\"type\":\"say\",\"text\":\"hello\"}],",
            "\"plugins\":[{\"name\":\"A.esp\",\"position\":2,\"active\":false,\"group\":\"Late\",",
            "\"messages\":[{\"type\":\"warn\",\"text\":\"careful \\\"now\\\"\"}],",
            "\"tags_add\":[\"Relev\"],\"tags_remove\":[\"Delev\"],",
            "\"findings\":[{\"kind\":\"incompatible\",\"hard\":true,\"text\":\"is incompatible with B.esp\"}]},",
            "{\"name\":\"B.esp\",\"position\":3,\"active\":true,\"group\":\"default\",\"messages\":[],",
            "\"tags_add\":[],\"tags_remove\":[],\"findings\":[]}],",
            "\"general_findings\":[{\"kind\":\"condition error\",\"hard\":false,\"text\":\"bad\"}],",
            "\"not_installed\":[\"Gone.esp\"],\"warnings\":[\"skipped\"],\"hard_violations\":1}\n"
        )
    );
}
