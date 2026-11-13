//! Apply masterlist metadata to a load order and report what applies and what is violated.
//!
//! Nothing here sorts or writes. For every plugin of the load order that is installed in the
//! `Data` directory, the entries whose name matches it are merged in file order (first `group`
//! wins, everything else is appended) and then:
//!
//! * `msg` and `tag` entries are kept when their condition holds,
//! * `req` files that are not present are reported (hard); a required plugin that loads after
//!   the plugin is a load-after violation (hard),
//! * `inc` files that are present are reported (hard),
//! * `after` plugins that load after the plugin are reported (hard),
//! * group relations are checked pairwise (soft unless [`Options::strict_groups`]).
//!
//! An item whose condition cannot be evaluated (an unsupported function, an unreadable
//! directory) is not applied; a [`FindingKind::Unsupported`] or [`FindingKind::ConditionError`]
//! finding records it, so a missing result is never silent.

use crate::condition::{ConditionError, Env, eval_str};
use crate::env::FsEnv;
use crate::metadata::{DEFAULT_GROUP, FileRef, Message, MessageKind, Metadata, PluginEntry};
use std::collections::{HashMap, HashSet};

/// What a finding is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FindingKind {
    /// A `req` file is missing.
    MissingRequirement,
    /// An `inc` file is present.
    Incompatible,
    /// An `after` (or `req`) plugin loads after the plugin that must come after it.
    LoadAfter,
    /// A group relation is violated.
    GroupOrder,
    /// The plugin's group is not defined.
    UndefinedGroup,
    /// A condition used a function this evaluator does not implement.
    Unsupported,
    /// A condition was invalid or could not be evaluated.
    ConditionError,
}

impl FindingKind {
    /// Short label used in the output.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::MissingRequirement => "missing requirement",
            Self::Incompatible => "incompatible",
            Self::LoadAfter => "load after",
            Self::GroupOrder => "group order",
            Self::UndefinedGroup => "undefined group",
            Self::Unsupported => "unsupported",
            Self::ConditionError => "condition error",
        }
    }
}

/// One problem found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    /// Category.
    pub kind: FindingKind,
    /// Human-readable description.
    pub text: String,
    /// Whether it makes the check fail (exit code 1).
    pub hard: bool,
}

/// A message that applies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppliedMessage {
    /// Severity.
    pub kind: MessageKind,
    /// English text.
    pub content: String,
}

/// Everything found for one plugin.
#[derive(Debug, Clone)]
pub struct PluginReport {
    /// File name as listed in the plugin list.
    pub name: String,
    /// Position in the load order, from 0.
    pub position: usize,
    /// Whether the plugin is active.
    pub active: bool,
    /// Group the plugin is in.
    pub group: String,
    /// Messages whose condition holds.
    pub messages: Vec<AppliedMessage>,
    /// Bash Tags suggested for addition.
    pub tags_add: Vec<String>,
    /// Bash Tags suggested for removal (without the `-`).
    pub tags_remove: Vec<String>,
    /// Problems.
    pub findings: Vec<Finding>,
    /// Number of metadata entries that matched the plugin.
    pub entries_matched: usize,
}

/// The result of a check.
#[derive(Debug, Clone, Default)]
pub struct Report {
    /// Global messages whose condition holds.
    pub globals: Vec<AppliedMessage>,
    /// One entry per installed plugin in the list, in load order.
    pub plugins: Vec<PluginReport>,
    /// Findings not tied to a plugin (global message conditions).
    pub general: Vec<Finding>,
    /// Listed plugins that are not in the `Data` directory.
    pub not_installed: Vec<String>,
    /// Metadata entries that were skipped while reading.
    pub metadata_warnings: Vec<String>,
}

impl Report {
    /// All findings, per plugin and general.
    pub fn findings(&self) -> impl Iterator<Item = &Finding> {
        self.general.iter().chain(
            self.plugins
                .iter()
                .flat_map(|plugin| plugin.findings.iter()),
        )
    }

    /// Number of findings that fail the check.
    #[must_use]
    pub fn hard_count(&self) -> usize {
        self.findings().filter(|finding| finding.hard).count()
    }

    /// Names of the condition functions that were not evaluated, with how often.
    #[must_use]
    pub fn unsupported_functions(&self) -> Vec<(String, usize)> {
        let mut counts: HashMap<String, usize> = HashMap::new();
        for finding in self
            .findings()
            .filter(|finding| finding.kind == FindingKind::Unsupported)
        {
            if let Some(name) = finding
                .text
                .split("()")
                .next()
                .and_then(|head| head.rsplit(' ').next().map(str::to_string))
            {
                *counts.entry(name).or_default() += 1;
            }
        }
        let mut counts: Vec<_> = counts.into_iter().collect();
        counts.sort();
        counts
    }
}

