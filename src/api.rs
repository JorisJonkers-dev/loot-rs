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

use crate::archives::{ArchiveIndex, PluginAssets};
use crate::check::{self, FindingKind as InnerKind};
use crate::condition::{ConditionError, eval_str};
use crate::env::{FsEnv, Slot, crc32_file};
use crate::explain;
use crate::load;
pub use crate::load::SortCache;
use crate::metadata::{
    self, Cleaning, DEFAULT_GROUP, MessageKind as InnerMessageKind, Metadata, PluginEntry,
};
use crate::overlap::{PairNeeds, PluginOverlaps};
use crate::parallel;
pub use crate::parallel::CancelToken;
use crate::plugin;
pub use crate::plugin::{PluginError, PluginHeader};
use crate::records::PluginRecords;
pub use crate::render::{CHECK_CONTRACT, CONTRACTS, SCHEMA_VERSION, SORT_CONTRACT, contracts_json};
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
    digest: String,
    prelude_digest: Option<String>,
    source: Option<MasterlistSource>,
}

/// What LOOT recorded about where a masterlist came from, read from the `<masterlist>.metadata.toml` file next to it
/// (LOOT writes `masterlist.yaml.metadata.toml` when it updates the masterlist).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct MasterlistSource {
    /// `blob_sha1`: the git blob id of the masterlist file LOOT downloaded (compare with [`Masterlist::digest`]).
    pub blob_sha1: String,
    /// `update_timestamp`, as LOOT wrote it (a date), when present.
    pub update_timestamp: Option<String>,
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

impl MasterlistError {
    /// A stable code for the variant: `masterlist-io` or `masterlist-invalid`. A code never changes meaning.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Io(_) => "masterlist-io",
            Self::Invalid(_) => "masterlist-invalid",
        }
    }

    /// The error as one block of plain text a person can copy into a bug report; see [`copy_text`].
    #[must_use]
    pub fn copy_text(&self) -> String {
        copy_text(self.code(), self)
    }
}

/// The version of this library API: the crate version, which every release's notes and tag carry. It follows
/// semantic versioning (see the module documentation).
pub const API_VERSION: &str = env!("CARGO_PKG_VERSION");

/// One block of plain text for an error, to show and to copy into a bug report: `loot-rs <version>`, the stable
/// `code` and the message, on three lines. The home directory in the message is written as `~`, as `$HOME` names it
/// (without trailing `/`; without `$HOME`, or when it is `/`, nothing is replaced). It never contains a command line.
#[must_use]
pub fn copy_text(code: &str, error: &dyn fmt::Display) -> String {
    let message = error.to_string();
    let message = match std::env::var("HOME") {
        Ok(home) => tilde(&message, &trim_home(&home)),
        Err(_) => message,
    };
    format!("loot-rs {API_VERSION}\nerror: {code}\n{message}")
}

/// `home` as [`copy_text`] matches it: without trailing `/` (as `$HOME` may end with one), and empty when that leaves
/// nothing, so that `/` (the root, not a home directory) replaces nothing.
fn trim_home(home: &str) -> String {
    home.trim_end_matches('/').to_string()
}

/// `text` with every `home` that starts a path (not inside a longer path, and followed by `/` or the end of a word)
/// written as `~`. An empty `home` (or `/`, which has no trailing `/` to trim) replaces nothing.
fn tilde(text: &str, home: &str) -> String {
    if home.is_empty() {
        return text.to_string();
    }
    // What can follow a path in a message: a character that is not part of one, or the end of the text. A name that
    // continues the path (`/home/someone2`) does not count.
    let path_char = |c: char| c.is_alphanumeric() || matches!(c, '/' | '.' | '_' | '-' | '~');
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find(home) {
        out.push_str(&rest[..at]);
        rest = &rest[at + home.len()..];
        let starts = !out.chars().next_back().is_some_and(path_char);
        let ends = rest
            .chars()
            .next()
            .is_none_or(|c| c == '/' || !path_char(c));
        if starts && ends {
            out.push('~');
        } else {
            out.push_str(home);
        }
    }
    out.push_str(rest);
    out
}

/// Most bytes read from a `.metadata.toml` file.
const MAX_SOURCE_FILE: u64 = 64 * 1024;

/// A UTF-8 text file of at most [`metadata::MAX_MASTERLIST_BYTES`] bytes.
fn read_text(path: &Path) -> Result<String, MasterlistError> {
    let io = |error: &dyn fmt::Display| MasterlistError::Io(format!("{}: {error}", path.display()));
    let limit = metadata::MAX_MASTERLIST_BYTES as u64;
    let file = fs::File::open(path).map_err(|error| io(&error))?;
    let mut bytes = Vec::new();
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| io(&error))?;
    if bytes.len() as u64 > limit {
        return Err(io(&format!("larger than the limit of {limit} bytes")));
    }
    String::from_utf8(bytes).map_err(|_| io(&"not valid UTF-8"))
}

