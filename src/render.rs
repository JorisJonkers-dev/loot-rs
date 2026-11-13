//! Plain-text rendering of a [`Report`].

use crate::check::{AppliedMessage, FindingKind, Report};
use std::fmt::Write;

/// Group-order findings printed per plugin before the rest are summarised.
const GROUP_LINES_PER_PLUGIN: usize = 5;

fn message_lines(out: &mut String, indent: &str, message: &AppliedMessage) {
    let mut lines = message.content.trim().lines();
    let _ = writeln!(
        out,
        "{indent}[{}] {}",
        message.kind.as_str(),
        lines.next().unwrap_or("")
    );
    for line in lines {
        let _ = writeln!(out, "{indent}      {line}");
    }
}

/// Render the report as text.
#[must_use]
pub fn render(report: &Report) -> String {
    let mut out = String::new();
    if !report.globals.is_empty() {
        out.push_str("Global messages\n");
        for message in &report.globals {
            message_lines(&mut out, "  ", message);
        }
        out.push('\n');
    }
    for plugin in &report.plugins {
        let quiet = plugin.messages.is_empty()
            && plugin.tags_add.is_empty()
            && plugin.tags_remove.is_empty()
            && plugin.findings.is_empty();
        if quiet {
            continue;
        }
        let _ = writeln!(
            out,
            "{} (position {}, group \"{}\"{})",
            plugin.name,
            plugin.position,
            plugin.group,
            if plugin.active { "" } else { ", inactive" }
        );
        for message in &plugin.messages {
            message_lines(&mut out, "  ", message);
        }
        if !plugin.tags_add.is_empty() || !plugin.tags_remove.is_empty() {
            let tags: Vec<String> = plugin
                .tags_add
                .iter()
                .map(|tag| format!("+{tag}"))
                .chain(plugin.tags_remove.iter().map(|tag| format!("-{tag}")))
                .collect();
            let _ = writeln!(out, "  tags: {}", tags.join(" "));
        }
        let mut group_lines = 0;
        for finding in &plugin.findings {
            if finding.kind == FindingKind::GroupOrder {
                group_lines += 1;
                if group_lines > GROUP_LINES_PER_PLUGIN {
                    continue;
                }
            }
            let _ = writeln!(
                out,
                "  {} {}: {}",
                if finding.hard { "VIOLATION" } else { "note" },
                finding.kind.label(),
                finding.text
            );
        }
        if group_lines > GROUP_LINES_PER_PLUGIN {
            let _ = writeln!(
                out,
                "  note group order: and {} more group-order findings for this plugin",
                group_lines - GROUP_LINES_PER_PLUGIN
            );
        }
        out.push('\n');
    }
    for finding in &report.general {
        let _ = writeln!(out, "note {}: {}", finding.kind.label(), finding.text);
    }
    for warning in &report.metadata_warnings {
        let _ = writeln!(out, "note masterlist: {warning}");
    }
    if !report.not_installed.is_empty() {
        let _ = writeln!(
            out,
            "note: {} listed plugin(s) are not in the Data directory and were not checked: {}",
            report.not_installed.len(),
            report.not_installed.join(", ")
        );
    }
    for (name, count) in report.unsupported_functions() {
        let _ = writeln!(
            out,
            "note: {count} item(s) use the unsupported condition function {name}() and were not applied"
        );
    }
    let matched = report
        .plugins
        .iter()
        .filter(|plugin| plugin.entries_matched > 0)
        .count();
    let _ = writeln!(
        out,
        "Checked {} plugin(s), {} with masterlist entries. Hard violations: {}.",
        report.plugins.len(),
        matched,
        report.hard_count()
    );
    out
}

#[cfg(test)]
#[path = "render_tests.rs"]
mod tests;