/// Switches for [`run`].
#[derive(Debug, Clone, Copy, Default)]
pub struct Options {
    /// Count group violations and undefined groups as hard.
    pub strict_groups: bool,
}

struct Evaluator<'a> {
    env: &'a FsEnv,
    findings: Vec<Finding>,
}

impl Evaluator<'_> {
    /// Evaluate an optional condition. `None` (absent) is true; an unevaluable condition is
    /// recorded as a finding and returns `None`.
    fn holds(&mut self, condition: Option<&str>) -> Option<bool> {
        let Some(text) = condition else {
            return Some(true);
        };
        match eval_str(text, self.env) {
            Ok(value) => Some(value),
            Err(error) => {
                let kind = if matches!(error, ConditionError::Unsupported(_)) {
                    FindingKind::Unsupported
                } else {
                    FindingKind::ConditionError
                };
                self.findings.push(Finding {
                    kind,
                    text: format!("{error} (in `{}`), item not applied", text.trim()),
                    hard: false,
                });
                None
            }
        }
    }

    fn messages(&mut self, messages: &[&Message]) -> Vec<AppliedMessage> {
        messages
            .iter()
            .filter(|message| self.holds(message.condition.as_deref()) == Some(true))
            .map(|message| AppliedMessage {
                kind: message.kind,
                content: message.content.clone(),
            })
            .collect()
    }

    /// True when the file entry is in force and its target is present.
    /// Returns `None` when the entry could not be evaluated, `Some(false)` when it is not in
    /// force, and `Some(true)` with the presence in the second field otherwise.
    fn file_state(&mut self, file: &FileRef) -> Option<(bool, bool)> {
        if !self.holds(file.condition.as_deref())? {
            return Some((false, false));
        }
        let mut present = match self.env.count_files(&file.name) {
            Ok(count) => count > 0,
            Err(error) => {
                self.findings.push(Finding {
                    kind: FindingKind::ConditionError,
                    text: format!("{} ({}), item not applied", error, file.name),
                    hard: false,
                });
                return None;
            }
        };
        if present && file.constraint.is_some() {
            present = self.holds(file.constraint.as_deref())?;
        }
        Some((true, present))
    }
}

/// Transitive closure of "loads after" over the merged group definitions.
fn group_closure(metadata: &Metadata) -> HashMap<String, HashSet<String>> {
    let direct: HashMap<&str, &[String]> = metadata
        .groups
        .iter()
        .map(|group| (group.name.as_str(), group.after.as_slice()))
        .collect();
    let mut closure = HashMap::new();
    for group in &metadata.groups {
        let mut seen: HashSet<String> = HashSet::new();
        let mut stack: Vec<&str> = direct[group.name.as_str()]
            .iter()
            .map(String::as_str)
            .collect();
        while let Some(next) = stack.pop() {
            if seen.insert(next.to_string())
                && let Some(more) = direct.get(next)
            {
                stack.extend(more.iter().map(String::as_str));
            }
        }
        closure.insert(group.name.clone(), seen);
    }
    closure
}

struct Run<'a> {
    env: &'a FsEnv,
    options: Options,
    closure: HashMap<String, HashSet<String>>,
    defined: HashSet<&'a str>,
    installed: Vec<(usize, &'a str)>,
    groups_of: HashMap<usize, String>,
}