/// `key = "value"` lines of LOOT's `.metadata.toml` file next to `masterlist`; `None` when there is no such file or
/// it names no `blob_sha1`.
fn read_source(masterlist: &Path) -> Option<MasterlistSource> {
    let mut name = masterlist.file_name()?.to_os_string();
    name.push(".metadata.toml");
    let file = fs::File::open(masterlist.with_file_name(name)).ok()?;
    let mut text = String::new();
    file.take(MAX_SOURCE_FILE).read_to_string(&mut text).ok()?;
    let value = |key: &str| {
        text.lines().find_map(|line| {
            let (name, value) = line.split_once('=')?;
            (name.trim() == key).then(|| {
                let value = value.trim();
                value
                    .strip_prefix('"')
                    .and_then(|v| v.strip_suffix('"'))
                    .or_else(|| value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')))
                    .unwrap_or(value)
                    .to_string()
            })
        })
    };
    Some(MasterlistSource {
        blob_sha1: value("blob_sha1")?.to_ascii_lowercase(),
        update_timestamp: value("update_timestamp"),
    })
}

impl Masterlist {
    /// Read a masterlist from YAML text.
    ///
    /// # Errors
    /// [`MasterlistError::Invalid`] for text that is not a masterlist or is over a limit.
    pub fn parse(text: &str) -> Result<Self, MasterlistError> {
        Self::build(text, None)
    }

    /// Read a masterlist from YAML text, with its `prelude:` section replaced by `prelude` (the text of LOOT's
    /// separate prelude file) as libloot does it (see `docs/API.md`). A masterlist without a block-style `prelude:`
    /// key is read as it is.
    ///
    /// # Errors
    /// As [`Masterlist::parse`], for the text after the replacement.
    pub fn parse_with_prelude(text: &str, prelude: &str) -> Result<Self, MasterlistError> {
        Self::build(text, Some(prelude))
    }

    fn build(text: &str, prelude: Option<&str>) -> Result<Self, MasterlistError> {
        let digest = crate::sha1::git_blob_sha1(text.as_bytes());
        let replaced = prelude.map(|prelude| metadata::replace_prelude(text, prelude));
        let metadata = metadata::parse(replaced.as_deref().unwrap_or(text))
            .map_err(|error| MasterlistError::Invalid(error.0))?;
        Ok(Self {
            metadata,
            digest,
            prelude_digest: prelude.map(|prelude| crate::sha1::git_blob_sha1(prelude.as_bytes())),
            source: None,
        })
    }

    /// Read a masterlist from a file, and the `.metadata.toml` file next to it if there is one (see
    /// [`Masterlist::source`]).
    ///
    /// # Errors
    /// [`MasterlistError::Io`] when the file cannot be read as UTF-8 text or is over the size limit,
    /// [`MasterlistError::Invalid`] as for [`Masterlist::parse`].
    pub fn from_file(path: &Path) -> Result<Self, MasterlistError> {
        let mut masterlist = Self::parse(&read_text(path)?)?;
        masterlist.source = read_source(path);
        Ok(masterlist)
    }

    /// [`Masterlist::from_file`] with the prelude replaced by the file at `prelude` (see
    /// [`Masterlist::parse_with_prelude`]).
    ///
    /// # Errors
    /// As [`Masterlist::from_file`], for either file.
    pub fn from_file_with_prelude(path: &Path, prelude: &Path) -> Result<Self, MasterlistError> {
        let prelude = read_text(prelude)?;
        let mut masterlist = Self::parse_with_prelude(&read_text(path)?, &prelude)?;
        masterlist.source = read_source(path);
        Ok(masterlist)
    }

    /// Entries that were skipped while reading (for example a regular expression with look-around).
    #[must_use]
    pub fn warnings(&self) -> &[String] {
        &self.metadata.warnings
    }

    /// The git blob id (`sha1("blob <length>\0" + bytes)`, what `git hash-object` prints) of the masterlist text as
    /// read, before any prelude replacement: the value LOOT keeps as `blob_sha1`.
    #[must_use]
    pub fn digest(&self) -> &str {
        &self.digest
    }

    /// The git blob id of the prelude text, when one replaced the masterlist's.
    #[must_use]
    pub fn prelude_digest(&self) -> Option<&str> {
        self.prelude_digest.as_deref()
    }

    /// What LOOT's `.metadata.toml` file next to the masterlist says, when [`Masterlist::from_file`] found one.
    #[must_use]
    pub const fn source(&self) -> Option<&MasterlistSource> {
        self.source.as_ref()
    }

    /// Whether the text read is the one the `.metadata.toml` file names (`None` without such a file).
    #[must_use]
    pub fn matches_source(&self) -> Option<bool> {
        self.source
            .as_ref()
            .map(|source| source.blob_sha1 == self.digest)
    }
}

/// A userlist (LOOT's `userlist.yaml`): the user's own plugin metadata, groups, general messages and known Bash
/// Tags, merged over the masterlist by [`sort_with`] as libloot merges them. Read with the same limits and alias
/// rules as a masterlist. loot-rs does not write userlists: a caller writes the YAML itself, in the masterlist's
/// shape (`plugins`, `groups`, `globals`, `bash_tags`; see `docs/API.md`).
#[derive(Debug, Clone)]
pub struct Userlist {
    metadata: Metadata,
    digest: String,
}

impl Userlist {
    /// Read a userlist from YAML text.
    ///
    /// # Errors
    /// [`MasterlistError::Invalid`] for text that is not metadata or is over a limit.
    pub fn parse(text: &str) -> Result<Self, MasterlistError> {
        let metadata = metadata::parse(text).map_err(|error| MasterlistError::Invalid(error.0))?;
        Ok(Self {
            metadata,
            digest: crate::sha1::git_blob_sha1(text.as_bytes()),
        })
    }

    /// Read a userlist from a file.
    ///
    /// # Errors
    /// [`MasterlistError::Io`] when the file cannot be read as UTF-8 text or is over the size limit, otherwise as
    /// [`Userlist::parse`].
    pub fn from_file(path: &Path) -> Result<Self, MasterlistError> {
        Self::parse(&read_text(path)?)
    }

    /// Entries that were skipped while reading.
    #[must_use]
    pub fn warnings(&self) -> &[String] {
        &self.metadata.warnings
    }

    /// The git blob id of the text read (see [`Masterlist::digest`]).
    #[must_use]
    pub fn digest(&self) -> &str {
        &self.digest
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

/// How [`sort`] orders the plugins.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum SortMode {
    /// libloot's full sort: the order LOOT would produce (the default).
    #[default]
    Full,
    /// Keep the given order wherever no hard constraint is violated and move the fewest plugins that fix the
    /// violations. Hard constraints are the master flag (masters first), header masters, `req`, `after` and the
    /// game's first plugin, plus the group rules libloot satisfies when `strict_groups` is set. Soft group rules
    /// and overlap are not enforced. The reason for each move is in [`SortOutput::moves`]. See `docs/API.md`.
    MinimalChange,
}

impl SortMode {
    /// A short word for output and flags: `full` or `minimal`.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::MinimalChange => "minimal",
        }
    }
}

/// The class of the rule that forced a plugin to move (see [`PluginMove`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum MoveReason {
    /// A plugin with the master flag has to load before one without it.
    MasterFlag,
    /// The game loads the other plugin first (`FalloutNV.esm`).
    Hardcoded,
    /// The other plugin is a master of this one in its header, or the reverse.
    HeaderMaster,
    /// The masterlist's `req` entry.
    Req,
    /// The masterlist's `after` entry.
    After,
    /// The group rules (in a minimal-change sort only with `strict_groups`).
    Group,
    /// The two plugins change the same records and the other changes more (full sort only).
    RecordOverlap,
    /// The two plugins load archives with the same files and the other loads more (full sort only).
    AssetOverlap,
    /// The edge the sort adds to keep the given order where nothing else decides (full sort only; it is a reason
    /// only on a path, never on its own).
    TieBreak,
    /// The userlist's `after` entry.
    UserAfter,
    /// The userlist's `req` entry.
    UserReq,
    /// The group rules, where the userlist defines a group on the path or sets the group of one of the plugins.
    UserGroup,
}

impl MoveReason {
    /// A short word for output: `master-flag`, `hardcoded`, `header-master`, `req`, `after`, `group`,
    /// `record-overlap`, `asset-overlap`, `tie-break`, `user-after`, `user-req` or `user-group`.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::MasterFlag => "master-flag",
            Self::Hardcoded => "hardcoded",
            Self::HeaderMaster => "header-master",
            Self::Req => "req",
            Self::After => "after",
            Self::Group => "group",
            Self::RecordOverlap => "record-overlap",
            Self::AssetOverlap => "asset-overlap",
            Self::TieBreak => "tie-break",
            Self::UserAfter => "user-after",
            Self::UserReq => "user-req",
            Self::UserGroup => "user-group",
        }
    }
}

/// One reason a plugin moved: a rule between it and `other` that puts it on the other side of `other` than in the
/// given order.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct MoveExplanation {
    /// The class of the rule.
    pub reason: MoveReason,
    /// The other plugin.
    pub other: String,
    /// True when the rule puts this plugin after `other` (it moved later), false when before.
    pub later: bool,
    /// True when the rule is not between the two plugins directly: a chain of rules connects them, and `via` is the
    /// link of the chain that goes against the given order.
    pub indirect: bool,
    /// For an indirect reason, that link: `(loads first, loads later)`.
    pub via: Option<(String, String)>,
    /// For a group rule: the groups of the rule's two plugins, the earlier one's first.
    pub groups: Option<(String, String)>,
    /// For an overlap rule: the override records (record overlap) or archive files (asset overlap) of the rule's two
    /// plugins, the earlier one's first; the one with more loads first.
    pub counts: Option<(usize, usize)>,
}

/// A plugin that moved, and why: in [`SortOutput::explanations`] every installed plugin whose place changed, with
/// every reason found (at least one); in [`SortOutput::moves`] each plugin a minimal-change sort moved, with the
/// constraint that forced it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct PluginMove {
    /// The plugin that moved.
    pub plugin: String,
    /// Its position among the installed plugins in the given order.
    pub from: usize,
    /// Its position in [`SortOutput::order`].
    pub to: usize,
    /// The class of the first violated constraint that forced the move.
    pub reason: MoveReason,
    /// The other plugin of that constraint.
    pub other: String,
    /// True when the constraint follows from a chain of constraints rather than being stated directly; `reason`
    /// is then the class of the first link.
    pub indirect: bool,
    /// Every reason, the one above first. Names and counts only, never file contents.
    pub reasons: Vec<MoveExplanation>,
}

