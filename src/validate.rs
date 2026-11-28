// SPDX-License-Identifier: GPL-3.0-or-later
//
// The messages are the ones LOOT's application generates while it checks an install: `checkInstallValidity` and
// `validateActivePluginCounts` in `src/gui/state/game/validation.cpp`, `toSourcedMessage` in
// `src/gui/sourced_message.cpp`, and `readBashTagsFile` and `getTagConflicts` in `src/gui/state/game/helpers.cpp`
// of LOOT (https://github.com/loot/loot, GPL-3.0-or-later, Copyright (C) Oliver Hamlet), with their English texts,
// for Fallout: New Vegas: the checks of other games (light, medium, update and blueprint plugins, case-sensitive
// paths) are left out. See NOTICE.md.

//! The messages LOOT adds to a plugin when it checks the install, as opposed to the messages the masterlist holds:
//! a master that is missing or inactive, a plugin that is its own master, a header version below the game's
//! minimum, a group the masterlist does not define, Bash Tags that a `BashTags` file overrides, cleaning data that
//! matches the file, and requirements and incompatibilities.
//!
//! LOOT stores these as Markdown: plain text is escaped (every ASCII punctuation character gets a backslash) and
//! the text of the masterlist's `detail` is appended as it is. The texts here are the same Markdown, which is also
//! what the masterlist's own messages are.

use std::collections::{BTreeSet, HashSet};
use std::fs;
use std::io::Read;

use crate::check::{AppliedMessage, Evaluator};
use crate::env::FsEnv;
use crate::metadata::{Cleaning, FileRef, MessageKind, PluginEntry};
use crate::plugin::PluginHeader;

/// The most active plugins the game is safe with.
pub const SAFE_MAX_ACTIVE_PLUGINS: usize = 255;
/// The lowest header version the game supports; a plugin below it gets a warning.
pub const MINIMUM_HEADER_VERSION: f32 = 1.32;
/// Most bytes read from a `BashTags` file.
pub const MAX_BASH_TAGS_FILE: u64 = 64 * 1024;

/// Escape ASCII punctuation as `CommonMark` allows, as LOOT does for the texts it generates.
#[must_use]
pub fn escape_markdown(text: &str) -> String {
    let mut out = String::with_capacity(text.len() * 2);
    for character in text.chars() {
        if character.is_ascii_punctuation() {
            out.push('\\');
        }
        out.push(character);
    }
    out
}

fn say(kind: MessageKind, text: &str) -> AppliedMessage {
    AppliedMessage {
        lang: crate::metadata::DEFAULT_LANGUAGE.to_string(),
        kind,
        content: escape_markdown(text),
    }
}

/// The plugins of the load order are too many for the game: `count` active plugins.
#[must_use]
pub fn active_plugin_count(count: usize) -> Option<AppliedMessage> {
    (count > SAFE_MAX_ACTIVE_PLUGINS).then(|| {
        say(
            MessageKind::Warn,
            &format!("You have {count} active plugins but the game only supports up to {SAFE_MAX_ACTIVE_PLUGINS}."),
        )
    })
}

/// The tags of the plugin description: `{{BASH:Tag,Tag}}`, as libloot reads them.
#[must_use]
pub fn description_tags(description: &str) -> Vec<String> {
    description
        .split_once("{{BASH:")
        .and_then(|(_, rest)| rest.split_once("}}"))
        .map_or_else(Vec::new, |(tags, _)| {
            tags.split(',').map(|tag| tag.trim().to_string()).collect()
        })
}

/// What the checks need to know about one plugin.
pub struct Plugin<'a> {
    /// File name as given in the load order.
    pub name: &'a str,
    /// Whether the plugin is active.
    pub active: bool,
    /// Its header, when it could be read.
    pub header: Option<&'a PluginHeader>,
    /// The masterlist entries that match it.
    pub entries: &'a [&'a PluginEntry],
    /// The names of the groups the masterlist defines.
    pub groups: &'a HashSet<&'a str>,
    /// The group the masterlist names for the plugin, if it names one.
    pub group: Option<&'a str>,
    /// Cleaning data of the masterlist (`dirty`) whose checksum is the installed file's.
    pub dirty: &'a [&'a Cleaning],
}

