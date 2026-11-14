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
//! * Nothing is written. Plugin files are only read (their records, as the sort needs them, and whole files for a
//!   checksum when the masterlist has cleaning data for them), archives only for their tables, and the
//!   masterlist is only read.
//! * The result depends only on the arguments and the files under `data_dir`, `SortOptions::game_dir` and `SortOptions::additional_data_dirs`: no clock, no network, no
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
//! other modules of the crate (`archives`, `check`, `cli`, `condition`, `env`, `metadata`, `overlap`, `plugin`,
//! `records`, `render`, `sort`, `validate`) are the implementation and are not covered by that promise. See
//! `docs/API.md`.

use crate::archives::{self, ArchiveError, MAX_ASSETS_TOTAL, PathIds, PluginAssets};
use crate::check::{self, FindingKind as InnerKind};
use crate::condition::{ConditionError, eval_str};
use crate::env::{FsEnv, Slot, crc32_file};
use crate::metadata::{
    self, Cleaning, DEFAULT_GROUP, MessageKind as InnerMessageKind, Metadata, PluginEntry,
};
use crate::overlap::PluginOverlaps;
use crate::plugin;
pub use crate::plugin::{PluginError, PluginHeader};
use crate::records::{self, MAX_RECORDS_TOTAL, NameIds, PluginRecords, RecordsError};
pub use crate::render::SCHEMA_VERSION;
use crate::sort::{self, GraphError, GroupInput, MAX_PLUGINS, PluginInput};
use crate::validate;
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
///
/// Not `Copy` since it holds directories; clone it where a copy is needed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct SortOptions {
    /// Count group violations and undefined groups as hard findings (see `docs/GROUPS.md`). Off by default.
    pub strict_groups: bool,
    /// The game directory: what a leading `..` in a condition path names (`../nvse_1_4.dll`). When unset, `..` is
    /// the parent of the directory searched.
    pub game_dir: Option<PathBuf>,
    /// Further directories that hold plugins and files, searched in this order before `data_dir`. When a file is
    /// in several directories, the first one that has it is used (see `docs/API.md`).
    pub additional_data_dirs: Vec<PathBuf>,
}

impl SortOptions {
    /// Count group violations and undefined groups as hard findings.
    #[must_use]
    pub const fn with_strict_groups(mut self, strict: bool) -> Self {
        self.strict_groups = strict;
        self
    }

    /// Set the game directory (see [`SortOptions::game_dir`]).
    #[must_use]
    pub fn with_game_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.game_dir = Some(dir.into());
        self
    }

    /// Add an additional data directory after those already added (see [`SortOptions::additional_data_dirs`]).
    #[must_use]
    pub fn with_additional_data_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.additional_data_dirs.push(dir.into());
        self
    }

    /// Replace the additional data directories (see [`SortOptions::additional_data_dirs`]).
    #[must_use]
    pub fn with_additional_data_dirs<I, P>(mut self, dirs: I) -> Self
    where
        I: IntoIterator<Item = P>,
        P: Into<PathBuf>,
    {
        self.additional_data_dirs = dirs.into_iter().map(Into::into).collect();
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

impl MessageKind {
    /// The word used for this kind in the text and JSON output: `say`, `warn` or `error`.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Say => "say",
            Self::Warn => "warn",
            Self::Error => "error",
        }
    }
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

impl FindingKind {
    /// The short label used for this kind in the text and JSON output, for example `missing requirement`.
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
    /// Messages LOOT generates when it checks the install, in Markdown like the masterlist's own: a master that
    /// is missing or inactive, a header version below the game's minimum, a group nobody defined, a `BashTags` file
    /// that overrides suggestions, cleaning data that matches the file, requirements and incompatibilities.
    pub generated_messages: Vec<Message>,
    /// The Bash Tags the plugin's description lists (`{{BASH:Tag,Tag}}`).
    pub bash_tags: Vec<String>,
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

/// Where an ordering rule comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum RuleSource {
    /// The plugin lists the other as a master in its header.
    HeaderMaster,
    /// The masterlist's `after` entry.
    After,
    /// The masterlist's `req` entry.
    Req,
    /// The groups of the two plugins.
    Group,
}

impl RuleSource {
    /// A short word for output: `header-master`, `after`, `req` or `group`.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::HeaderMaster => "header-master",
            Self::After => "after",
            Self::Req => "req",
            Self::Group => "group",
        }
    }
}