/// Switches for [`sort`].
///
/// Not `Copy` since it holds directories; clone it where a copy is needed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
// Independent on/off switches, each with its own builder; an enum per switch would add nothing.
#[allow(clippy::struct_excessive_bools)]
pub struct SortOptions {
    /// Count group violations and undefined groups as hard findings (see `docs/GROUPS.md`). Off by default.
    pub strict_groups: bool,
    /// The game directory: what a leading `..` in a condition path names (`../nvse_1_4.dll`). When unset, `..` is
    /// the parent of the directory searched.
    pub game_dir: Option<PathBuf>,
    /// Further directories that hold plugins and files, searched in this order before `data_dir`. When a file is
    /// in several directories, the first one that has it is used (see `docs/API.md`).
    pub additional_data_dirs: Vec<PathBuf>,
    /// How to order the plugins (default [`SortMode::Full`]).
    pub mode: SortMode,
    /// Threads used to read plugins and archives and to compare them: 0 (the default) is one per available core, at
    /// most [`crate::parallel::MAX_THREADS`]. The result is the same for every value.
    pub threads: usize,
    /// What earlier sorts read, reused when the files are unchanged (see [`SortCache`]). The result is the same with
    /// or without it.
    pub cache: Option<SortCache>,
    /// Set it (from any thread) to stop the sort: it returns [`SortError::Cancelled`] soon after, having written
    /// nothing and left no threads running. Checked before each file and inside the graph search.
    pub cancel: Option<CancelToken>,
    /// Called with the stage and counts as the sort goes, from the sort's threads (see [`Progress`]); [`check`]
    /// reports only the stages it has.
    pub progress: Option<ProgressHook>,
    /// Languages for message text, most wanted first (`de`, `pt_BR`, ...): each in turn by libloot's rule (the
    /// language itself, or one with the same language code), then English, then the first text. Empty (the default):
    /// English.
    pub languages: Vec<String>,
    /// Do not read whole plugins for the CRC-32 of the masterlist's cleaning data: [`SortOutput::dirty`] stays empty
    /// and no cleaning message is generated. `checksum()` conditions are still evaluated. The order is unchanged.
    pub skip_checksums: bool,
    /// Do not read BSA tables: no asset overlap edges, so the order can differ from LOOT's where two plugins load
    /// archives with the same files and nothing else decides between them.
    pub skip_archives: bool,
    /// Do not read executables' version resources (`version()` and `product_version()` on `.exe` and `.dll` files,
    /// cargo feature `executable-versions`): such conditions evaluate as unsupported and their items are left out.
    pub skip_executable_versions: bool,
}

impl SortOptions {
    /// Choose the sort mode.
    #[must_use]
    pub const fn with_mode(mut self, mode: SortMode) -> Self {
        self.mode = mode;
        self
    }

    /// Stop the sort when `cancel` is set; see [`SortOptions::cancel`].
    #[must_use]
    pub fn with_cancel(mut self, cancel: CancelToken) -> Self {
        self.cancel = Some(cancel);
        self
    }

    /// Report progress to `hook`; see [`SortOptions::progress`].
    #[must_use]
    pub fn with_progress(mut self, hook: ProgressHook) -> Self {
        self.progress = Some(hook);
        self
    }

    /// Skip checksums, archive tables and executable versions: what a caller that wants only the order (and accepts
    /// that archive overlap is not used) needs. See the three `skip_` fields.
    #[must_use]
    pub const fn order_only(mut self) -> Self {
        self.skip_checksums = true;
        self.skip_archives = true;
        self.skip_executable_versions = true;
        self
    }

    /// Set [`SortOptions::skip_checksums`].
    #[must_use]
    pub const fn with_skip_checksums(mut self, skip: bool) -> Self {
        self.skip_checksums = skip;
        self
    }

    /// Set [`SortOptions::skip_archives`].
    #[must_use]
    pub const fn with_skip_archives(mut self, skip: bool) -> Self {
        self.skip_archives = skip;
        self
    }

    /// Set [`SortOptions::skip_executable_versions`].
    #[must_use]
    pub const fn with_skip_executable_versions(mut self, skip: bool) -> Self {
        self.skip_executable_versions = skip;
        self
    }

    /// Prefer these languages for message text; see [`SortOptions::languages`].
    #[must_use]
    pub fn with_languages<I, S>(mut self, languages: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.languages = languages.into_iter().map(Into::into).collect();
        self
    }

    /// Reuse what earlier sorts with the same cache read; see [`SortCache`].
    #[must_use]
    pub fn with_cache(mut self, cache: SortCache) -> Self {
        self.cache = Some(cache);
        self
    }

    /// Use this many threads (0: one per available core); see [`SortOptions::threads`].
    #[must_use]
    pub const fn with_threads(mut self, threads: usize) -> Self {
        self.threads = threads;
        self
    }

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

/// A stage of [`sort`], for [`Progress`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Stage {
    /// Plugin headers and records; `total` is the installed plugins.
    ReadingPlugins,
    /// Archive tables; `total` is the archives.
    ReadingArchives,
    /// Comparing the plugins' records and archive files; `total` is the installed plugins.
    ComparingPlugins,
    /// The graph sort; `total` is 1.
    Sorting,
    /// The masterlist's messages, findings and checksums over the sorted order (over the order given, for
    /// [`check`]); `total` is 1.
    Checking,
}

impl Stage {
    /// A short word for output: `reading-plugins`, `reading-archives`, `comparing-plugins`, `sorting` or `checking`.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::ReadingPlugins => "reading-plugins",
            Self::ReadingArchives => "reading-archives",
            Self::ComparingPlugins => "comparing-plugins",
            Self::Sorting => "sorting",
            Self::Checking => "checking",
        }
    }
}

/// How far a sort is: `done` of `total` in `stage`. Each stage is reported first with `done` 0; stages come in the
/// order of [`Stage`] (one is left out when there is nothing to do in it). Counts only, never names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Progress {
    /// What the sort is doing.
    pub stage: Stage,
    /// Items done in this stage.
    pub done: usize,
    /// Items in this stage.
    pub total: usize,
}

/// A function [`sort`] calls with its [`Progress`] (see [`SortOptions::with_progress`]). It is called from the
/// sort's threads, one call at a time and with the counts of a stage in order, so it must be `Send + Sync` and quick;
/// it must not call [`sort`] itself.
#[derive(Clone)]
pub struct ProgressHook(std::sync::Arc<dyn Fn(Progress) + Send + Sync>);

impl ProgressHook {
    /// A hook that calls `f`.
    pub fn new(f: impl Fn(Progress) + Send + Sync + 'static) -> Self {
        Self(std::sync::Arc::new(f))
    }
}

impl fmt::Debug for ProgressHook {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ProgressHook")
    }
}

/// Two hooks are equal when they are clones of the same one.
impl PartialEq for ProgressHook {
    fn eq(&self, other: &Self) -> bool {
        std::sync::Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for ProgressHook {}

/// Reports the progress of one stage.
struct Reporter<'a> {
    hook: Option<&'a ProgressHook>,
    stage: Stage,
    total: usize,
    /// Items done; the lock is held while the hook is called, so the hook sees the counts in order, one call at a
    /// time, even when the items are finished on several threads.
    done: std::sync::Mutex<usize>,
}

impl<'a> Reporter<'a> {
    /// Start `stage` (reported with `done` 0).
    fn start(hook: Option<&'a ProgressHook>, stage: Stage, total: usize) -> Self {
        let reporter = Self {
            hook,
            stage,
            total,
            done: std::sync::Mutex::new(0),
        };
        reporter.call(0);
        reporter
    }

