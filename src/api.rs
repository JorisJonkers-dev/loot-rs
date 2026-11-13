//! The library API for a program that manages a load order (a mod manager): [`sort`] takes the load order and a
//! [`Masterlist`] and returns the sorted order, the messages and Bash Tags that apply, and the cleaning data
//! that matches the installed plugins.
//!
//! ```no_run
//! use loot_rs::api::{Masterlist, PluginSlot, SortOptions, sort};
//! use std::path::Path;
//!
//! let masterlist = Masterlist::from_file(Path::new("masterlist.yaml"))?;
//! let load_order = vec![
//!     PluginSlot::new("FalloutNV.esm", true),
//!     PluginSlot::new("MyMod.esp", true),
//! ];
//! let result = sort(&load_order, &masterlist, Path::new("Data"), &SortOptions::default())?;
//! for name in &result.order {
//!     println!("{name}");
//! }
//! for plugin in &result.plugins {
//!     for message in &plugin.messages {
//!         println!("{}: {}", plugin.name, message.text);
//!     }
//! }
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! # Contract
//!
//! * Nothing is written. Plugin files are only read (headers, and whole files for a checksum when the masterlist
//!   has cleaning data for them), and the masterlist is only read.
//! * The result depends only on the arguments and the files under `data_dir`: no clock, no network, no
//!   randomness. Which group rules are dropped does not depend on the order given, and sorting a sorted order
//!   changes nothing.
//! * Every size that comes from input is bounded (see `docs/RESOURCE-LIMITS.md`), and limits are errors, never
//!   panics.
//! * What is not implemented is reported, never guessed: a condition this crate cannot evaluate leaves its item
//!   out and appears as a [`FindingKind::Unsupported`] finding. See `docs/COMPATIBILITY.md`.
//!
//! # Versioning
//!
//! Everything in this module follows semantic versioning (while the crate is 0.x, a minor version may break it, a
//! patch version never does). Structs are `#[non_exhaustive]` and built with `new` or `Default`, so a field can be
//! added without a break; enums are `#[non_exhaustive]`, so a variant can be added: always keep a `_` arm. The
//! other modules of the crate (`check`, `condition`, `env`, `metadata`, `plugin`, `render`, `sort`, `cli`) are the
//! implementation and are not covered by that promise. See `docs/API.md`.

use crate::check::{self, FindingKind as InnerKind};
use crate::condition::{ConditionError, eval_str};
use crate::env::{FsEnv, Slot, crc32_file};
use crate::metadata::{
    self, DEFAULT_GROUP, MessageKind as InnerMessageKind, Metadata, PluginEntry,
};
use crate::plugin;
use crate::sort::{GraphError, MAX_PLUGINS, order};
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

/// A masterlist that has been read and checked. Cheap to keep and to share between sorts.
#[derive(Debug, Clone)]
pub struct Masterlist {
    metadata: Metadata,
}

/// A masterlist that could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum MasterlistError {
    /// The file could not be read, or is larger than the limit ([`metadata::MAX_MASTERLIST_BYTES`] bytes).
    Io(String),
    /// The text is not a masterlist, or is over a limit (aliases that expand too far, nesting that is too deep).
    Invalid(String),
}

impl fmt::Display for MasterlistError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(text) => write!(f, "cannot read the masterlist: {text}"),
            Self::Invalid(text) => write!(f, "invalid masterlist: {text}"),
        }
    }
}

impl std::error::Error for MasterlistError {}

impl Masterlist {
    /// Read a masterlist from YAML text.
    ///
    /// # Errors
    /// [`MasterlistError::Invalid`] for text that is not a masterlist or is over a limit.
    pub fn parse(text: &str) -> Result<Self, MasterlistError> {
        metadata::parse(text)
            .map(|metadata| Self { metadata })
            .map_err(|error| MasterlistError::Invalid(error.0))
    }