/// A rule that the sorted order honours: `before` loads before `after`. Rules that were dropped are not listed
/// (see [`SortOutput::dropped_group_rules`]); "masters load first" is a rule of the whole sort and not listed.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct OrderingRule {
    /// The plugin that loads first.
    pub before: String,
    /// The plugin that loads later.
    pub after: String,
    /// Why.
    pub source: RuleSource,
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
    /// Messages LOOT generates about the load order as a whole (more active plugins than the game supports).
    pub generated_general_messages: Vec<Message>,
    /// One entry per plugin of [`SortOutput::order`], in that order.
    pub plugins: Vec<PluginResult>,
    /// The rules the order honours, to explain it or to check it against another tool's order.
    pub ordering_rules: Vec<OrderingRule>,
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

    /// The whole load order with the plugins that are not installed put back, by the default rule: a plugin that
    /// is not installed goes right after the installed plugin that precedes it in `load_order` (after any
    /// not-installed plugins already placed there), or first when nothing installed precedes it. Names are matched
    /// without regard to ASCII case; installed plugins keep the sorted order of [`SortOutput::order`].
    ///
    /// A caller that wants another rule places [`SortOutput::not_installed`] itself.
    #[must_use]
    pub fn order_with_not_installed(&self, load_order: &[PluginSlot]) -> Vec<String> {
        let installed = |name: &str| {
            self.order
                .iter()
                .any(|item| item.eq_ignore_ascii_case(name))
        };
        // Not-installed plugins by the lowercase name of the installed plugin they follow (`None`: the front).
        let mut after: HashMap<Option<String>, Vec<&str>> = HashMap::new();
        let mut anchor: Option<String> = None;
        for slot in load_order {
            if installed(&slot.name) {
                anchor = Some(slot.name.to_ascii_lowercase());
            } else {
                after.entry(anchor.clone()).or_default().push(&slot.name);
            }
        }
        let mut placed: Vec<String> = after
            .remove(&None)
            .unwrap_or_default()
            .into_iter()
            .map(str::to_owned)
            .collect();
        for name in &self.order {
            placed.push(name.clone());
            if let Some(names) = after.remove(&Some(name.to_ascii_lowercase())) {
                placed.extend(names.into_iter().map(str::to_owned));
            }
        }
        placed
    }
}

/// Read the header of a plugin file: its masters, flags, author, description and record count. Reads only the header
/// record, so it is cheap to call for every plugin of a load order.
///
/// # Errors
/// An I/O error for a file that cannot be read; a header that is malformed gives [`std::io::ErrorKind::InvalidData`]
/// wrapping a [`PluginError`].
pub fn read_plugin_header(path: &Path) -> std::io::Result<PluginHeader> {
    plugin::read_header(path)
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
    /// Groups that each load after the other, by name (LOOT refuses such a masterlist too).
    GroupCycle(Vec<String>),
    /// The load order needs more work than the limit allows.
    TooMuchWork,
    /// A check inside the sorter failed. This is a bug, never a property of the input.
    Internal(String),
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
            Self::GroupCycle(groups) => {
                write!(
                    f,
                    "groups that load after each other: {}",
                    groups.join(", ")
                )
            }
            Self::TooMuchWork => {
                f.write_str("the load order needs more work than the limit allows")
            }
            Self::Internal(text) => write!(f, "internal error in the sorter: {text}"),
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
    /// Whether the header could be read (the plugin's records are only read then).
    readable: bool,
    header: Option<plugin::PluginHeader>,
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
/// The order is LOOT's: libloot's plugin graph (masters first; edges from header masters, `req`, `after` and groups;
/// overlap of records and of archive files; then the current load order wherever nothing forbids it), see
/// `docs/COMPATIBILITY.md` for what is ported. A plugin therefore only moves for a reason.
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
    let slots = distinct_slots(load_order, &mut output.warnings);
    let env = search_env(data_dir, slots, options);
    check_directories(&env, options, &mut output.warnings);
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
    let rules = rules(&env, metadata, &facts, &mut output.warnings);

    let name_of = |index: usize| facts[index].name.to_string();
    let records = read_all_records(&env, &facts, &mut output.warnings)?;
    let assets = read_all_assets(&env, data_dir, &facts, &mut output.warnings)?;
    let inputs: Vec<PluginInput> = facts
        .iter()
        .enumerate()
        .map(|(index, fact)| {
            let mut input = plugin_input(fact, &rules, index);
            input.override_records = records[index].overridden;
            input.assets = assets[index].keys.len();
            input
        })
        .collect();
    let overlaps = PluginOverlaps::new(
        records.into_iter().map(|plugin| plugin.keys).collect(),
        assets.into_iter().map(|plugin| plugin.keys).collect(),
    );
    let groups: Vec<GroupInput> = metadata
        .groups
        .iter()
        .map(|group| GroupInput {
            name: group.name.clone(),
            after: group.after.clone(),
        })
        .collect();
    let sorted = sort::sort(&inputs, &groups, &[EARLY_LOADER.to_string()], &overlaps)
        .map_err(|error| sort_error(&error, &name_of))?;
    let position: Vec<usize> = {
        let mut position = vec![0; facts.len()];
        for (at, &index) in sorted.order.iter().enumerate() {
            position[index] = at;
        }
        position
    };
    // Group rules the order does not satisfy: the sorter left them out because they would have closed a cycle.
    let dropped: Vec<(usize, usize)> = rules
        .soft
        .iter()
        .copied()
        .filter(|&(earlier, later)| position[earlier] > position[later])
        .collect();
    output.dropped_group_rules = dropped
        .iter()
        .map(|&(earlier, later)| (name_of(later), name_of(earlier)))
        .collect();
    output.order = sorted.order.iter().map(|&index| name_of(index)).collect();
    output.ordering_rules = ordering_rules(&rules, &dropped, &name_of);

    // What the masterlist says, with the plugins in their sorted places: a rule that the sort could not satisfy
    // shows up as a finding.
    let sorted_slots: Vec<Slot> = sorted
        .order
        .iter()
        .map(|&index| installed[index].clone())
        .collect();
    let sorted_env = search_env(data_dir, sorted_slots, options);
    let report = check::run(
        metadata,
        &sorted_env,
        check::Options {
            strict_groups: options.strict_groups,
        },
    );
    describe(
        &mut output,
        metadata,
        &facts,
        &sorted.order,
        &sorted_env,
        &report,
    );
    Ok(output)
}