    /// One more item done.
    fn tick(&self) {
        if self.hook.is_some() {
            let mut done = self
                .done
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            *done += 1;
            self.call((*done).min(self.total));
        }
    }

    fn call(&self, done: usize) {
        if let Some(hook) = self.hook {
            (hook.0)(Progress {
                stage: self.stage,
                done,
                total: self.total,
            });
        }
    }
}

/// [`SortError::Cancelled`] when `options` asks the sort to stop.
fn check_cancel(options: &SortOptions) -> Result<(), SortError> {
    if parallel::cancelled(options.cancel.as_ref()) {
        Err(SortError::Cancelled)
    } else {
        Ok(())
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
    /// The text, substitutions applied: Markdown as the masterlist writes it (see `docs/API.md`), in the language
    /// [`SortOptions::languages`] chose.
    pub text: String,
    /// The language of `text`: the masterlist's code (`en`, `de`, `pt_BR`, ...); `en` for text that is not localised
    /// and for the messages LOOT generates.
    pub lang: String,
}

/// A place where a plugin can be found: the masterlist's `url` entries.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Location {
    /// The URL, as the masterlist writes it; never checked or fetched.
    pub url: String,
    /// The name of the page or site, when the masterlist gives one.
    pub name: Option<String>,
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

/// A master of a plugin that is not available in the expected state.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct MissingMaster {
    /// The file name of the master as given in the plugin's header.
    pub name: String,
    /// Whether the master is missing or merely inactive.
    pub state: MasterState,
}

/// The state of a missing master.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum MasterState {
    /// The master is not installed.
    Missing,
    /// The master is installed but not active.
    Inactive,
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
    /// The masterlist's (and userlist's) `url` entries, each once, the userlist's first.
    pub locations: Vec<Location>,
    /// The plugin's masters that are not installed or not active, in the order of its header; empty when the plugin
    /// itself is inactive.
    pub missing_masters: Vec<MissingMaster>,
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
    /// The userlist's `after` entry.
    UserAfter,
    /// The userlist's `req` entry.
    UserReq,
    /// The groups of the two plugins, where the userlist set one of the groups.
    UserGroup,
}

impl RuleSource {
    /// A short word for output: `header-master`, `after`, `req`, `group`, `user-after`, `user-req` or `user-group`.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::HeaderMaster => "header-master",
            Self::After => "after",
            Self::Req => "req",
            Self::Group => "group",
            Self::UserAfter => "user-after",
            Self::UserReq => "user-req",
            Self::UserGroup => "user-group",
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
    /// The mode that produced the order.
    pub mode: SortMode,
    /// Plugins that moved and why; empty unless the mode is [`SortMode::MinimalChange`].
    pub moves: Vec<PluginMove>,
    /// Every installed plugin whose place differs from its place in the given order, in the sorted order, with every
    /// reason found (see [`PluginMove`]). In a full sort the reasons come from the edges the sorter added; in a
    /// minimal-change sort this is [`SortOutput::moves`]. A plugin that kept its place is not listed.
    pub explanations: Vec<PluginMove>,
}

impl SortOutput {
    /// The mode, and the moves into `explanations` (and into `moves` for a minimal-change sort).
    fn set_moves(&mut self, mode: SortMode, moves: Vec<PluginMove>) {
        self.mode = mode;
        if mode == SortMode::MinimalChange {
            self.explanations.clone_from(&moves);
            self.moves = moves;
        } else {
            self.explanations = moves;
        }
    }

    /// Findings that fail a check (`hard`), over every plugin: the `hard_violations` of `loot-rs sort --json`.
    #[must_use]
    pub fn hard_violations(&self) -> usize {
        self.plugins
            .iter()
            .flat_map(|plugin| &plugin.findings)
            .filter(|finding| finding.hard)
            .count()
    }

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
    /// The caller cancelled the sort ([`SortOptions::cancel`]).
    Cancelled,
    /// Rules that cannot all hold, at least one of them from the userlist: each plugin of the cycle with the rule that
    /// makes it load before the next one (the last before the first). A cycle among masterlist rules and header
    /// masters only is [`SortError::Conflict`].
    RuleCycle(Vec<CycleLink>),
}

/// One link of a [`SortError::RuleCycle`]: `plugin` has to load before the next plugin of the cycle because of `rule`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct CycleLink {
    /// The plugin.
    pub plugin: String,
    /// The class of the rule (for example [`MoveReason::UserAfter`]).
    pub rule: MoveReason,
}

impl SortError {
    /// A stable code for the variant, for programs and translations: `data-directory`, `too-many-plugins`,
    /// `conflict`, `master-order`, `group-cycle`, `too-much-work`, `internal`, `cancelled` or `rule-cycle`. A code
    /// never changes meaning; a new variant gets a new code.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::DataDirectory(_) => "data-directory",
            Self::TooManyPlugins(_) => "too-many-plugins",
            Self::Conflict { .. } => "conflict",
            Self::MasterOrder { .. } => "master-order",
            Self::GroupCycle(_) => "group-cycle",
            Self::TooMuchWork => "too-much-work",
            Self::Internal(_) => "internal",
            Self::Cancelled => "cancelled",
            Self::RuleCycle(_) => "rule-cycle",
        }
    }
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
            Self::Cancelled => f.write_str("the sort was cancelled"),
            Self::RuleCycle(links) => {
                f.write_str("rules that cannot all hold:")?;
                for link in links {
                    write!(f, " {} ({}) before", link.plugin, link.rule.label())?;
                }
                match links.first() {
                    Some(first) => write!(f, " {}", first.plugin),
                    None => Ok(()),
                }
            }
        }
    }
}

impl std::error::Error for SortError {}

impl SortError {
    /// The error as one block of plain text a person can copy into a bug report; see [`copy_text`].
    #[must_use]
    pub fn copy_text(&self) -> String {
        copy_text(self.code(), self)
    }
}

fn matching<'a>(metadata: &'a Metadata, name: &str) -> Vec<&'a PluginEntry> {
    metadata.entries_for(name)
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
            lang: message.lang.clone(),
        })
        .collect()
}

/// What each installed plugin is, from its header and the masterlist, in load order.
struct Facts<'a> {
    name: &'a str,
    /// Where the plugin was found.
    path: Option<PathBuf>,
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
    sort_with(load_order, masterlist, None, data_dir, options)
}