    /// Read a masterlist from a file.
    ///
    /// # Errors
    /// [`MasterlistError::Io`] when the file cannot be read as UTF-8 text or is over the size limit,
    /// [`MasterlistError::Invalid`] as for [`Masterlist::parse`].
    pub fn from_file(path: &Path) -> Result<Self, MasterlistError> {
        let io =
            |error: &dyn fmt::Display| MasterlistError::Io(format!("{}: {error}", path.display()));
        let limit = metadata::MAX_MASTERLIST_BYTES as u64;
        let file = fs::File::open(path).map_err(|error| io(&error))?;
        let mut bytes = Vec::new();
        file.take(limit + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| io(&error))?;
        if bytes.len() as u64 > limit {
            return Err(io(&format!("larger than the limit of {limit} bytes")));
        }
        let text = String::from_utf8(bytes).map_err(|_| io(&"not valid UTF-8"))?;
        Self::parse(&text)
    }

    /// Entries that were skipped while reading (for example a regular expression with look-around).
    #[must_use]
    pub fn warnings(&self) -> &[String] {
        &self.metadata.warnings
    }
}

/// One plugin of the load order.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct PluginSlot {
    /// The file name, as it is in the `Data` directory (case does not matter).
    pub name: String,
    /// Whether the plugin is active.
    pub active: bool,
}

impl PluginSlot {
    /// A plugin of the load order.
    #[must_use]
    pub fn new(name: impl Into<String>, active: bool) -> Self {
        Self {
            name: name.into(),
            active,
        }
    }
}

/// Switches for [`sort`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct SortOptions {
    /// Count group violations and undefined groups as hard findings (see `docs/GROUPS.md`). Off by default.
    pub strict_groups: bool,
}

impl SortOptions {
    /// Count group violations and undefined groups as hard findings.
    #[must_use]
    pub const fn with_strict_groups(mut self, strict: bool) -> Self {
        self.strict_groups = strict;
        self
    }
}

/// The severity of a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum MessageKind {
    /// A generic note.
    Say,
    /// A non-critical issue.
    Warn,
    /// A critical issue.
    Error,
}

/// A message from the masterlist whose condition holds.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Message {
    /// Severity.
    pub kind: MessageKind,
    /// The text (English, substitutions applied).
    pub text: String,
}

/// What a finding is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum FindingKind {
    /// A required file is missing.
    MissingRequirement,
    /// An incompatible file is present.
    Incompatible,
    /// A plugin loads before one it must load after (only after a sort that could not satisfy the rule).
    LoadAfter,
    /// A group relation is violated (a group rule that had to be dropped, see [`SortOutput::dropped_group_rules`]).
    GroupOrder,
    /// The plugin's group is not defined in the masterlist.
    UndefinedGroup,
    /// A condition uses a function that is not implemented, so its item was not applied.
    Unsupported,
    /// A condition was invalid or could not be evaluated, so its item was not applied.
    ConditionError,
}

impl From<InnerKind> for FindingKind {
    fn from(kind: InnerKind) -> Self {
        match kind {
            InnerKind::MissingRequirement => Self::MissingRequirement,
            InnerKind::Incompatible => Self::Incompatible,
            InnerKind::LoadAfter => Self::LoadAfter,
            InnerKind::GroupOrder => Self::GroupOrder,
            InnerKind::UndefinedGroup => Self::UndefinedGroup,
            InnerKind::Unsupported => Self::Unsupported,
            InnerKind::ConditionError => Self::ConditionError,
        }
    }
}

/// A problem found for a plugin.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Finding {
    /// Category.
    pub kind: FindingKind,
    /// A sentence describing it.
    pub text: String,
    /// True when it should fail a check (a violated requirement, an incompatibility; group findings only with
    /// [`SortOptions::strict_groups`]).
    pub hard: bool,
}