/// The environment over `data_dir` and the directories of `options`.
fn search_env(data_dir: &Path, slots: Vec<Slot>, options: &SortOptions) -> FsEnv {
    FsEnv::new(data_dir, slots)
        .with_game_dir(options.game_dir.as_deref())
        .with_additional_data_dirs(&options.additional_data_dirs)
}

/// Warnings about the directories searched: one that is not a directory, names that differ only in case.
fn check_directories(env: &FsEnv, options: &SortOptions, warnings: &mut Vec<String>) {
    for dir in options.game_dir.iter().chain(&options.additional_data_dirs) {
        if !dir.is_dir() {
            warnings.push(format!("{}: not a directory, ignored", dir.display()));
        }
    }
    warnings.extend(env.case_warnings());
}

/// What the masterlist and LOOT's own checks say about each plugin, in the sorted order.
fn describe(
    output: &mut SortOutput,
    metadata: &Metadata,
    facts: &[Facts],
    order: &[usize],
    sorted_env: &FsEnv,
    report: &check::Report,
) {
    output.general_messages = messages(&report.globals);
    let matches: Vec<Vec<(CleaningStatus, &Cleaning)>> = order
        .iter()
        .map(|&index| cleaning_matches(&facts[index], sorted_env))
        .collect();
    let defined: HashSet<&str> = metadata
        .groups
        .iter()
        .map(|group| group.name.as_str())
        .collect();
    output.plugins = report
        .plugins
        .iter()
        .zip(order)
        .zip(&matches)
        .map(|((plugin, &index), matched)| {
            let fact = &facts[index];
            let dirty: Vec<&Cleaning> = matched
                .iter()
                .filter(|(status, _)| *status == CleaningStatus::Dirty)
                .map(|&(_, data)| data)
                .collect();
            let generated = validate::plugin_messages(
                sorted_env,
                &validate::Plugin {
                    name: fact.name,
                    active: plugin.active,
                    header: fact.header.as_ref(),
                    entries: &fact.entries,
                    groups: &defined,
                    group: fact.entries.iter().find_map(|entry| entry.group.as_deref()),
                    dirty: &dirty,
                },
            );
            let bash_tags = fact
                .header
                .as_ref()
                .and_then(|header| header.description.as_deref())
                .map(validate::description_tags)
                .unwrap_or_default();
            plugin_result(plugin, &generated, bash_tags)
        })
        .collect();
    output.dirty = dirty_info(order, facts, &matches);
    let active = report.plugins.iter().filter(|plugin| plugin.active).count();
    output.generated_general_messages = messages(
        &validate::active_plugin_count(active)
            .into_iter()
            .collect::<Vec<_>>(),
    );
}