/// [`sort`] with a userlist merged over the masterlist, as libloot does with its userlist loaded: for each plugin the
/// userlist's entries come first (a group it sets wins; its `after`, `req`, `inc`, messages, tags and cleaning data
/// come before the masterlist's), its groups are added to the group graph after the masterlist's (a userlist group may
/// load after a masterlist group), its `after` and `req` give sorting edges of their own, and its general messages
/// and Bash Tags follow the masterlist's. Rules from the userlist have the sources [`RuleSource::UserAfter`],
/// [`RuleSource::UserReq`] and [`RuleSource::UserGroup`]; a cycle through one is [`SortError::RuleCycle`].
///
/// # Errors
/// As [`sort`].
pub fn sort_with(
    load_order: &[PluginSlot],
    masterlist: &Masterlist,
    userlist: Option<&Userlist>,
    data_dir: &Path,
    options: &SortOptions,
) -> Result<SortOutput, SortError> {
    if !data_dir.is_dir() {
        return Err(SortError::DataDirectory(data_dir.to_path_buf()));
    }
    if load_order.len() > MAX_PLUGINS * 4 {
        return Err(SortError::TooManyPlugins(load_order.len()));
    }
    let merged = userlist.map(|userlist| metadata::merge(&userlist.metadata, &masterlist.metadata));
    let metadata = merged.as_ref().unwrap_or(&masterlist.metadata);
    let mut output = SortOutput::default();
    let slots = distinct_slots(load_order, &mut output.warnings);
    let env = search_env(data_dir, slots, options);
    check_directories(&env, options, &mut output.warnings);
    let (installed, not_installed): (Vec<&Slot>, Vec<&Slot>) = env
        .slots()
        .iter()
        .partition(|slot| env.installed(&slot.name));
    output.not_installed = not_installed.iter().map(|slot| slot.name.clone()).collect();
    if installed.len() > MAX_PLUGINS {
        return Err(SortError::TooManyPlugins(installed.len()));
    }

    let threads = parallel::threads(options.threads);
    let hook = options.progress.as_ref();
    // The records of every plugin tick this stage, the headers are read with it.
    let reading = Reporter::start(hook, Stage::ReadingPlugins, installed.len());
    let facts = all_facts(
        &env,
        metadata,
        &installed,
        threads,
        options,
        &mut output.warnings,
        &|| {},
    );
    check_cancel(options)?;
    let rules = rules(&env, metadata, &facts, &mut output.warnings);

    let name_of = |index: usize| facts[index].name.to_string();
    let records = read_all_records(&facts, threads, options, &reading, &mut output.warnings)?;
    let assets = if options.skip_archives {
        vec![PluginAssets::default(); facts.len()]
    } else {
        read_all_assets(
            &env,
            data_dir,
            &facts,
            threads,
            options,
            &mut output.warnings,
        )?
    };
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
    let comparing = Reporter::start(hook, Stage::ComparingPlugins, inputs.len());
    let overlaps = overlaps_of(&inputs, records, assets, threads, options, &comparing)?;
    let groups = group_inputs(&masterlist.metadata);
    let user_groups = userlist.map_or_else(Vec::new, |userlist| group_inputs(&userlist.metadata));
    let sorting = Reporter::start(hook, Stage::Sorting, 1);
    let (order, moves) =
        order_plugins(&inputs, &groups, &user_groups, &overlaps, options, &name_of)?;
    sorting.tick();
    let checking = Reporter::start(hook, Stage::Checking, 1);
    output.set_moves(options.mode, moves);
    record_order(&mut output, &rules, &order, &name_of);

    // What the masterlist says, with the plugins in their sorted places: a rule that the sort could not satisfy
    // shows up as a finding.
    let sorted_slots: Vec<Slot> = order
        .iter()
        .map(|&index| installed[index].clone())
        .collect();
    let sorted_env = search_env(data_dir, sorted_slots, options);
    // The checksums the cleaning data needs; the check below and `describe` reuse them.
    if !options.skip_checksums {
        prefetch_checksums(&facts, &sorted_env, threads, options.cancel.as_ref());
    }
    check_cancel(options)?;
    let report = check::run_with_languages(
        metadata,
        &sorted_env,
        check::Options {
            strict_groups: options.strict_groups,
            fixed_first: &[EARLY_LOADER],
        },
        &options.languages,
    );
    check_cancel(options)?;
    describe(
        &mut output,
        metadata,
        &facts,
        &order,
        &sorted_env,
        &report,
        options.skip_checksums,
    );
    checking.tick();
    Ok(output)
}

/// The result of [`check`]: what the masterlist and LOOT's own checks say about the load order as given.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct CheckOutput {
    /// One entry per installed plugin, in the order given.
    pub plugins: Vec<PluginResult>,
    /// Messages of the masterlist that apply to no plugin in particular.
    pub general_messages: Vec<Message>,
    /// Messages LOOT generates about the load order as a whole (more active plugins than the game supports).
    pub generated_general_messages: Vec<Message>,
    /// Problems of a general message whose condition could not be evaluated, so the message was not applied.
    pub general_findings: Vec<Finding>,
    /// Cleaning data that matches installed files.
    pub dirty: Vec<DirtyInfo>,
    /// Plugins of the load order that are not in the data directories, in the order given.
    pub not_installed: Vec<String>,
    /// Things that were skipped: masterlist and userlist entries the metadata reader left out, unreadable plugin
    /// headers, duplicate names, directories that are not ones.
    pub warnings: Vec<String>,
}

impl CheckOutput {
    /// Findings that fail a check (`hard`), over every plugin and the general findings.
    #[must_use]
    pub fn hard_violations(&self) -> usize {
        self.plugins
            .iter()
            .flat_map(|plugin| &plugin.findings)
            .chain(&self.general_findings)
            .filter(|finding| finding.hard)
            .count()
    }
}

/// Check a load order as it is, without sorting it: the messages, Bash Tags, cleaning data and findings (unmet
/// `req`, present `inc`, violated `after` and group rules) of every installed plugin in the order given. This is
/// what `loot-rs check` reports. `userlist`, when given, is merged over the masterlist as in [`sort_with`].
///
/// `options` supplies the languages, `strict_groups`, the game and additional data directories, the cancel token,
/// the progress hook (stages [`Stage::ReadingPlugins`], one tick per plugin whose header was looked at, and
/// [`Stage::Checking`]) and the skip switches; the mode, the threads beyond reading headers and the cache do not
/// matter. Nothing is written. [`CheckOutput::warnings`] holds what `loot-rs check` lists as notes: the entries the
/// masterlist and the userlist left out, duplicate names, directories that are not ones and unreadable headers.
///
/// # Errors
/// [`SortError::DataDirectory`], [`SortError::TooManyPlugins`] or [`SortError::Cancelled`].
pub fn check(
    load_order: &[PluginSlot],
    masterlist: &Masterlist,
    userlist: Option<&Userlist>,
    data_dir: &Path,
    options: &SortOptions,
) -> Result<CheckOutput, SortError> {
    if !data_dir.is_dir() {
        return Err(SortError::DataDirectory(data_dir.to_path_buf()));
    }
    if load_order.len() > MAX_PLUGINS * 4 {
        return Err(SortError::TooManyPlugins(load_order.len()));
    }
    let merged = userlist.map(|userlist| metadata::merge(&userlist.metadata, &masterlist.metadata));
    let metadata = merged.as_ref().unwrap_or(&masterlist.metadata);
    let mut warnings = metadata.warnings.clone();
    let slots = distinct_slots(load_order, &mut warnings);
    let env = search_env(data_dir, slots, options);
    check_directories(&env, options, &mut warnings);
    // Installed-ness is decided once for every listed plugin, so the two lists below cannot disagree.
    let found: Vec<bool> = env
        .slots()
        .iter()
        .map(|slot| env.installed(&slot.name))
        .collect();
    let installed: Vec<&Slot> = env
        .slots()
        .iter()
        .zip(&found)
        .filter(|&(_, &present)| present)
        .map(|(slot, _)| slot)
        .collect();
    let not_installed: Vec<String> = env
        .slots()
        .iter()
        .zip(&found)
        .filter(|&(_, &present)| !present)
        .map(|(slot, _)| slot.name.clone())
        .collect();
    if installed.len() > MAX_PLUGINS {
        return Err(SortError::TooManyPlugins(installed.len()));
    }
    let threads = parallel::threads(options.threads);
    let hook = options.progress.as_ref();
    // No stage between reading the headers and the checks: the first stage ends with `done == total`.
    let reading = Reporter::start(hook, Stage::ReadingPlugins, installed.len());
    let facts = all_facts(
        &env,
        metadata,
        &installed,
        threads,
        options,
        &mut warnings,
        &|| reading.tick(),
    );
    check_cancel(options)?;
    let checking = Reporter::start(hook, Stage::Checking, 1);
    if !options.skip_checksums {
        prefetch_checksums(&facts, &env, threads, options.cancel.as_ref());
    }
    check_cancel(options)?;
    let report = check::run_with_languages(
        metadata,
        &env,
        check::Options {
            strict_groups: options.strict_groups,
            fixed_first: &[EARLY_LOADER],
        },
        &options.languages,
    );
    check_cancel(options)?;
    // The plugins are checked in the order given, so the places are that order.
    let order: Vec<usize> = (0..facts.len()).collect();
    let mut output = SortOutput::default();
    describe(
        &mut output,
        metadata,
        &facts,
        &order,
        &env,
        &report,
        options.skip_checksums,
    );
    checking.tick();
    Ok(CheckOutput {
        plugins: output.plugins,
        general_messages: output.general_messages,
        generated_general_messages: output.generated_general_messages,
        general_findings: findings(&report.general),
        dirty: output.dirty,
        not_installed,
        warnings,
    })
}