/// The messages for one plugin, in the order LOOT adds them.
#[must_use]
pub fn plugin_messages(env: &FsEnv, plugin: &Plugin) -> Vec<AppliedMessage> {
    let mut ev = Evaluator::new(env);
    let mut out = Vec::new();
    if plugin.active {
        requirements(&mut ev, plugin, &mut out);
        incompatibilities(&mut ev, env, plugin, &mut out);
    }
    let loot_tags = tag_names(&mut ev, plugin);
    masters(env, plugin, &loot_tags, &mut out);
    if let Some(version) = plugin.header.and_then(|header| header.version)
        && version < MINIMUM_HEADER_VERSION
    {
        out.push(say(
            MessageKind::Warn,
            &format!(
                "This plugin has a header version of {version}, which is less than the game's minimum supported header version of {MINIMUM_HEADER_VERSION}."
            ),
        ));
    }
    if let Some(group) = plugin.group
        && !plugin.groups.contains(group)
    {
        out.push(say(
            MessageKind::Error,
            &format!("This plugin belongs to the group \"{group}\", which does not exist."),
        ));
    }
    if !loot_tags.is_empty() {
        let conflicts = tag_conflicts(&loot_tags, &bash_tags_file(env, plugin.name));
        if !conflicts.is_empty() {
            out.push(say(
                MessageKind::Say,
                &format!(
                    "This plugin has a BashTags file that will override the suggestions made by LOOT for the following Bash Tags: {}.",
                    conflicts.join(", ")
                ),
            ));
        }
    }
    out.extend(plugin.dirty.iter().map(|data| dirty_message(data)));
    out
}

/// The displayed name of a file entry, as LOOT keys its duplicates: the display text, else the escaped name.
fn display_key(file: &FileRef) -> String {
    file.display
        .clone()
        .unwrap_or_else(|| escape_markdown(&file.name))
}

fn with_detail(text: String, file: &FileRef) -> String {
    match &file.detail {
        Some(detail) => format!("{text} {detail}"),
        None => text,
    }
}

fn requirements(ev: &mut Evaluator, plugin: &Plugin, out: &mut Vec<AppliedMessage>) {
    let mut shown = HashSet::new();
    for file in plugin.entries.iter().flat_map(|entry| entry.req.iter()) {
        // Present only when the file exists and the constraint holds.
        if ev.file_state(file) != Some((true, false)) || !shown.insert(display_key(file)) {
            continue;
        }
        let text = match &file.display {
            Some(display) => {
                format!("This plugin requires {display} to be installed, but it is missing.")
            }
            None => format!(
                "This plugin requires \"{}\" to be installed, but it is missing.",
                escape_markdown(&file.name)
            ),
        };
        out.push(AppliedMessage {
            lang: crate::metadata::DEFAULT_LANGUAGE.to_string(),
            kind: MessageKind::Error,
            content: with_detail(text, file),
        });
    }
}

fn has_plugin_extension(name: &str) -> bool {
    let lower = name.to_lowercase();
    [
        ".esp",
        ".esm",
        ".esl",
        ".omwaddon",
        ".omwgame",
        ".omwscripts",
    ]
    .iter()
    .any(|extension| lower.ends_with(extension))
}

fn incompatibilities(
    ev: &mut Evaluator,
    env: &FsEnv,
    plugin: &Plugin,
    out: &mut Vec<AppliedMessage>,
) {
    let mut shown = HashSet::new();
    for file in plugin.entries.iter().flat_map(|entry| entry.inc.iter()) {
        // The entry counts when its condition holds; the file is there, and a plugin only when it is active.
        let in_force = ev.file_state(file) == Some((true, true));
        let hits = in_force && (!has_plugin_extension(&file.name) || env.is_active(&file.name));
        if !hits || !shown.insert(display_key(file)) {
            continue;
        }
        let text = match &file.display {
            Some(display) => {
                format!("This plugin is incompatible with {display}, but both are present.")
            }
            None => format!(
                "This plugin is incompatible with \"{}\", but both are present.",
                escape_markdown(&file.name)
            ),
        };
        out.push(AppliedMessage {
            lang: crate::metadata::DEFAULT_LANGUAGE.to_string(),
            kind: MessageKind::Error,
            content: with_detail(text, file),
        });
    }
}

/// The tags (additions and removals, by name) that apply to the plugin.
pub(crate) fn tag_names(ev: &mut Evaluator, plugin: &Plugin) -> Vec<(String, bool)> {
    let mut tags = Vec::new();
    for tag in plugin.entries.iter().flat_map(|entry| entry.tag.iter()) {
        if ev.holds(tag.condition.as_deref()) == Some(true) {
            match tag.name.strip_prefix('-') {
                Some(name) => tags.push((name.to_string(), false)),
                None => tags.push((tag.name.clone(), true)),
            }
        }
    }
    tags
}