/// The record ids of every installed plugin whose header could be read. A file that cannot be read as records is
/// noted and counts as one without; a limit is an error.
fn read_all_records(
    env: &FsEnv,
    facts: &[Facts],
    warnings: &mut Vec<String>,
) -> Result<Vec<PluginRecords>, SortError> {
    let mut names = NameIds::default();
    let mut budget = MAX_RECORDS_TOTAL;
    let mut all = Vec::with_capacity(facts.len());
    for fact in facts {
        let path = env.resolve(fact.name).filter(|_| fact.readable);
        let Some(path) = path else {
            all.push(PluginRecords::default());
            continue;
        };
        match records::read_records(&path, fact.name, &fact.masters, &mut names, &mut budget) {
            Ok(records) => all.push(records),
            Err(RecordsError::TooMany) => return Err(SortError::TooMuchWork),
            Err(error) => {
                warnings.push(format!(
                    "{}: records not readable ({error}); it is treated as having none",
                    fact.name
                ));
                all.push(PluginRecords::default());
            }
        }
    }
    Ok(all)
}

/// The files of the archives each installed plugin loads. An archive that cannot be read is noted and skipped; a
/// limit is an error.
fn read_all_assets(
    env: &FsEnv,
    data_dir: &Path,
    facts: &[Facts],
    warnings: &mut Vec<String>,
) -> Result<Vec<PluginAssets>, SortError> {
    let mut ids = PathIds::default();
    let mut budget = MAX_ASSETS_TOTAL;
    let mut all = Vec::with_capacity(facts.len());
    for fact in facts {
        // As libloot does, archives are taken from every directory searched, the additional ones first.
        let found: Vec<PathBuf> = env
            .additional_data_dirs()
            .iter()
            .map(PathBuf::as_path)
            .chain(std::iter::once(data_dir))
            .flat_map(|dir| archives::associated_archives(dir, fact.name))
            .collect();
        let mut skipped = Vec::new();
        match archives::read_plugin_assets(&found, &mut ids, &mut budget, &mut skipped) {
            Ok(assets) => all.push(assets),
            Err(ArchiveError::TooMuch) => return Err(SortError::TooMuchWork),
            Err(error) => {
                // Not produced by `read_plugin_assets`, which reports everything else through `skipped`.
                warnings.push(format!("{}: archives not read ({error})", fact.name));
                all.push(PluginAssets::default());
            }
        }
        for (path, error) in skipped {
            let file = path
                .file_name()
                .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
            warnings.push(format!("{}: archive {file} not read ({error})", fact.name));
        }
    }
    Ok(all)
}

/// The plugin that Fallout: New Vegas always loads first.
const EARLY_LOADER: &str = "FalloutNV.esm";

/// What the sorter is told about one plugin: the header and the masterlist rules that apply.
fn plugin_input(fact: &Facts, rules: &Rules, index: usize) -> PluginInput {
    let mut input = PluginInput::new(fact.name);
    input.is_master = fact.is_master;
    input.group.clone_from(&fact.group);
    input.masters.clone_from(&fact.masters);
    for &(before, later, source) in &rules.hard {
        if later != index {
            continue;
        }
        match source {
            RuleSource::After => input.load_after.push(rules.names[before].clone()),
            RuleSource::Req => input.requirements.push(rules.names[before].clone()),
            // Header masters come from the plugin itself; group rules from the group graph.
            RuleSource::HeaderMaster | RuleSource::Group => {}
        }
    }
    input
}

fn sort_error(error: &GraphError, name_of: &dyn Fn(usize) -> String) -> SortError {
    match error {
        GraphError::Cycle(cycle) => {
            let first = cycle.first().map_or(0, |&(plugin, _)| plugin);
            let second = cycle.get(1).map_or(first, |&(plugin, _)| plugin);
            SortError::Conflict {
                first: name_of(first),
                second: name_of(second),
            }
        }
        GraphError::MasterOrder { plugin, master, .. } => SortError::MasterOrder {
            plugin: name_of(*plugin),
            master: name_of(*master),
        },
        GraphError::TooMany(count) => SortError::TooManyPlugins(*count),
        GraphError::GroupCycle(groups) => SortError::GroupCycle(groups.clone()),
        GraphError::TooMuchWork => SortError::TooMuchWork,
        GraphError::Internal(text) => SortError::Internal((*text).to_string()),
    }
}