fn group_inputs(metadata: &Metadata) -> Vec<GroupInput> {
    metadata
        .groups
        .iter()
        .map(|group| GroupInput {
            name: group.name.clone(),
            after: group.after.clone(),
        })
        .collect()
}

/// The overlap data of the installed plugins, with every pair the sorter can ask about compared on `threads`
/// threads.
fn overlaps_of(
    inputs: &[PluginInput],
    records: Vec<PluginRecords>,
    assets: Vec<PluginAssets>,
    threads: usize,
    options: &SortOptions,
    reporter: &Reporter,
) -> Result<PluginOverlaps, SortError> {
    let mut overlaps = PluginOverlaps::new(
        records.into_iter().map(|plugin| plugin.keys).collect(),
        assets.into_iter().map(|plugin| plugin.keys).collect(),
    )
    .with_cancel(options.cancel.clone());
    let is_master: Vec<bool> = inputs.iter().map(|input| input.is_master).collect();
    let override_records: Vec<usize> = inputs.iter().map(|input| input.override_records).collect();
    let asset_counts: Vec<usize> = inputs.iter().map(|input| input.assets).collect();
    overlaps
        .precompute(
            PairNeeds {
                is_master: &is_master,
                override_records: &override_records,
                assets: &asset_counts,
            },
            threads,
            &|| reporter.tick(),
        )
        .map_err(|()| {
            if parallel::cancelled(options.cancel.as_ref()) {
                SortError::Cancelled
            } else {
                SortError::TooMuchWork
            }
        })?;
    Ok(overlaps)
}

/// Read, on `threads` threads, the CRC-32 of every plugin whose cleaning data needs it, and give them to `env`.
fn prefetch_checksums(facts: &[Facts], env: &FsEnv, threads: usize, cancel: Option<&CancelToken>) {
    let wanted: Vec<PathBuf> = facts
        .iter()
        .filter(|fact| needs_checksum(fact))
        .filter_map(|fact| fact.path.clone())
        .collect();
    let crcs = parallel::map(&wanted, threads, |path| {
        if parallel::cancelled(cancel) {
            None
        } else {
            crc32_file(path).ok()
        }
    });
    if parallel::cancelled(cancel) {
        return;
    }
    env.remember_crcs(wanted.into_iter().zip(crcs));
}

/// Why [`evaluate`] gave no answer.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum EvaluateError {
    /// `data_dir` is not a directory.
    DataDirectory(PathBuf),
    /// The condition is not valid LOOT condition syntax or is over a limit, or a file it names could not be read; the
    /// text says why.
    Invalid(String),
    /// The condition uses a function this crate does not evaluate (named).
    Unsupported(String),
}

impl EvaluateError {
    /// A stable code for the variant: `data-directory`, `invalid` or `unsupported`.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::DataDirectory(_) => "data-directory",
            Self::Invalid(_) => "invalid",
            Self::Unsupported(_) => "unsupported",
        }
    }
}

impl fmt::Display for EvaluateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DataDirectory(path) => write!(f, "{}: not a directory", path.display()),
            Self::Invalid(text) => f.write_str(text),
            Self::Unsupported(name) => write!(f, "unsupported condition function {name}()"),
        }
    }
}

impl std::error::Error for EvaluateError {}

impl EvaluateError {
    /// The error as one block of plain text a person can copy into a bug report; see [`copy_text`].
    #[must_use]
    pub fn copy_text(&self) -> String {
        copy_text(self.code(), self)
    }
}

/// Evaluate one LOOT condition string (for example `file("NVSE/nvse_1_4.dll") and not active("Old.esp")`) against
/// a load order and its data directory, as [`sort`] evaluates the masterlist's conditions: a tester for a doctor
/// command. An empty condition is true. `options` supplies the game and additional data directories and the
/// executable-version switch; the other options do not matter. Nothing is written.
///
/// # Errors
/// [`EvaluateError`] for a data directory that is not one, a condition that is not valid, or one that uses a function
/// this crate cannot evaluate.
pub fn evaluate(
    condition: &str,
    load_order: &[PluginSlot],
    data_dir: &Path,
    options: &SortOptions,
) -> Result<bool, EvaluateError> {
    if !data_dir.is_dir() {
        return Err(EvaluateError::DataDirectory(data_dir.to_path_buf()));
    }
    let mut ignored = Vec::new();
    let env = search_env(data_dir, distinct_slots(load_order, &mut ignored), options);
    eval_str(condition, &env).map_err(|error| match error {
        ConditionError::Unsupported(name) => EvaluateError::Unsupported(name),
        other => EvaluateError::Invalid(other.to_string()),
    })
}