/// Everything the masterlist says about one plugin.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct PluginResult {
    /// The file name as given in the load order.
    pub name: String,
    /// Position in [`SortOutput::order`], from 0.
    pub position: usize,
    /// Whether the plugin is active.
    pub active: bool,
    /// The plugin's group (`default` when the masterlist names none).
    pub group: String,
    /// Messages whose condition holds.
    pub messages: Vec<Message>,
    /// Bash Tags to add.
    pub tags_added: Vec<String>,
    /// Bash Tags to remove.
    pub tags_removed: Vec<String>,
    /// Problems.
    pub findings: Vec<Finding>,
}

/// Whether a checksum is on the masterlist's dirty or its clean list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CleaningStatus {
    /// This exact file needs cleaning.
    Dirty,
    /// This exact file was checked and is clean.
    Clean,
}

/// Cleaning data of the masterlist that matches the installed file by CRC-32.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct DirtyInfo {
    /// The plugin's file name as given in the load order.
    pub plugin: String,
    /// The CRC-32 of the installed file (also the one on the masterlist).
    pub crc: u32,
    /// Dirty or clean.
    pub status: CleaningStatus,
    /// The utility the data was made with.
    pub utility: String,
    /// Identical-to-master records.
    pub itm: u32,
    /// Undeleted references.
    pub udr: u32,
    /// Deleted navmeshes.
    pub nav: u32,
}

/// The result of [`sort`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct SortOutput {
    /// The installed plugins in sorted load order, named as given. Plugins that are not installed are not in it,
    /// see [`SortOutput::not_installed`]; a caller that keeps them places them itself.
    pub order: Vec<String>,
    /// Messages of the masterlist that apply to no plugin in particular.
    pub general_messages: Vec<Message>,
    /// One entry per plugin of [`SortOutput::order`], in that order.
    pub plugins: Vec<PluginResult>,
    /// Cleaning data that matches installed files.
    pub dirty: Vec<DirtyInfo>,
    /// Plugins of the load order that are not in the `Data` directory.
    pub not_installed: Vec<String>,
    /// Group rules `(plugin that should be later, plugin that should be earlier)` left out because a requirement,
    /// a master or a stronger group rule contradicts them.
    pub dropped_group_rules: Vec<(String, String)>,
    /// Things that were skipped: unreadable plugin headers, duplicate names, masterlist entries.
    pub warnings: Vec<String>,
}

impl SortOutput {
    /// True when the sorted order differs from the order of the installed plugins that was given.
    #[must_use]
    pub fn changed(&self, load_order: &[PluginSlot]) -> bool {
        let given: Vec<String> = load_order
            .iter()
            .filter(|slot| {
                self.order
                    .iter()
                    .any(|name| name.eq_ignore_ascii_case(&slot.name))
            })
            .map(|slot| slot.name.clone())
            .collect();
        given != self.order
    }
}

/// Why [`sort`] gave no result.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SortError {
    /// `data_dir` is not a directory.
    DataDirectory(PathBuf),
    /// More plugins than can be sorted ([`crate::sort::MAX_PLUGINS`]).
    TooManyPlugins(usize),
    /// Two rules that cannot both hold: `first` has to load before `second` and `second` before `first`
    /// (through `after`, `req` or the masters of the plugins).
    Conflict {
        /// A plugin that must load first.
        first: String,
        /// A plugin that must load later, but also has to load before `first`.
        second: String,
    },
    /// A plugin that is not a master has to load before one that is.
    MasterOrder {
        /// The plugin without the master flag.
        plugin: String,
        /// The master it has to load before.
        master: String,
    },
    /// The load order needs more work than the limit allows.
    TooMuchWork,
}

impl fmt::Display for SortError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DataDirectory(path) => write!(f, "{}: not a directory", path.display()),
            Self::TooManyPlugins(count) => {
                write!(f, "{count} plugins, more than the limit of {MAX_PLUGINS}")
            }
            Self::Conflict { first, second } => write!(
                f,
                "{first} has to load before {second}, and {second} has to load before {first}"
            ),
            Self::MasterOrder { plugin, master } => write!(
                f,
                "{plugin} (not a master) has to load before {master} (a master)"
            ),
            Self::TooMuchWork => {
                f.write_str("the load order needs more work than the limit allows")
            }
        }
    }
}

