//! Plain-text rendering of a [`Report`].

use crate::api::SortOutput;
use crate::check::{AppliedMessage, Finding, FindingKind, Report};
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

fn json_string(out: &mut String, text: &str) {
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if u32::from(c) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", u32::from(c));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

fn json_list<T>(out: &mut String, items: &[T], mut each: impl FnMut(&mut String, &T)) {
    out.push('[');
    for (index, item) in items.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        each(out, item);
    }
    out.push(']');
}

fn json_message(out: &mut String, message: &AppliedMessage) {
    out.push_str("{\"type\":");
    json_string(out, message.kind.as_str());
    out.push_str(",\"text\":");
    json_string(out, message.content.trim());
    out.push('}');
}

fn json_finding(out: &mut String, finding: &Finding) {
    out.push_str("{\"kind\":");
    json_string(out, finding.kind.label());
    let _ = write!(out, ",\"hard\":{},\"text\":", finding.hard);
    json_string(out, &finding.text);
    out.push('}');
}

fn json_strings(out: &mut String, items: &[String]) {
    json_list(out, items, |out, item| json_string(out, item));
}

/// Render the report as one line of JSON, for tools that compare it with another implementation.
///
/// Keys: `general_messages`, `plugins` (each with `name`, `position`, `active`, `group`, `messages`,
/// `tags_add`, `tags_remove`, `findings`), `general_findings`, `not_installed`, `warnings` and
/// `hard_violations`. Everything is in load order and the text is the same as in [`render`].
#[must_use]
pub fn render_json(report: &Report) -> String {
    let mut out = String::from("{\"general_messages\":");
    json_list(&mut out, &report.globals, json_message);
    out.push_str(",\"plugins\":");
    json_list(&mut out, &report.plugins, |out, plugin| {
        out.push_str("{\"name\":");
        json_string(out, &plugin.name);
        let _ = write!(
            out,
            ",\"position\":{},\"active\":{},\"group\":",
            plugin.position, plugin.active
        );
        json_string(out, &plugin.group);
        out.push_str(",\"messages\":");
        json_list(out, &plugin.messages, json_message);
        out.push_str(",\"tags_add\":");
        json_strings(out, &plugin.tags_add);
        out.push_str(",\"tags_remove\":");
        json_strings(out, &plugin.tags_remove);
        out.push_str(",\"findings\":");
        json_list(out, &plugin.findings, json_finding);
        out.push('}');
    });
    out.push_str(",\"general_findings\":");
    json_list(&mut out, &report.general, json_finding);
    out.push_str(",\"not_installed\":");
    json_strings(&mut out, &report.not_installed);
    out.push_str(",\"warnings\":");
    json_strings(&mut out, &report.metadata_warnings);
    let _ = writeln!(out, ",\"hard_violations\":{}}}", report.hard_count());
    out
}

/// The sorted load order as text: the order, then what applies to each plugin that has something to say.
#[must_use]
pub fn render_sort(result: &SortOutput) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "Sorted load order ({} plugin(s)):", result.order.len());
    for (index, plugin) in result.plugins.iter().enumerate() {
        let _ = writeln!(
            out,
            "{:>4}. {}{}",
            index + 1,
            plugin.name,
            if plugin.active { "" } else { " (inactive)" }
        );
    }
    for message in &result.general_messages {
        let _ = writeln!(
            out,
            "\n{}: {}",
            message_label(message.kind),
            message.text.trim()
        );
    }
    for plugin in &result.plugins {
        let quiet = plugin.messages.is_empty()
            && plugin.tags_added.is_empty()
            && plugin.tags_removed.is_empty()
            && plugin.findings.is_empty();
        if quiet {
            continue;
        }
        let _ = writeln!(out, "\n{} (group \"{}\")", plugin.name, plugin.group);
        for message in &plugin.messages {
            let _ = writeln!(
                out,
                "  {}: {}",
                message_label(message.kind),
                message.text.trim()
            );
        }
        for tag in &plugin.tags_added {
            let _ = writeln!(out, "  tag +{tag}");
        }
        for tag in &plugin.tags_removed {
            let _ = writeln!(out, "  tag -{tag}");
        }
        for finding in &plugin.findings {
            let _ = writeln!(
                out,
                "  {} {}: {}",
                if finding.hard { "VIOLATION" } else { "note" },
                finding_label(finding.kind),
                finding.text
            );
        }
    }
    for (later, earlier) in &result.dropped_group_rules {
        let _ = writeln!(
            out,
            "\nnote: group rule dropped: {later} should load after {earlier}"
        );
    }
    for plugin in &result.not_installed {
        let _ = writeln!(out, "note: {plugin} is listed but not installed, left out");
    }
    for warning in &result.warnings {
        let _ = writeln!(out, "warning: {warning}");
    }
    out
}