/// The plugin's masters that are not installed or not active, in header order.
///
/// The boolean is `true` when the master is installed but inactive, and `false` when it is not installed.
/// The result is empty when the plugin is inactive or has the `Filter` tag.
pub(crate) fn missing_masters(
    env: &FsEnv,
    plugin: &Plugin,
    loot_tags: &[(String, bool)],
) -> Vec<(String, bool)> {
    let Some(header) = plugin.header else {
        return Vec::new();
    };
    let description_filter = description_tags(header.description.as_deref().unwrap_or_default())
        .iter()
        .any(|tag| tag == "Filter");
    let filter = description_filter || loot_tags.iter().any(|(name, _)| name == "Filter");
    let expect_active = plugin.active && !filter;
    if !expect_active {
        return Vec::new();
    }
    header
        .masters
        .iter()
        .filter_map(|master| {
            if !env.installed(master) {
                Some((master.clone(), false))
            } else if !env.is_active(master) {
                Some((master.clone(), true))
            } else {
                None
            }
        })
        .collect()
}

fn masters(
    env: &FsEnv,
    plugin: &Plugin,
    loot_tags: &[(String, bool)],
    out: &mut Vec<AppliedMessage>,
) {
    let Some(header) = plugin.header else {
        return;
    };
    for (master, installed) in missing_masters(env, plugin, loot_tags) {
        let text = if installed {
            format!("This plugin requires \"{master}\" to be active, but it is inactive.")
        } else {
            format!("This plugin requires \"{master}\" to be installed, but it is missing.")
        };
        out.push(say(MessageKind::Error, &text));
    }
    for master in &header.masters {
        if plugin.name.to_lowercase() == master.to_lowercase() {
            out.push(say(
                MessageKind::Error,
                "This plugin has itself as a master.",
            ));
        }
    }
}

/// The tags in `Data/BashTags/<plugin without its extension>.txt`: comma-separated, `#` starts a comment, a leading
/// `-` is a removal. A file that is missing, unreadable or over [`MAX_BASH_TAGS_FILE`] bytes gives none.
fn bash_tags_file(env: &FsEnv, plugin_name: &str) -> Vec<(String, bool)> {
    let stem_length = plugin_name.chars().count().saturating_sub(4);
    let stem: String = plugin_name.chars().take(stem_length).collect();
    let Some(path) = env.resolve(&format!("BashTags/{stem}.txt")) else {
        return Vec::new();
    };
    let mut text = String::new();
    let read = fs::File::open(path)
        .and_then(|file| file.take(MAX_BASH_TAGS_FILE).read_to_string(&mut text));
    if read.is_err() {
        return Vec::new();
    }
    let mut tags = Vec::new();
    for line in text.lines() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.split('#').next().unwrap_or_default();
        for entry in line
            .split(',')
            .map(str::trim)
            .filter(|entry| !entry.is_empty())
        {
            match entry.strip_prefix('-') {
                Some(name) => tags.push((name.to_string(), false)),
                None => tags.push((entry.to_string(), true)),
            }
        }
    }
    tags
}

/// Tags that one side adds and the other removes, sorted.
fn tag_conflicts(a: &[(String, bool)], b: &[(String, bool)]) -> Vec<String> {
    let names = |tags: &[(String, bool)], addition: bool| -> BTreeSet<String> {
        tags.iter()
            .filter(|(_, add)| *add == addition)
            .map(|(name, _)| name.clone())
            .collect()
    };
    let mut conflicts: Vec<String> = names(a, true)
        .intersection(&names(b, false))
        .chain(names(b, true).intersection(&names(a, false)))
        .cloned()
        .collect();
    conflicts.sort();
    conflicts
}

fn plural(count: u32, one: &str, many: &str) -> String {
    format!("{count} {}", if count == 1 { one } else { many })
}

/// The message for cleaning data that matches the plugin: what the utility found, then the data's `detail`.
#[must_use]
pub fn dirty_message(data: &Cleaning) -> AppliedMessage {
    let itm = plural(data.itm, "ITM record", "ITM records");
    let udr = plural(data.udr, "deleted reference", "deleted references");
    let nav = plural(data.nav, "deleted navmesh", "deleted navmeshes");
    let util = &data.util;
    let mut text = match (data.itm > 0, data.udr > 0, data.nav > 0) {
        (true, true, true) => format!("{util} found {itm}, {udr} and {nav}."),
        (false, false, false) => format!("{util} found dirty edits."),
        (false, true, true) => format!("{util} found {udr} and {nav}."),
        (true, false, true) => format!("{util} found {itm} and {nav}."),
        (true, true, false) => format!("{util} found {itm} and {udr}."),
        (true, false, false) => format!("{util} found {itm}."),
        (false, true, false) => format!("{util} found {udr}."),
        (false, false, true) => format!("{util} found {nav}."),
    };
    if let Some(detail) = &data.detail {
        text = format!("{text} {detail}");
    }
    AppliedMessage {
        lang: crate::metadata::DEFAULT_LANGUAGE.to_string(),
        kind: MessageKind::Warn,
        content: text,
    }
}

#[cfg(test)]
#[path = "validate_tests.rs"]
mod tests;