impl std::error::Error for SortError {}

fn matching<'a>(metadata: &'a Metadata, name: &str) -> Vec<&'a PluginEntry> {
    metadata
        .plugins
        .iter()
        .filter(|entry| entry.pattern.matches(name))
        .collect()
}

fn group_of(entries: &[&PluginEntry]) -> String {
    entries
        .iter()
        .find_map(|entry| entry.group.clone())
        .unwrap_or_else(|| DEFAULT_GROUP.to_string())
}

const fn message_kind(kind: InnerMessageKind) -> MessageKind {
    match kind {
        InnerMessageKind::Say => MessageKind::Say,
        InnerMessageKind::Warn => MessageKind::Warn,
        InnerMessageKind::Error => MessageKind::Error,
    }
}

fn messages(applied: &[check::AppliedMessage]) -> Vec<Message> {
    applied
        .iter()
        .map(|message| Message {
            kind: message_kind(message.kind),
            text: message.content.clone(),
        })
        .collect()
}

/// What each installed plugin is, from its header and the masterlist, in load order.
struct Facts<'a> {
    name: &'a str,
    is_master: bool,
    masters: Vec<String>,
    entries: Vec<&'a PluginEntry>,
    group: String,
}

/// Sort a load order.
///
/// `load_order` is the current order, with the active flags; `data_dir` is the game's `Data` directory (paths in
/// conditions are relative to it, `..` reaches the game directory). The result is described in the module
/// documentation and on [`SortOutput`].
///
/// The rules, in the order they win: masters (plugins with the master flag) load before the others; the masters
/// a plugin's header lists load before it; the masterlist's `after` and `req` plugins load before it; groups
/// (a plugin in a group that loads after another group loads after that group's plugins) where they do not
/// contradict the rules before. Among the plugins that may come next, the one that was earlier in the load order
/// comes first, so a plugin only moves for a reason.
///
/// # Errors
/// [`SortError`] when the rules that must hold contradict each other, or the input is too large. A masterlist
/// condition that cannot be evaluated is not an error: its item is left out and reported in the plugin's findings.
pub fn sort(
    load_order: &[PluginSlot],
    masterlist: &Masterlist,
    data_dir: &Path,
    options: &SortOptions,
) -> Result<SortOutput, SortError> {
    if !data_dir.is_dir() {
        return Err(SortError::DataDirectory(data_dir.to_path_buf()));
    }
    if load_order.len() > MAX_PLUGINS * 4 {
        return Err(SortError::TooManyPlugins(load_order.len()));
    }
    let metadata = &masterlist.metadata;
    let mut output = SortOutput::default();
    let env = FsEnv::new(data_dir, distinct_slots(load_order, &mut output.warnings));
    let installed: Vec<&Slot> = env
        .slots()
        .iter()
        .filter(|slot| env.installed(&slot.name))
        .collect();
    output.not_installed = env
        .slots()
        .iter()
        .filter(|slot| !env.installed(&slot.name))
        .map(|slot| slot.name.clone())
        .collect();
    if installed.len() > MAX_PLUGINS {
        return Err(SortError::TooManyPlugins(installed.len()));
    }

    let facts: Vec<Facts> = installed
        .iter()
        .map(|slot| facts_of(&env, metadata, slot, &mut output.warnings))
        .collect();
    let (hard, soft) = rules(&env, metadata, &facts, &mut output.warnings);

    let is_master: Vec<bool> = facts.iter().map(|fact| fact.is_master).collect();
    let name_of = |index: usize| facts[index].name.to_string();
    let sorted = order(facts.len(), &is_master, &hard, &soft).map_err(|error| match error {
        GraphError::Cycle(a, b) => SortError::Conflict {
            first: name_of(a),
            second: name_of(b),
        },
        GraphError::MasterOrder(a, b) => SortError::MasterOrder {
            plugin: name_of(a),
            master: name_of(b),
        },
        GraphError::TooMany(count) => SortError::TooManyPlugins(count),
        // The edges are built from indices of `facts`.
        GraphError::TooMuchWork | GraphError::BadIndex(_) => SortError::TooMuchWork,
    })?;
    output.dropped_group_rules = sorted
        .dropped
        .iter()
        .map(|&(earlier, later)| (name_of(later), name_of(earlier)))
        .collect();
    output.order = sorted.order.iter().map(|&index| name_of(index)).collect();

    // What the masterlist says, with the plugins in their sorted places: a rule that the sort could not satisfy
    // shows up as a finding.
    let sorted_slots: Vec<Slot> = sorted
        .order
        .iter()
        .map(|&index| installed[index].clone())
        .collect();
    let sorted_env = FsEnv::new(data_dir, sorted_slots);
    let report = check::run(
        metadata,
        &sorted_env,
        check::Options {
            strict_groups: options.strict_groups,
        },
    );
    output.general_messages = messages(&report.globals);
    output.plugins = report.plugins.iter().map(plugin_result).collect();
    output.dirty = dirty_info(&facts, &sorted.order, &sorted_env);
    Ok(output)
}