fn message_label(kind: crate::api::MessageKind) -> &'static str {
    match kind {
        crate::api::MessageKind::Say => "say",
        crate::api::MessageKind::Warn => "warn",
        crate::api::MessageKind::Error => "error",
    }
}

fn finding_label(kind: crate::api::FindingKind) -> &'static str {
    use crate::api::FindingKind as Kind;
    match kind {
        Kind::MissingRequirement => FindingKind::MissingRequirement.label(),
        Kind::Incompatible => FindingKind::Incompatible.label(),
        Kind::LoadAfter => FindingKind::LoadAfter.label(),
        Kind::GroupOrder => FindingKind::GroupOrder.label(),
        Kind::UndefinedGroup => FindingKind::UndefinedGroup.label(),
        Kind::Unsupported => FindingKind::Unsupported.label(),
        Kind::ConditionError => FindingKind::ConditionError.label(),
    }
}

/// The result of a sort as one line of JSON, in the shape of [`render_json`] plus the sort.
///
/// Keys: `sorted` (names in order), `general_messages`, `plugins` (each with `name`, `position`, `active`, `group`,
/// `messages`, `tags_add`, `tags_remove`, `findings`), `dirty`, `rules` (`before`, `after`, `source`),
/// `dropped_group_rules` (`later`, `earlier`), `not_installed`, `warnings`, `hard_violations`.
#[must_use]
pub fn render_sort_json(result: &SortOutput) -> String {
    let mut out = String::from("{\"sorted\":");
    json_strings(&mut out, &result.order);
    out.push_str(",\"general_messages\":");
    json_list(&mut out, &result.general_messages, json_api_message);
    out.push_str(",\"plugins\":");
    json_list(&mut out, &result.plugins, |out, plugin| {
        out.push_str("{\"name\":");
        json_string(out, &plugin.name);
        let _ = write!(
            out,
            ",\"position\":{},\"active\":{},\"group\":",
            plugin.position, plugin.active
        );
        json_string(out, &plugin.group);
        out.push_str(",\"messages\":");
        json_list(out, &plugin.messages, json_api_message);
        out.push_str(",\"tags_add\":");
        json_strings(out, &plugin.tags_added);
        out.push_str(",\"tags_remove\":");
        json_strings(out, &plugin.tags_removed);
        out.push_str(",\"findings\":");
        json_list(out, &plugin.findings, |out, finding| {
            out.push_str("{\"kind\":");
            json_string(out, finding_label(finding.kind));
            let _ = write!(out, ",\"hard\":{},\"text\":", finding.hard);
            json_string(out, &finding.text);
            out.push('}');
        });
        out.push('}');
    });
    out.push_str(",\"dirty\":");
    json_list(&mut out, &result.dirty, |out, info| {
        out.push_str("{\"plugin\":");
        json_string(out, &info.plugin);
        let status = match info.status {
            crate::api::CleaningStatus::Dirty => "dirty",
            crate::api::CleaningStatus::Clean => "clean",
        };
        let _ = write!(
            out,
            ",\"crc\":{},\"status\":\"{status}\",\"utility\":",
            info.crc
        );
        json_string(out, &info.utility);
        let _ = write!(
            out,
            ",\"itm\":{},\"udr\":{},\"nav\":{}}}",
            info.itm, info.udr, info.nav
        );
    });
    out.push_str(",\"rules\":");
    json_list(&mut out, &result.ordering_rules, |out, rule| {
        out.push_str("{\"before\":");
        json_string(out, &rule.before);
        out.push_str(",\"after\":");
        json_string(out, &rule.after);
        out.push_str(",\"source\":");
        json_string(out, rule.source.label());
        out.push('}');
    });
    out.push_str(",\"dropped_group_rules\":");
    json_list(
        &mut out,
        &result.dropped_group_rules,
        |out, (later, earlier)| {
            out.push_str("{\"later\":");
            json_string(out, later);
            out.push_str(",\"earlier\":");
            json_string(out, earlier);
            out.push('}');
        },
    );
    out.push_str(",\"not_installed\":");
    json_strings(&mut out, &result.not_installed);
    out.push_str(",\"warnings\":");
    json_strings(&mut out, &result.warnings);
    let hard = result
        .plugins
        .iter()
        .flat_map(|plugin| &plugin.findings)
        .filter(|finding| finding.hard)
        .count();
    let _ = writeln!(out, ",\"hard_violations\":{hard}}}");
    out
}

fn json_api_message(out: &mut String, message: &crate::api::Message) {
    out.push_str("{\"type\":");
    json_string(out, message_label(message.kind));
    out.push_str(",\"text\":");
    json_string(out, message.text.trim());
    out.push('}');
}

#[cfg(test)]
#[path = "render_tests.rs"]
mod tests;