/// The rules that were kept, once each.
fn ordering_rules(
    rules: &Rules,
    dropped: &[(usize, usize)],
    name_of: &dyn Fn(usize) -> String,
) -> Vec<OrderingRule> {
    let soft = rules
        .soft
        .iter()
        .filter(|edge| !dropped.contains(edge))
        .map(|&(a, b)| (a, b, RuleSource::Group));
    let mut seen = HashSet::new();
    rules
        .hard
        .iter()
        .copied()
        .chain(soft)
        .filter(|&(a, b, source)| seen.insert((a, b, source.label())))
        .map(|(a, b, source)| OrderingRule {
            before: name_of(a),
            after: name_of(b),
            source,
        })
        .collect()
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
    let readable = header.is_ok();
    let (is_master, masters) = match &header {
        Ok(header) => (header.is_master(), header.masters.clone()),
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
        readable,
        header: header.ok(),
        is_master,
        masters,
        entries,
        group,
    }
}

/// Edges `(earlier, later)` over indices of the installed plugins.
type Edges = Vec<(usize, usize)>;

/// The edges that must hold with where each comes from, and those that hold when they can.
struct Rules {
    hard: Vec<(usize, usize, RuleSource)>,
    soft: Edges,
    /// Names of the installed plugins, by index.
    names: Vec<String>,
}

/// The edges `(earlier, later)` over the indices of `facts`: those that must hold (masters, `after`, `req`) and
/// those that hold when they can (groups), the latter in an order that depends on names only.
fn rules(env: &FsEnv, metadata: &Metadata, facts: &[Facts], warnings: &mut Vec<String>) -> Rules {
    let index_of: HashMap<String, usize> = facts
        .iter()
        .enumerate()
        .map(|(index, fact)| (fact.name.to_lowercase(), index))
        .collect();
    let mut conditions = Conditions { env, warnings };
    let mut hard: Vec<(usize, usize, RuleSource)> = Vec::new();
    for (index, fact) in facts.iter().enumerate() {
        for master in &fact.masters {
            if let Some(&before) = index_of.get(&master.to_lowercase()) {
                hard.push((before, index, RuleSource::HeaderMaster));
            }
        }
        let wanted = fact.entries.iter().flat_map(|entry| {
            let after = entry.after.iter().map(|file| (file, RuleSource::After));
            after.chain(entry.req.iter().map(|file| (file, RuleSource::Req)))
        });
        for (file, source) in wanted {
            let Some(&before) = index_of.get(&file.name.to_lowercase()) else {
                continue;
            };
            if conditions.holds(file.condition.as_deref(), fact.name)
                && (file.constraint.is_none()
                    || conditions.holds(file.constraint.as_deref(), fact.name))
            {
                hard.push((before, index, source));
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
    Rules {
        hard,
        soft,
        names: facts.iter().map(|fact| fact.name.to_string()).collect(),
    }
}

fn plugin_result(
    plugin: &check::PluginReport,
    generated: &[check::AppliedMessage],
    bash_tags: Vec<String>,
) -> PluginResult {
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
        generated_messages: messages(generated),
        bash_tags,
    }
}

fn io_error(text: &str) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::NotFound, text.to_string())
}

/// The cleaning data whose checksum is the installed file's. Only files that have such data are read.
fn dirty_info(
    order: &[usize],
    facts: &[Facts],
    matches: &[Vec<(CleaningStatus, &Cleaning)>],
) -> Vec<DirtyInfo> {
    let mut found = Vec::new();
    for (&index, matched) in order.iter().zip(matches) {
        for &(status, data) in matched {
            found.push(DirtyInfo {
                plugin: facts[index].name.to_string(),
                // `cleaning_matches` only keeps data that has a checksum, which is the installed file's.
                crc: data.crc.unwrap_or_default(),
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

/// The cleaning data of the masterlist (dirty and clean) whose checksum is the installed file's. Only files that
/// have such data are read.
fn cleaning_matches<'a>(fact: &Facts<'a>, env: &FsEnv) -> Vec<(CleaningStatus, &'a Cleaning)> {
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
        return Vec::new();
    }
    let Some(crc) = env
        .resolve(fact.name)
        .and_then(|path| crc32_file(&path).ok())
    else {
        return Vec::new();
    };
    all.into_iter()
        .filter(|(_, data)| data.crc == Some(crc))
        .collect()
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