/// The sorted order, the group rules it does not satisfy and the rules it honours, into `output`.
fn record_order(
    output: &mut SortOutput,
    rules: &Rules,
    order: &[usize],
    name_of: &dyn Fn(usize) -> String,
) {
    let position: Vec<usize> = {
        let mut position = vec![0; order.len()];
        for (at, &index) in order.iter().enumerate() {
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
    output.order = order.iter().map(|&index| name_of(index)).collect();
    output.ordering_rules = ordering_rules(rules, &dropped, name_of);
}

/// The environment over `data_dir` and the directories of `options`.
fn search_env(data_dir: &Path, slots: Vec<Slot>, options: &SortOptions) -> FsEnv {
    FsEnv::new(data_dir, slots)
        .with_game_dir(options.game_dir.as_deref())
        .with_additional_data_dirs(&options.additional_data_dirs)
        .with_executable_versions(!options.skip_executable_versions)
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
    skip_checksums: bool,
) {
    output.general_messages = messages(&report.globals);
    let matches: Vec<Vec<(CleaningStatus, &Cleaning)>> = order
        .iter()
        .map(|&index| {
            if skip_checksums {
                Vec::new()
            } else {
                cleaning_matches(&facts[index], sorted_env)
            }
        })
        .collect();
    let defined: HashSet<&str> = metadata
        .groups
        .iter()
        .map(|group| group.name.as_str())
        .collect();
    // The group LOOT generates a message about: the first one the entries set, as the check takes it. The message
    // asks whether the groups define it, so a group the userlist sets is defined by the merge.
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
            let validate_plugin = validate::Plugin {
                name: fact.name,
                active: plugin.active,
                header: fact.header.as_ref(),
                entries: &fact.entries,
                groups: &defined,
                group: named_group(&fact.entries),
                dirty: &dirty,
            };
            let generated = validate::plugin_messages(sorted_env, &validate_plugin);
            let bash_tags = fact
                .header
                .as_ref()
                .and_then(|header| header.description.as_deref())
                .map_or_else(Vec::new, validate::description_tags);
            let missing_masters = missing_masters_list(sorted_env, &validate_plugin);
            let mut result = plugin_result(plugin, &generated, bash_tags, missing_masters);
            result.locations = locations(&fact.entries);
            result
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

/// The record ids of every installed plugin whose header could be read (see `load::read_records`).
fn read_all_records(
    facts: &[Facts],
    threads: usize,
    options: &SortOptions,
    reporter: &Reporter,
    warnings: &mut Vec<String>,
) -> Result<Vec<PluginRecords>, SortError> {
    let jobs: Vec<load::RecordJob> = facts
        .iter()
        .map(|fact| load::RecordJob {
            name: fact.name,
            path: fact.path.as_deref().filter(|_| fact.readable),
            masters: &fact.masters,
        })
        .collect();
    let hooks = load::Hooks {
        cancel: options.cancel.as_ref(),
        done: &|| reporter.tick(),
        limits: load::Limits::DEFAULT,
    };
    load::read_records(&jobs, threads, options.cache.as_ref(), hooks, warnings).map_err(stop_error)
}

/// The files of the archives each installed plugin loads (see `load::read_assets`). As libloot does, archives are
/// taken from every directory searched, the additional ones first. The stage is reported per archive, and the
/// archives are read on `threads` threads, except when the caller cancelled: then one after the other, so that the
/// sort stops at the first archive.
fn read_all_assets(
    env: &FsEnv,
    data_dir: &Path,
    facts: &[Facts],
    threads: usize,
    options: &SortOptions,
    warnings: &mut Vec<String>,
) -> Result<Vec<PluginAssets>, SortError> {
    let dirs: Vec<&Path> = env
        .additional_data_dirs()
        .iter()
        .map(PathBuf::as_path)
        .chain(std::iter::once(data_dir))
        .collect();
    let index = ArchiveIndex::new(&dirs);
    let names: Vec<&str> = facts.iter().map(|fact| fact.name).collect();
    let found: Vec<Vec<PathBuf>> = names.iter().map(|name| index.archives_of(name)).collect();
    let mut distinct: Vec<&PathBuf> = found.iter().flatten().collect();
    distinct.sort();
    distinct.dedup();
    let reporter = Reporter::start(
        options.progress.as_ref(),
        Stage::ReadingArchives,
        distinct.len(),
    );
    let threads = if parallel::cancelled(options.cancel.as_ref()) {
        1
    } else {
        threads
    };
    let hooks = load::Hooks {
        cancel: options.cancel.as_ref(),
        done: &|| reporter.tick(),
        limits: load::Limits::DEFAULT,
    };
    load::read_assets(
        &names,
        &found,
        threads,
        options.cache.as_ref(),
        hooks,
        warnings,
    )
    .map_err(stop_error)
}

const fn stop_error(stop: load::Stop) -> SortError {
    match stop {
        load::Stop::OverLimit => SortError::TooMuchWork,
        load::Stop::Cancelled => SortError::Cancelled,
    }
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
            RuleSource::UserAfter => input.user_load_after.push(rules.names[before].clone()),
            RuleSource::UserReq => input.user_requirements.push(rules.names[before].clone()),
            // Header masters come from the plugin itself; group rules from the group graph.
            RuleSource::HeaderMaster | RuleSource::Group | RuleSource::UserGroup => {}
        }
    }
    input.group_is_user = rules.user_group[index];
    input
}

/// The new order (indices of `inputs`) by the mode of `options`, and the moves a minimal-change sort made.
fn order_plugins(
    inputs: &[PluginInput],
    groups: &[GroupInput],
    user_groups: &[GroupInput],
    overlaps: &PluginOverlaps,
    options: &SortOptions,
    name_of: &dyn Fn(usize) -> String,
) -> Result<(Vec<usize>, Vec<PluginMove>), SortError> {
    let early = [EARLY_LOADER.to_string()];
    match options.mode {
        SortMode::Full => {
            let sorted = sort::sort_with_user_groups(inputs, groups, user_groups, &early, overlaps)
                .map_err(|error| sort_error(&error, name_of))?;
            let moves = explained_moves(inputs, &sorted, name_of);
            Ok((sorted.order, moves))
        }
        SortMode::MinimalChange => {
            let repaired = sort::sort_minimal_with_user_groups(
                inputs,
                groups,
                user_groups,
                &early,
                overlaps,
                options.strict_groups,
            )
            .map_err(|error| sort_error(&error, name_of))?;
            let moves = repaired
                .moves
                .iter()
                .map(|moved| {
                    let reason = move_reason(moved.class);
                    PluginMove {
                        plugin: name_of(moved.plugin),
                        from: moved.from,
                        to: moved.to,
                        reason,
                        other: name_of(moved.other),
                        indirect: moved.indirect,
                        reasons: vec![MoveExplanation {
                            reason,
                            other: name_of(moved.other),
                            later: moved.to > moved.from,
                            indirect: moved.indirect,
                            via: None,
                            groups: None,
                            counts: None,
                        }],
                    }
                })
                .collect();
            Ok((repaired.order, moves))
        }
    }
}

fn move_reason(class: sort::EdgeType) -> MoveReason {
    match class {
        sort::EdgeType::Hardcoded => MoveReason::Hardcoded,
        sort::EdgeType::Master => MoveReason::HeaderMaster,
        sort::EdgeType::MasterlistRequirement => MoveReason::Req,
        sort::EdgeType::MasterlistLoadAfter => MoveReason::After,
        sort::EdgeType::MasterlistGroup => MoveReason::Group,
        sort::EdgeType::RecordOverlap => MoveReason::RecordOverlap,
        sort::EdgeType::AssetOverlap => MoveReason::AssetOverlap,
        sort::EdgeType::TieBreak => MoveReason::TieBreak,
        sort::EdgeType::UserRequirement => MoveReason::UserReq,
        sort::EdgeType::UserLoadAfter => MoveReason::UserAfter,
        sort::EdgeType::UserGroup => MoveReason::UserGroup,
        _ => MoveReason::MasterFlag,
    }
}

/// The moves of a full sort, each with the reasons `explain` finds in the edges the sorter added, in the sorted order.
fn explained_moves(
    inputs: &[PluginInput],
    sorted: &sort::Sorted,
    name_of: &dyn Fn(usize) -> String,
) -> Vec<PluginMove> {
    let is_master: Vec<bool> = inputs.iter().map(|input| input.is_master).collect();
    let mut moved = explain::explain(&sorted.order, &sorted.edges, &is_master);
    moved.sort_by_key(|entry| entry.to);
    moved
        .into_iter()
        .filter_map(|entry| {
            let reasons: Vec<MoveExplanation> = entry
                .reasons
                .iter()
                .map(|reason| move_explanation(inputs, entry.plugin, reason, name_of))
                .collect();
            let first = reasons.first()?.clone();
            Some(PluginMove {
                plugin: name_of(entry.plugin),
                from: entry.plugin,
                to: entry.to,
                reason: first.reason,
                other: first.other,
                indirect: first.indirect,
                reasons,
            })
        })
        .collect()
}

fn move_explanation(
    inputs: &[PluginInput],
    plugin: usize,
    reason: &explain::Reason,
    name_of: &dyn Fn(usize) -> String,
) -> MoveExplanation {
    let class = match reason.cause {
        explain::Cause::Edge(kind) => move_reason(kind),
        explain::Cause::MasterFlag => MoveReason::MasterFlag,
    };
    // The rule's two plugins, the one that loads first first: the link of the chain, or the two plugins.
    let (first, second) = reason.via.unwrap_or(if reason.later {
        (reason.other, plugin)
    } else {
        (plugin, reason.other)
    });
    let groups = matches!(class, MoveReason::Group | MoveReason::UserGroup)
        .then(|| (inputs[first].group.clone(), inputs[second].group.clone()));
    let counts = match class {
        MoveReason::RecordOverlap => Some((
            inputs[first].override_records,
            inputs[second].override_records,
        )),
        MoveReason::AssetOverlap => Some((inputs[first].assets, inputs[second].assets)),
        _ => None,
    };
    MoveExplanation {
        reason: class,
        other: name_of(reason.other),
        later: reason.later,
        indirect: reason.via.is_some(),
        via: reason.via.map(|(from, to)| (name_of(from), name_of(to))),
        groups,
        counts,
    }
}

fn sort_error(error: &GraphError, name_of: &dyn Fn(usize) -> String) -> SortError {
    match error {
        GraphError::Cycle(cycle)
            if cycle.iter().any(|&(_, kind)| {
                matches!(
                    kind,
                    sort::EdgeType::UserLoadAfter
                        | sort::EdgeType::UserRequirement
                        | sort::EdgeType::UserGroup
                )
            }) =>
        {
            SortError::RuleCycle(
                cycle
                    .iter()
                    .map(|&(plugin, kind)| CycleLink {
                        plugin: name_of(plugin),
                        rule: move_reason(kind),
                    })
                    .collect(),
            )
        }
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
        GraphError::Cancelled => SortError::Cancelled,
        GraphError::Internal(text) => SortError::Internal((*text).to_string()),
    }
}

/// The rules that were kept, once each.
fn ordering_rules(
    rules: &Rules,
    dropped: &[(usize, usize)],
    name_of: &dyn Fn(usize) -> String,
) -> Vec<OrderingRule> {
    let dropped: HashSet<(usize, usize)> = dropped.iter().copied().collect();
    let soft = rules
        .soft
        .iter()
        .filter(|edge| !dropped.contains(edge))
        .map(|&(a, b)| {
            let user = rules.user_group[a] || rules.user_group[b];
            (
                a,
                b,
                if user {
                    RuleSource::UserGroup
                } else {
                    RuleSource::Group
                },
            )
        });
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

/// The facts of every installed plugin, with the headers read on `threads` threads; `done` is called once per header
/// looked at, from the thread that looked at it. When the caller cancelled, the headers are looked at one after the
/// other, so that a cancelled check stops at the first one instead of reading the whole load order.
fn all_facts<'a>(
    env: &FsEnv,
    metadata: &'a Metadata,
    installed: &[&'a Slot],
    threads: usize,
    options: &SortOptions,
    warnings: &mut Vec<String>,
    done: &(dyn Fn() + Sync),
) -> Vec<Facts<'a>> {
    let paths: Vec<Option<PathBuf>> = installed
        .iter()
        .map(|slot| env.resolve(&slot.name))
        .collect();
    let threads = if parallel::cancelled(options.cancel.as_ref()) {
        1
    } else {
        threads
    };
    let headers = parallel::map(&paths, threads, |path| {
        let header = match path {
            _ if parallel::cancelled(options.cancel.as_ref()) => Err(io_error("cancelled")),
            Some(path) => plugin::read_header(path),
            None => Err(io_error("not found")),
        };
        done();
        header
    });
    installed
        .iter()
        .zip(paths)
        .zip(headers)
        .map(|((slot, path), header)| facts_of(metadata, slot, path, header, warnings))
        .collect()
}

/// The header and the masterlist entries of one installed plugin.
fn facts_of<'a>(
    metadata: &'a Metadata,
    slot: &'a Slot,
    path: Option<PathBuf>,
    header: std::io::Result<plugin::PluginHeader>,
    warnings: &mut Vec<String>,
) -> Facts<'a> {
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
        path,
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
    /// Whether the userlist set the group of each installed plugin.
    user_group: Vec<bool>,
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
            let (after, req) = if entry.user {
                (RuleSource::UserAfter, RuleSource::UserReq)
            } else {
                (RuleSource::After, RuleSource::Req)
            };
            let afters = entry.after.iter().map(move |file| (file, after));
            afters.chain(entry.req.iter().map(move |file| (file, req)))
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
    // The groups of the plugins as small numbers, and for each pair of them whether the first loads after the
    // second (and not the reverse), so the loop over pairs of plugins does no string lookups.
    let mut group_number: HashMap<&str, usize> = HashMap::new();
    let numbers: Vec<usize> = facts
        .iter()
        .map(|fact| {
            let next = group_number.len();
            *group_number.entry(fact.group.as_str()).or_insert(next)
        })
        .collect();
    let mut names: Vec<&str> = vec![""; group_number.len()];
    for (&name, &number) in &group_number {
        names[number] = name;
    }
    let loads_after: Vec<Vec<bool>> = names
        .iter()
        .map(|&group| {
            let after = closure.get(group).filter(|_| defined.contains(group));
            names
                .iter()
                .map(|&other| {
                    let reverse = closure.get(other).is_some_and(|set| set.contains(group));
                    after.is_some_and(|after| after.contains(other)) && !reverse
                })
                .collect()
        })
        .collect();
    let mut soft: Vec<(usize, usize)> = Vec::new();
    for &later in &by_name {
        let row = &loads_after[numbers[later]];
        if !row.iter().any(|&after| after) {
            continue;
        }
        for &earlier in &by_name {
            if earlier != later && row[numbers[earlier]] {
                soft.push((earlier, later));
            }
        }
    }
    Rules {
        hard,
        soft,
        names: facts.iter().map(|fact| fact.name.to_string()).collect(),
        user_group: facts
            .iter()
            .map(|fact| {
                fact.entries
                    .iter()
                    .find(|entry| entry.group.is_some())
                    .is_some_and(|entry| entry.user)
            })
            .collect(),
    }
}