impl Run<'_> {
    fn file_rules(&self, ev: &mut Evaluator, index: usize, entries: &[&PluginEntry]) {
        let env = self.env;
        for file in entries.iter().flat_map(|entry| entry.req.iter()) {
            match ev.file_state(file) {
                Some((true, false)) => {
                    let detail = file
                        .detail
                        .as_deref()
                        .map_or(String::new(), |d| format!(" {d}"));
                    ev.findings.push(Finding {
                        kind: FindingKind::MissingRequirement,
                        text: format!("requires {}, which is not present.{detail}", file.label()),
                        hard: true,
                    });
                }
                Some((true, true)) => {
                    if let Some(other) = env.position(&file.name).filter(|&at| at > index) {
                        ev.findings.push(load_after(
                            &file.name,
                            &env.slots()[other].name,
                            "requires",
                        ));
                    }
                }
                _ => {}
            }
        }
        for file in entries.iter().flat_map(|entry| entry.inc.iter()) {
            if let Some((true, true)) = ev.file_state(file) {
                let detail = file
                    .detail
                    .as_deref()
                    .map_or(String::new(), |d| format!(" {d}"));
                ev.findings.push(Finding {
                    kind: FindingKind::Incompatible,
                    text: format!(
                        "is incompatible with {}, which is present.{detail}",
                        file.label()
                    ),
                    hard: true,
                });
            }
        }
        for file in entries.iter().flat_map(|entry| entry.after.iter()) {
            if ev.holds(file.condition.as_deref()) != Some(true) {
                continue;
            }
            if let Some(other) = env.position(&file.name).filter(|&at| at > index)
                && env.installed(&file.name)
            {
                ev.findings.push(load_after(
                    &file.name,
                    &env.slots()[other].name,
                    "must load after",
                ));
            }
        }
    }

    fn group_rules(&self, ev: &mut Evaluator, index: usize) {
        let group = &self.groups_of[&index];
        if !self.defined.contains(group.as_str()) {
            ev.findings.push(Finding {
                kind: FindingKind::UndefinedGroup,
                text: format!("is in group \"{group}\", which is not defined"),
                hard: self.options.strict_groups,
            });
            return;
        }
        let Some(after) = self.closure.get(group) else {
            return;
        };
        for &(other_index, other_name) in self.installed.iter().filter(|(at, _)| *at > index) {
            let other_group = &self.groups_of[&other_index];
            let reverse = self
                .closure
                .get(other_group)
                .is_some_and(|set| set.contains(group));
            if after.contains(other_group) && !reverse {
                ev.findings.push(Finding {
                    kind: FindingKind::GroupOrder,
                    text: format!(
                        "(group \"{group}\") loads before {other_name} (group \"{other_group}\"), but \"{group}\" loads after \"{other_group}\""
                    ),
                    hard: self.options.strict_groups,
                });
            }
        }
    }

    fn plugin(&self, index: usize, name: &str, entries: &[&PluginEntry]) -> PluginReport {
        let mut ev = Evaluator {
            env: self.env,
            findings: Vec::new(),
        };
        let all_messages: Vec<&Message> =
            entries.iter().flat_map(|entry| entry.msg.iter()).collect();
        let messages = ev.messages(&all_messages);

        let mut added: Vec<String> = Vec::new();
        let mut removed: Vec<String> = Vec::new();
        for tag in entries.iter().flat_map(|entry| entry.tag.iter()) {
            if ev.holds(tag.condition.as_deref()) == Some(true) {
                match tag.name.strip_prefix('-') {
                    Some(name) => removed.push(name.to_string()),
                    None => added.push(tag.name.clone()),
                }
            }
        }
        added.retain(|tag| !removed.contains(tag));
        added.dedup();
        removed.dedup();

        self.file_rules(&mut ev, index, entries);
        self.group_rules(&mut ev, index);

        PluginReport {
            name: name.to_string(),
            position: index,
            active: self.env.slots()[index].active,
            group: self.groups_of[&index].clone(),
            messages,
            tags_add: added,
            tags_remove: removed,
            findings: ev.findings,
            entries_matched: entries.len(),
        }
    }
}

/// Evaluate `metadata` over the load order held by `env`.
#[must_use]
pub fn run(metadata: &Metadata, env: &FsEnv, options: Options) -> Report {
    let mut report = Report {
        metadata_warnings: metadata.warnings.clone(),
        ..Report::default()
    };
    let mut globals = Evaluator {
        env,
        findings: Vec::new(),
    };
    let global_refs: Vec<&Message> = metadata.globals.iter().collect();
    report.globals = globals.messages(&global_refs);
    report.general = globals.findings;
    report.not_installed = env
        .slots()
        .iter()
        .filter(|slot| !env.installed(&slot.name))
        .map(|slot| slot.name.clone())
        .collect();

    let installed: Vec<(usize, &str)> = env
        .slots()
        .iter()
        .enumerate()
        .filter(|(_, slot)| env.installed(&slot.name))
        .map(|(index, slot)| (index, slot.name.as_str()))
        .collect();
    let mut groups_of = HashMap::new();
    let mut matched: HashMap<usize, Vec<&PluginEntry>> = HashMap::new();
    for &(index, name) in &installed {
        let entries: Vec<&PluginEntry> = metadata
            .plugins
            .iter()
            .filter(|entry| entry.pattern.matches(name))
            .collect();
        let group = entries
            .iter()
            .find_map(|entry| entry.group.clone())
            .unwrap_or_else(|| DEFAULT_GROUP.to_string());
        groups_of.insert(index, group);
        matched.insert(index, entries);
    }
    let run = Run {
        env,
        options,
        closure: group_closure(metadata),
        defined: metadata
            .groups
            .iter()
            .map(|group| group.name.as_str())
            .collect(),
        installed: installed.clone(),
        groups_of,
    };
    for (index, name) in installed {
        report
            .plugins
            .push(run.plugin(index, name, &matched[&index]));
    }
    report
}

fn load_after(wanted: &str, found: &str, verb: &str) -> Finding {
    Finding {
        kind: FindingKind::LoadAfter,
        text: format!("{verb} {wanted}, but {found} is later in the load order"),
        hard: true,
    }
}
