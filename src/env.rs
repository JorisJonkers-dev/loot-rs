//! The load order and the `Data` directory that conditions and rules are evaluated against.
//!
//! Paths in metadata are relative to the `Data` directory and are matched case-insensitively
//! (the game's file system is, a Linux one is not).
//!
//! # Where a path is looked for
//!
//! A load order can be spread over several directories (a mod manager's staging tree next to the game's own
//! `Data`). [`FsEnv`] searches, for a path or a name pattern, the additional data directories in the order given
//! and then the `Data` directory, as `loot-condition-interpreter` does. When a file exists in several of them, the
//! first directory that has it wins, for its contents as well as for its existence: that is the file that is read
//! for a header, records, a checksum or a version. A leading `..` names the game directory when one is set
//! ([`FsEnv::with_game_dir`]) and the parent of the searched directory when not.
//!
//! A plugin stored as `Name.esp.ghost` (or `.esm.ghost`) counts as `Name.esp`, as in libloot. Two names in one
//! directory that differ only in case are matched in sorted order and reported by [`FsEnv::case_warnings`].

use crate::condition::{Arg, ConditionError, Env};
use crate::metadata::{compile_whole_name, looks_like_regex};
use crate::plugin;
use regex::Regex;
use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// One line of the plugin list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Slot {
    /// Plugin file name as written.
    pub name: String,
    /// Whether the plugin is active.
    pub active: bool,
}

/// Parse a `plugins.txt`. Blank lines and `#` comments are skipped. When any line starts with
/// `*`, only starred lines are active (the `*` style); otherwise every listed plugin is
/// active (the active-only style MO2 writes for New Vegas). The list order is the load order.
#[must_use]
pub fn parse_plugins_txt(text: &str) -> Vec<Slot> {
    let lines: Vec<&str> = text
        .split('\n')
        .map(|line| line.trim_end_matches('\r').trim())
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect();
    let starred = lines.iter().any(|line| line.starts_with('*'));
    lines
        .iter()
        .map(|line| match line.strip_prefix('*') {
            Some(name) => Slot {
                name: name.trim().to_string(),
                active: true,
            },
            None => Slot {
                name: (*line).to_string(),
                active: !starred,
            },
        })
        .collect()
}

#[allow(clippy::cast_possible_truncation)]
const fn crc_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut index = 0;
    while index < 256 {
        let mut value = index as u32; // index < 256
        let mut bit = 0;
        while bit < 8 {
            value = if value & 1 != 0 {
                0xEDB8_8320 ^ (value >> 1)
            } else {
                value >> 1
            };
            bit += 1;
        }
        table[index] = value;
        index += 1;
    }
    table
}

/// Slicing-by-8 tables: `CRC_TABLES[k][b]` is the CRC of byte `b` followed by `k` zero bytes.
const fn crc_tables() -> [[u32; 256]; 8] {
    let mut tables = [[0u32; 256]; 8];
    tables[0] = crc_table();
    let mut k = 1;
    while k < 8 {
        let mut index = 0;
        while index < 256 {
            let previous = tables[k - 1][index];
            tables[k][index] = (previous >> 8) ^ tables[0][(previous & 0xFF) as usize];
            index += 1;
        }
        k += 1;
    }
    tables
}

static CRC_TABLES: [[u32; 256]; 8] = crc_tables();

/// Continue a CRC-32 (in its inverted running form) over `bytes`, eight bytes per step.
fn crc32_update(mut crc: u32, bytes: &[u8]) -> u32 {
    let t = &CRC_TABLES;
    let (chunks, rest) = bytes.as_chunks::<8>();
    for chunk in chunks {
        let low = crc ^ u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        let high = u32::from_le_bytes([chunk[4], chunk[5], chunk[6], chunk[7]]);
        let [l0, l1, l2, l3] = low.to_le_bytes();
        let [h0, h1, h2, h3] = high.to_le_bytes();
        crc = t[7][usize::from(l0)]
            ^ t[6][usize::from(l1)]
            ^ t[5][usize::from(l2)]
            ^ t[4][usize::from(l3)]
            ^ t[3][usize::from(h0)]
            ^ t[2][usize::from(h1)]
            ^ t[1][usize::from(h2)]
            ^ t[0][usize::from(h3)];
    }
    for &byte in rest {
        crc = t[0][usize::from(crc.to_le_bytes()[0] ^ byte)] ^ (crc >> 8);
    }
    crc
}

/// CRC-32 (IEEE, as used by zip and by LOOT's file checksums) of `bytes`.
#[must_use]
pub fn crc32(bytes: &[u8]) -> u32 {
    !crc32_update(!0, bytes)
}