fn missing_masters_list(env: &FsEnv, plugin: &validate::Plugin<'_>) -> Vec<MissingMaster> {
    let mut ev = check::Evaluator::new(env);
    let loot_tags = validate::tag_names(&mut ev, plugin);
    validate::missing_masters(env, plugin, &loot_tags)
        .into_iter()
        .map(|(name, installed)| MissingMaster {
            name,
            state: if installed {
                MasterState::Inactive
            } else {
                MasterState::Missing
            },
        })
        .collect()
}

fn plugin_result(
    plugin: &check::PluginReport,
    generated: &[check::AppliedMessage],
    bash_tags: Vec<String>,
    missing_masters: Vec<MissingMaster>,
) -> PluginResult {
    PluginResult {
        name: plugin.name.clone(),
        position: plugin.position,
        active: plugin.active,
        group: plugin.group.clone(),
        messages: messages(&plugin.messages),
        tags_added: plugin.tags_add.clone(),
        tags_removed: plugin.tags_remove.clone(),
        findings: findings(&plugin.findings),
        generated_messages: messages(generated),
        bash_tags,
        locations: Vec::new(),
        missing_masters,
    }
}

/// The findings of a check report, for the API.
fn findings(found: &[check::Finding]) -> Vec<Finding> {
    found
        .iter()
        .map(|finding| Finding {
            kind: finding.kind.into(),
            text: finding.text.clone(),
            hard: finding.hard,
        })
        .collect()
}

/// The `url` entries of a plugin's entries, each once, in entry order (the userlist's first).
fn locations(entries: &[&PluginEntry]) -> Vec<Location> {
    let mut found: Vec<Location> = Vec::new();
    for location in entries.iter().flat_map(|entry| &entry.url) {
        let location = Location {
            url: location.url.clone(),
            name: location.name.clone(),
        };
        if !found.contains(&location) {
            found.push(location);
        }
    }
    found
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

/// True when the masterlist has cleaning data with a checksum for the plugin, so its CRC-32 is needed.
fn needs_checksum(fact: &Facts) -> bool {
    fact.entries
        .iter()
        .flat_map(|entry| entry.dirty.iter().chain(&entry.clean))
        .any(|data| data.crc.is_some())
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
    let Some(crc) = fact.path.as_deref().and_then(|path| env.crc_of(path)) else {
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

/// The group LOOT generates a message about: the first one the entries set.
fn named_group<'a>(entries: &[&'a PluginEntry]) -> Option<&'a str> {
    entries.iter().find_map(|entry| entry.group.as_deref())
}