/// The slots without repeated names (the first of a name is kept, the rest are noted).
fn distinct_slots(load_order: &[PluginSlot], warnings: &mut Vec<String>) -> Vec<Slot> {
    let mut seen = HashSet::new();
    let mut slots = Vec::new();
    for slot in load_order {
        if seen.insert(slot.name.to_lowercase()) {
            slots.push(Slot {
                name: slot.name.clone(),
                active: slot.active,
            });
        } else {
            warnings.push(format!(
                "{}: listed more than once, the first is used",
                slot.name
            ));
        }
    }
    slots
}

/// The header and the masterlist entries of one installed plugin.
fn facts_of<'a>(
    env: &FsEnv,
    metadata: &'a Metadata,
    slot: &'a Slot,
    warnings: &mut Vec<String>,
) -> Facts<'a> {
    let header = env
        .resolve(&slot.name)
        .ok_or_else(|| io_error("not found"))
        .and_then(|path| plugin::read_header(&path));
    let (is_master, masters) = match header {
        Ok(header) => (header.is_master(), header.masters),
        Err(error) => {
            warnings.push(format!(
                "{}: header not readable ({error}); treated as a plugin without masters",
                slot.name
            ));
            (false, Vec::new())
        }
    };
    let entries = matching(metadata, &slot.name);
    let group = group_of(&entries);
    Facts {
        name: &slot.name,
        is_master,
        masters,
        entries,
        group,
    }
}

/// Edges `(earlier, later)` over indices of the installed plugins.
type Edges = Vec<(usize, usize)>;