/// CRC-32 of a file, read in blocks so that a large plugin is never held in memory.
///
/// # Errors
/// When the file cannot be opened or read.
pub fn crc32_file(path: &Path) -> io::Result<u32> {
    use std::io::Read;
    let mut file = fs::File::open(path)?;
    let mut buffer = vec![0u8; 256 * 1024];
    let mut crc = !0u32;
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            return Ok(!crc);
        }
        crc = crc32_update(crc, &buffer[..read]);
    }
}

/// A load order over a `Data` directory on disk.
pub struct FsEnv {
    data: PathBuf,
    extra: Vec<PathBuf>,
    game: Option<PathBuf>,
    slots: Vec<Slot>,
    listings: RefCell<HashMap<PathBuf, Vec<String>>>,
    collisions: RefCell<BTreeSet<String>>,
    /// CRC-32 of files already read (`None`: not readable), so a file is read once per environment.
    crcs: RefCell<HashMap<PathBuf, Option<u32>>>,
    /// Compiled file-name regular expressions by pattern.
    regexes: RefCell<HashMap<String, Result<Regex, String>>>,
    /// Results of whole conditions by their text (see [`Env::remembered`]).
    results: RefCell<HashMap<String, Result<bool, ConditionError>>>,
}

const GHOST: &str = ".ghost";

/// True for a file name with a plugin extension (`.esp` or `.esm`), without regard to case.
fn has_plugin_extension(name: &str) -> bool {
    Path::new(name).extension().is_some_and(|extension| {
        extension.eq_ignore_ascii_case("esp") || extension.eq_ignore_ascii_case("esm")
    })
}

/// `name` without a `.ghost` suffix when what is left has a plugin extension, as libloot trims it.
fn unghosted(name: &str) -> &str {
    let lower = name.to_ascii_lowercase();
    match lower.strip_suffix(GHOST) {
        Some(stem) if has_plugin_extension(stem) => &name[..stem.len()],
        _ => name,
    }
}

impl FsEnv {
    /// Create the environment for `slots` (in load order) installed under `data`.
    #[must_use]
    pub fn new(data: &Path, slots: Vec<Slot>) -> Self {
        Self {
            data: data.to_path_buf(),
            extra: Vec::new(),
            game: None,
            slots,
            listings: RefCell::new(HashMap::new()),
            collisions: RefCell::new(BTreeSet::new()),
            crcs: RefCell::new(HashMap::new()),
            regexes: RefCell::new(HashMap::new()),
            results: RefCell::new(HashMap::new()),
        }
    }

    /// Set the game directory: what a leading `..` in a path names.
    #[must_use]
    pub fn with_game_dir(mut self, game: Option<&Path>) -> Self {
        self.game = game.map(Path::to_path_buf);
        self
    }

    /// Set the additional data directories, searched in this order before the `Data` directory.
    #[must_use]
    pub fn with_additional_data_dirs(mut self, dirs: &[PathBuf]) -> Self {
        self.extra = dirs.to_vec();
        self
    }

    /// The game directory, when one was set.
    #[must_use]
    pub fn game_dir(&self) -> Option<&Path> {
        self.game.as_deref()
    }

    /// The additional data directories, in search order.
    #[must_use]
    pub fn additional_data_dirs(&self) -> &[PathBuf] {
        &self.extra
    }

    /// The directories that are searched, in order: the additional ones, then the `Data` directory.
    fn roots(&self) -> impl Iterator<Item = &PathBuf> {
        self.extra.iter().chain(std::iter::once(&self.data))
    }

    /// One warning for every directory searched (and every other directory listed so far) that holds two names
    /// differing only in case. Such names cannot both exist on the game's file system; here the first in sorted
    /// order is used.
    #[must_use]
    pub fn case_warnings(&self) -> Vec<String> {
        for root in self.roots() {
            let _ = self.listing(root);
        }
        self.collisions.borrow().iter().cloned().collect()
    }

    /// The load order.
    #[must_use]
    pub fn slots(&self) -> &[Slot] {
        &self.slots
    }

    /// The `Data` directory.
    #[must_use]
    pub fn data_dir(&self) -> &Path {
        &self.data
    }

    /// The CRC-32 of the file at `path`, read once and then remembered.
    pub fn crc_of(&self, path: &Path) -> Option<u32> {
        if let Some(&crc) = self.crcs.borrow().get(path) {
            return crc;
        }
        let crc = crc32_file(path).ok();
        self.crcs.borrow_mut().insert(path.to_path_buf(), crc);
        crc
    }

    /// Remember CRC-32s computed elsewhere (for example on several threads), so they are not read again.
    pub fn remember_crcs(&self, crcs: impl IntoIterator<Item = (PathBuf, Option<u32>)>) {
        self.crcs.borrow_mut().extend(crcs);
    }

    fn listing(&self, dir: &Path) -> Result<Vec<String>, ConditionError> {
        if let Some(cached) = self.listings.borrow().get(dir) {
            return Ok(cached.clone());
        }
        let names: Vec<String> = match fs::read_dir(dir) {
            Ok(entries) => entries
                .filter_map(Result::ok)
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .collect(),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Vec::new(),
            Err(error) => {
                return Err(ConditionError::Environment(format!(
                    "{}: {error}",
                    dir.display()
                )));
            }
        };
        let mut names = names;
        names.sort();
        self.note_collisions(dir, &names);
        self.listings
            .borrow_mut()
            .insert(dir.to_path_buf(), names.clone());
        Ok(names)
    }

    fn note_collisions(&self, dir: &Path, names: &[String]) {
        let mut by_lower: HashMap<String, Vec<&str>> = HashMap::new();
        for name in names {
            by_lower.entry(name.to_lowercase()).or_default().push(name);
        }
        let mut groups: Vec<Vec<&str>> = by_lower.into_values().filter(|g| g.len() > 1).collect();
        groups.sort();
        for group in groups {
            self.collisions.borrow_mut().insert(format!(
                "{}: {} differ only in case; conditions match case-insensitively and use {}",
                dir.display(),
                group.join(", "),
                group[0]
            ));
        }
    }

    /// Resolve a `/`-separated path, matching each component case-insensitively: in the additional data
    /// directories in order, then in `Data`. The first directory that has the path wins. `None` when no directory
    /// has it. A plugin name also finds `Name.esp.ghost`.
    #[must_use]
    pub fn resolve(&self, relative: &str) -> Option<PathBuf> {
        self.roots()
            .find_map(|root| self.resolve_in(root, relative))
    }

    fn resolve_in(&self, root: &Path, relative: &str) -> Option<PathBuf> {
        let mut current = root.to_path_buf();
        let mut depth = 0usize;
        for part in relative.split('/').filter(|part| !part.is_empty()) {
            match part {
                "." => {}
                ".." => match &self.game {
                    Some(game) if depth == 0 => current.clone_from(game),
                    _ => {
                        current.pop();
                        depth = depth.saturating_sub(1);
                    }
                },
                name => {
                    depth += 1;
                    current = self.child(&current, name)?;
                }
            }
        }
        Some(current)
    }

    /// The entry of `dir` named `name` without regard to case, the exact spelling first.
    fn child(&self, dir: &Path, name: &str) -> Option<PathBuf> {
        let direct = dir.join(name);
        if direct.exists() {
            return Some(direct);
        }
        let lower = name.to_lowercase();
        let listing = self.listing(dir).ok()?;
        let found = listing
            .iter()
            .find(|candidate| candidate.to_lowercase() == lower)
            .or_else(|| {
                if !has_plugin_extension(name) {
                    return None;
                }
                let ghost = format!("{lower}{GHOST}");
                listing
                    .iter()
                    .find(|candidate| candidate.to_lowercase() == ghost)
            })?;
        Some(dir.join(found))
    }

    /// True when `name` is installed in the `Data` directory.
    #[must_use]
    pub fn installed(&self, name: &str) -> bool {
        self.resolve(name).is_some_and(|path| path.is_file())
    }

    /// True when `name` is in the plugin list, active, and installed.
    #[must_use]
    pub fn is_active(&self, name: &str) -> bool {
        let lower = name.to_lowercase();
        self.slots
            .iter()
            .any(|slot| slot.active && slot.name.to_lowercase() == lower)
            && self.installed(name)
    }

    /// Position of `name` in the load order, case-insensitively.
    #[must_use]
    pub fn position(&self, name: &str) -> Option<usize> {
        let lower = name.to_lowercase();
        self.slots
            .iter()
            .position(|slot| slot.name.to_lowercase() == lower)
    }

    /// The whole-name regular expression of `pattern`, compiled once per environment.
    fn whole_name(&self, pattern: &str) -> Result<Regex, ConditionError> {
        if let Some(found) = self.regexes.borrow().get(pattern) {
            return found.clone().map_err(ConditionError::Syntax);
        }
        let compiled = compile_whole_name(pattern);
        self.regexes
            .borrow_mut()
            .insert(pattern.to_string(), compiled.clone());
        compiled.map_err(ConditionError::Syntax)
    }

    fn regex_parts(path: &str) -> (&str, &str) {
        path.rsplit_once('/').unwrap_or(("", path))
    }
}