/// The edges `(earlier, later)` over the indices of `facts`: those that must hold (masters, `after`, `req`) and
/// those that hold when they can (groups), the latter in an order that depends on names only.
fn rules(
    env: &FsEnv,
    metadata: &Metadata,
    facts: &[Facts],
    warnings: &mut Vec<String>,
) -> (Edges, Edges) {
    let index_of: HashMap<String, usize> = facts
        .iter()
        .enumerate()
        .map(|(index, fact)| (fact.name.to_lowercase(), index))
        .collect();
    let mut conditions = Conditions { env, warnings };
    let mut hard: Vec<(usize, usize)> = Vec::new();
    for (index, fact) in facts.iter().enumerate() {
        for master in &fact.masters {
            if let Some(&before) = index_of.get(&master.to_lowercase()) {
                hard.push((before, index));
            }
        }
        for file in fact
            .entries
            .iter()
            .flat_map(|entry| entry.after.iter().chain(entry.req.iter()))
        {
            let Some(&before) = index_of.get(&file.name.to_lowercase()) else {
                continue;
            };
            if conditions.holds(file.condition.as_deref(), fact.name)
                && (file.constraint.is_none()
                    || conditions.holds(file.constraint.as_deref(), fact.name))
            {
                hard.push((before, index));
            }
        }
    }

    let closure = check::group_closure(metadata);
    let defined: HashSet<&str> = metadata
        .groups
        .iter()
        .map(|group| group.name.as_str())
        .collect();
    let mut by_name: Vec<usize> = (0..facts.len()).collect();
    by_name.sort_by_key(|&index| facts[index].name.to_lowercase());
    let mut soft: Vec<(usize, usize)> = Vec::new();
    for &later in &by_name {
        let group = &facts[later].group;
        let Some(after) = closure
            .get(group)
            .filter(|_| defined.contains(group.as_str()))
        else {
            continue;
        };
        for &earlier in &by_name {
            let other = &facts[earlier].group;
            let reverse = closure.get(other).is_some_and(|set| set.contains(group));
            if earlier != later && after.contains(other) && !reverse {
                soft.push((earlier, later));
            }
        }
    }
    (hard, soft)
}

fn plugin_result(plugin: &check::PluginReport) -> PluginResult {
    PluginResult {
        name: plugin.name.clone(),
        position: plugin.position,
        active: plugin.active,
        group: plugin.group.clone(),
        messages: messages(&plugin.messages),
        tags_added: plugin.tags_add.clone(),
        tags_removed: plugin.tags_remove.clone(),
        findings: plugin
            .findings
            .iter()
            .map(|finding| Finding {
                kind: finding.kind.into(),
                text: finding.text.clone(),
                hard: finding.hard,
            })
            .collect(),
    }
}

fn io_error(text: &str) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::NotFound, text.to_string())
}

/// The cleaning data whose checksum is the installed file's. Only files that have such data are read.
fn dirty_info(facts: &[Facts], order: &[usize], env: &FsEnv) -> Vec<DirtyInfo> {
    let mut found = Vec::new();
    for &index in order {
        let fact = &facts[index];
        let wanted = |status: CleaningStatus| {
            fact.entries.iter().flat_map(move |entry| {
                let list = match status {
                    CleaningStatus::Dirty => &entry.dirty,
                    CleaningStatus::Clean => &entry.clean,
                };
                list.iter().map(move |data| (status, data))
            })
        };
        let all: Vec<_> = wanted(CleaningStatus::Dirty)
            .chain(wanted(CleaningStatus::Clean))
            .filter(|(_, data)| data.crc.is_some())
            .collect();
        if all.is_empty() {
            continue;
        }
        let Some(crc) = env
            .resolve(fact.name)
            .and_then(|path| crc32_file(&path).ok())
        else {
            continue;
        };
        for (status, data) in all.into_iter().filter(|(_, data)| data.crc == Some(crc)) {
            found.push(DirtyInfo {
                plugin: fact.name.to_string(),
                crc,
                status,
                utility: data.util.clone(),
                itm: data.itm,
                udr: data.udr,
                nav: data.nav,
            });
        }
    }
    found
}

struct Conditions<'a> {
    env: &'a FsEnv,
    warnings: &'a mut Vec<String>,
}

impl Conditions<'_> {
    /// Does the condition hold? One that cannot be evaluated does not, and is noted.
    fn holds(&mut self, condition: Option<&str>, plugin: &str) -> bool {
        let Some(text) = condition else {
            return true;
        };
        match eval_str(text, self.env) {
            Ok(value) => value,
            Err(error) => {
                let how = match error {
                    ConditionError::Unsupported(_) => "not evaluated",
                    _ => "not valid",
                };
                self.warnings.push(format!(
                    "{plugin}: ordering rule left out, condition {how}: {error} (in `{}`)",
                    text.trim()
                ));
                false
            }
        }
    }
}

#[cfg(test)]
#[path = "api_tests.rs"]
mod tests;