impl Env for FsEnv {
    fn count_files(&self, path: &str) -> Result<usize, ConditionError> {
        if !looks_like_regex(path) {
            return Ok(usize::from(self.resolve(path).is_some_and(|p| p.exists())));
        }
        let (dir, pattern) = Self::regex_parts(path);
        let regex = self.whole_name(pattern)?;
        let mut seen = HashSet::new();
        for root in self.roots() {
            let Some(dir) = self.resolve_in(root, dir) else {
                continue;
            };
            for name in self.listing(&dir)? {
                let name = unghosted(&name);
                if regex.is_match(name) {
                    seen.insert(name.to_lowercase());
                }
            }
        }
        Ok(seen.len())
    }

    fn count_active(&self, path: &str) -> Result<usize, ConditionError> {
        if !looks_like_regex(path) {
            return Ok(usize::from(self.is_active(path)));
        }
        let (dir, pattern) = Self::regex_parts(path);
        if !dir.is_empty() {
            return Ok(0);
        }
        let regex = self.whole_name(pattern)?;
        Ok(self
            .slots
            .iter()
            .filter(|slot| slot.active && regex.is_match(&slot.name) && self.installed(&slot.name))
            .count())
    }

    fn file_size(&self, path: &str) -> Option<u64> {
        let meta = fs::metadata(self.resolve(path)?).ok()?;
        meta.is_file().then_some(meta.len())
    }

    fn readable(&self, path: &str) -> bool {
        self.resolve(path)
            .is_some_and(|path| fs::File::open(&path).is_ok() || fs::read_dir(&path).is_ok())
    }

    fn is_master(&self, path: &str) -> bool {
        self.resolve(path)
            .and_then(|path| plugin::read_header(&path).ok())
            .is_some_and(|header| header.is_master())
    }

    fn checksum(&self, path: &str) -> Option<u32> {
        let path = self.resolve(path)?;
        if !path.is_file() {
            return None;
        }
        self.crc_of(&path)
    }

    fn remembered(&self, text: &str) -> Option<Result<bool, ConditionError>> {
        self.results.borrow().get(text).cloned()
    }

    fn remember(&self, text: &str, result: &Result<bool, ConditionError>) {
        self.results
            .borrow_mut()
            .insert(text.to_string(), result.clone());
    }

    fn version_call(&self, name: &str, args: &[Arg]) -> Result<bool, ConditionError> {
        let Some(Arg::Str(path)) = args.first() else {
            return Err(ConditionError::Syntax(format!(
                "{name}() expects a quoted string as argument 1"
            )));
        };
        if name == "version" && has_plugin_extension(unghosted(path)) {
            return self.plugin_version(path, args);
        }
        #[cfg(feature = "executable-versions")]
        {
            crate::exe_version::evaluate(name, args, self.resolve(path).as_deref())
        }
        #[cfg(not(feature = "executable-versions"))]
        {
            Err(ConditionError::Unsupported(name.to_string()))
        }
    }
}

impl FsEnv {
    /// `version(plugin, version, operator)` (or with the operator second): the plugin's version is the one its
    /// description gives ([`crate::version::extract_version`]), compared as LOOT compares versions. As in libloot,
    /// a plugin that is not in the load order or not installed, one whose description gives no version, is false;
    /// a plugin that cannot be read is an error.
    fn plugin_version(&self, path: &str, args: &[Arg]) -> Result<bool, ConditionError> {
        let syntax = |text: &str| ConditionError::Syntax(format!("version() {text}"));
        if args.len() != 3 {
            return Err(syntax("takes 3 arguments"));
        }
        let ((Arg::Str(given), Arg::Word(operator)) | (Arg::Word(operator), Arg::Str(given))) =
            (&args[1], &args[2])
        else {
            return Err(syntax("needs a quoted version and a comparison operator"));
        };
        if crate::version::compare("0", operator, "0").is_none() {
            return Err(syntax("has an unknown comparison operator"));
        }
        let file = path.rsplit('/').next().unwrap_or(path);
        let name = unghosted(file);
        let Some(resolved) = self
            .resolve(path)
            .filter(|resolved| resolved.is_file() && self.position(name).is_some())
        else {
            return Ok(false);
        };
        let header = plugin::read_header(&resolved).map_err(|error| {
            ConditionError::Environment(format!("{}: {error}", resolved.display()))
        })?;
        Ok(header
            .description
            .as_deref()
            .and_then(crate::version::extract_version)
            .and_then(|actual| crate::version::compare(&actual, operator, given))
            .unwrap_or(false))
    }
}

#[cfg(test)]
#[path = "env_tests.rs"]
mod tests;
