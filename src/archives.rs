// SPDX-License-Identifier: GPL-3.0-or-later
//
// Which archives a plugin loads and which files they hold follows libloot (https://github.com/loot/libloot,
// `src/archive/find.rs`, `bsa.rs` and `parse.rs`, GPL-3.0-or-later, Copyright (C) the libloot authors), for
// Fallout: New Vegas: the BSA files of the `Data` directory whose names begin with the plugin's name, read for
// the folder and file names they list. The reader is rewritten for this crate with every size checked against
// the file before anything is allocated by it, and the Fallout 4 and Starfield archives (BA2) and the other
// games' rules are left out. See NOTICE.md.

//! The files that the archives of a plugin hold, as far as sorting needs them: two plugins whose archives hold a
//! file with the same path are about the same assets, and the one that loads more of them goes first.
//!
//! A BSA (`BSA\0`, version 103, 104 or 105; New Vegas uses 104) starts with a header, then one record per folder
//! (name hash, file count, offset), then per folder its name and one 16-byte record per file, then the names of the
//! files, separated by zero bytes, in the order of the file records. Only these tables are read, never the data.

use std::collections::HashMap;
use std::fmt;
use std::fs::File;
use std::hash::{BuildHasherDefault, Hasher};
use std::io::{self, BufReader, Read};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::parallel::{Holder, SharedBudget};

/// Most bytes of tables read from one archive (a BSA with 300,000 files has about 10 MiB of tables).
pub const MAX_ARCHIVE_TABLES: u64 = 64 * 1024 * 1024;
/// Most distinct files kept for all plugins together: eight bytes each, and their names once.
pub const MAX_ASSETS_TOTAL: usize = 4_000_000;

const HEADER_SIZE: u64 = 36;
const FILE_RECORD_SIZE: u64 = 16;
const FLAG_HAS_DIRECTORY_NAMES: u32 = 0x1;
const FLAG_HAS_FILE_NAMES: u32 = 0x2;
const FLAG_BIG_ENDIAN: u32 = 0x40;

/// Why an archive could not be read.
#[derive(Debug)]
#[non_exhaustive]
pub enum ArchiveError {
    /// The file could not be read.
    Io(io::Error),
    /// Not a BSA, or a version or kind this reader does not handle.
    Unsupported(String),
    /// The tables announce more than the file holds, or point outside themselves.
    Malformed(&'static str),
    /// Tables over [`MAX_ARCHIVE_TABLES`], or more files than [`MAX_ASSETS_TOTAL`] leaves room for.
    TooMuch,
}

impl fmt::Display for ArchiveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "{error}"),
            Self::Unsupported(text) => write!(f, "{text}"),
            Self::Malformed(text) => write!(f, "malformed archive: {text}"),
            Self::TooMuch => write!(f, "more than the limit"),
        }
    }
}

impl std::error::Error for ArchiveError {}

/// Folder names and file names as small numbers, so that a file is one `u64`. Shared by threads: the tables are
/// split into shards, each behind its own lock. Which number a name gets depends on which thread reads it first,
/// but two files have the same key exactly when their names are the same, which is all the sorter asks.
#[derive(Debug, Default)]
pub struct PathIds {
    folders: Interner,
    files: Interner,
}

const SHARDS: usize = 64;

/// Names of one shard and their numbers.
type Shard = HashMap<Box<[u8]>, u32, BuildHasherDefault<NameHasher>>;

#[derive(Debug)]
struct Interner {
    shards: Vec<Mutex<Shard>>,
}

impl Default for Interner {
    fn default() -> Self {
        Self {
            shards: (0..SHARDS).map(|_| Mutex::default()).collect(),
        }
    }
}

impl Interner {
    fn id(&self, name: &[u8]) -> u32 {
        let mut hasher = NameHasher::default();
        hasher.write(name);
        // `SHARDS` is a power of two; the low bits of the hash pick the shard.
        #[allow(clippy::cast_possible_truncation)]
        let shard = (hasher.finish() as usize) & (SHARDS - 1);
        let mut map = self.shards[shard]
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(&id) = map.get(name) {
            return id;
        }
        // The shard in the low bits keeps ids of different shards apart; a shard holds far fewer than 2^26 names
        // under `MAX_ASSETS_TOTAL`.
        let local = u32::try_from(map.len()).unwrap_or(u32::MAX >> 6);
        #[allow(clippy::cast_possible_truncation)]
        let id = local << 6 | shard as u32;
        map.insert(name.into(), id);
        id
    }
}

/// A fast hash for path bytes (`FxHash`'s multiply-rotate on eight bytes at a time). The input is the user's own
/// archive tables, bounded in size, so no protection against crafted collisions is needed beyond the size limits.
#[derive(Debug, Default)]
struct NameHasher(u64);

impl Hasher for NameHasher {
    fn write(&mut self, bytes: &[u8]) {
        const SEED: u64 = 0x51_7c_c1_b7_27_22_0a_95;
        let (chunks, rest) = bytes.as_chunks::<8>();
        for chunk in chunks {
            let word = u64::from_le_bytes(*chunk);
            self.0 = (self.0.rotate_left(5) ^ word).wrapping_mul(SEED);
        }
        for &byte in rest {
            self.0 = (self.0.rotate_left(5) ^ u64::from(byte)).wrapping_mul(SEED);
        }
        self.0 = (self.0.rotate_left(5) ^ bytes.len() as u64).wrapping_mul(SEED);
    }

    fn finish(&self) -> u64 {
        // Mix the high bits down: the shard is taken from the low ones.
        self.0 ^ (self.0 >> 29)
    }
}

impl PathIds {
    fn key(&self, folder: &[u8], file: &[u8]) -> u64 {
        u64::from(self.folders.id(folder)) << 32 | u64::from(self.files.id(file))
    }
}

/// The archives `plugin_name` loads: BSA files directly in `data_dir` whose names begin with the plugin's name
/// without its extension, compared without regard to case. (A plugin `A.esp` therefore also loads `AB.bsa`, as in
/// libloot.) Sorted by name.
#[must_use]
pub fn associated_archives(data_dir: &Path, plugin_name: &str) -> Vec<PathBuf> {
    ArchiveIndex::new(&[data_dir]).archives_of(plugin_name)
}

/// The BSA files of some directories, listed once, so that finding the archives of each plugin of a load order does
/// not list a directory per plugin.
#[derive(Debug, Clone, Default)]
pub struct ArchiveIndex {
    /// Per directory, in the order given: the lower-case file name and the path of each BSA, sorted by path.
    dirs: Vec<Vec<(String, PathBuf)>>,
}

impl ArchiveIndex {
    /// List the BSA files directly in each of `dirs` (a directory that cannot be listed has none).
    #[must_use]
    pub fn new(dirs: &[&Path]) -> Self {
        let dirs = dirs
            .iter()
            .map(|dir| {
                let Ok(entries) = std::fs::read_dir(dir) else {
                    return Vec::new();
                };
                let mut found: Vec<(String, PathBuf)> = entries
                    .filter_map(Result::ok)
                    .filter(|entry| {
                        entry
                            .file_type()
                            .is_ok_and(|kind| kind.is_file() || kind.is_symlink())
                    })
                    .filter_map(|entry| {
                        let name = entry.file_name().to_string_lossy().to_lowercase();
                        Path::new(&name)
                            .extension()
                            .is_some_and(|extension| extension.eq_ignore_ascii_case("bsa"))
                            .then(|| (name, entry.path()))
                    })
                    .collect();
                found.sort_by(|a, b| a.1.cmp(&b.1));
                found
            })
            .collect();
        Self { dirs }
    }

    /// The archives `plugin_name` loads, as [`associated_archives`] finds them, directory by directory in the order
    /// given.
    #[must_use]
    pub fn archives_of(&self, plugin_name: &str) -> Vec<PathBuf> {
        let stem = Path::new(plugin_name)
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or_default()
            .to_lowercase();
        if stem.is_empty() {
            return Vec::new();
        }
        self.dirs
            .iter()
            .flatten()
            .filter(|(name, _)| name.starts_with(&stem))
            .map(|(_, path)| path.clone())
            .collect()
    }
}

/// The files of the archive at `path`, as keys of `ids`, appended to `keys` (not sorted, may repeat).
///
/// # Errors
/// [`ArchiveError`] for an unreadable or unsupported archive, or one over a limit. Nothing is appended then.
pub fn read_archive(
    path: &Path,
    ids: &PathIds,
    keys: &mut Vec<u64>,
    budget: &mut usize,
) -> Result<(), ArchiveError> {
    let shared = SharedBudget::new(budget.saturating_sub(keys.len()), 1);
    let mut holder = shared.holder();
    read_archive_at(path, ids, keys, &mut holder)
}

/// Read the archive at `path` into `keys` (not sorted, may repeat), taking each file from `holder`.
///
/// # Errors
/// As [`read_archive`]; on an error nothing is appended.
pub fn read_archive_with(
    path: &Path,
    ids: &PathIds,
    keys: &mut Vec<u64>,
    holder: &mut Holder,
) -> Result<(), ArchiveError> {
    read_archive_at(path, ids, keys, holder)
}

fn read_archive_at(
    path: &Path,
    ids: &PathIds,
    keys: &mut Vec<u64>,
    holder: &mut Holder,
) -> Result<(), ArchiveError> {
    let file = File::open(path).map_err(ArchiveError::Io)?;
    let length = file.metadata().map_err(ArchiveError::Io)?.len();
    read_archive_holder(BufReader::new(file), length, ids, keys, holder)
}

/// The header of a BSA, checked.
struct Header {
    folder_record_size: u64,
    folder_count: u64,
    file_count: u64,
    folder_names_length: u64,
    file_names_length: u64,
}

fn word(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

fn read_header<R: Read>(reader: &mut R) -> Result<Header, ArchiveError> {
    let mut header = [0u8; 36];
    reader
        .read_exact(&mut header)
        .map_err(|error| match error.kind() {
            io::ErrorKind::UnexpectedEof => ArchiveError::Malformed("shorter than a header"),
            _ => ArchiveError::Io(error),
        })?;
    if &header[..4] != b"BSA\0" {
        return Err(ArchiveError::Unsupported(
            "not a BSA (Fallout 4 archives are not read)".to_string(),
        ));
    }
    let (version, records_offset, flags) = (word(&header, 4), word(&header, 8), word(&header, 12));
    if u64::from(records_offset) != HEADER_SIZE {
        return Err(ArchiveError::Malformed(
            "the folder records do not follow the header",
        ));
    }
    let folder_record_size = match version {
        103 | 104 => 16,
        105 => 24,
        other => return Err(ArchiveError::Unsupported(format!("BSA version {other}"))),
    };
    if flags & FLAG_BIG_ENDIAN != 0 {
        return Err(ArchiveError::Unsupported("big-endian archive".to_string()));
    }
    if flags & FLAG_HAS_DIRECTORY_NAMES == 0 || flags & FLAG_HAS_FILE_NAMES == 0 {
        return Err(ArchiveError::Unsupported(
            "an archive without folder or file names".to_string(),
        ));
    }
    Ok(Header {
        folder_record_size,
        folder_count: u64::from(word(&header, 16)),
        file_count: u64::from(word(&header, 20)),
        folder_names_length: u64::from(word(&header, 24)),
        file_names_length: u64::from(word(&header, 28)),
    })
}

fn read_table<R: Read>(reader: &mut R, size: u64) -> Result<Vec<u8>, ArchiveError> {
    let mut table = vec![0u8; usize::try_from(size).map_err(|_| ArchiveError::TooMuch)?];
    reader.read_exact(&mut table).map_err(ArchiveError::Io)?;
    Ok(table)
}

/// Each folder's normalised name and file count: a folder record points at its name (a length byte, the name and
/// a zero) among the file records.
fn folder_names(
    header: &Header,
    folders: &[u8],
    records: &[u8],
) -> Result<Vec<(Vec<u8>, u64)>, ArchiveError> {
    let size = usize::try_from(header.folder_record_size).unwrap_or(16);
    let offset_at = size - usize::from(size == 24) * 4 - 4;
    let baseline =
        HEADER_SIZE + header.folder_count * header.folder_record_size + header.file_names_length;
    let mut names = Vec::new();
    for record in folders.chunks_exact(size) {
        let count = u64::from(word(record, 8));
        let offset = u64::from(word(record, offset_at));
        let at = offset
            .checked_sub(baseline)
            .and_then(|at| usize::try_from(at).ok())
            .ok_or(ArchiveError::Malformed(
                "a folder name lies before the file records",
            ))?;
        let length = *records.get(at).ok_or(ArchiveError::Malformed(
            "a folder name lies past the file records",
        ))?;
        let name = records
            .get(at + 1..at + 1 + usize::from(length))
            .ok_or(ArchiveError::Malformed("a folder name is cut off"))?;
        // The length counts the terminating zero.
        names.push((normalise(name.strip_suffix(&[0]).unwrap_or(name)), count));
    }
    Ok(names)
}

/// [`read_archive`] on any reader that is `length` bytes long.
///
/// # Errors
/// As [`read_archive`].
pub fn read_archive_from<R: Read>(
    reader: R,
    length: u64,
    ids: &PathIds,
    keys: &mut Vec<u64>,
    budget: &mut usize,
) -> Result<(), ArchiveError> {
    let shared = SharedBudget::new(budget.saturating_sub(keys.len()), 1);
    let mut holder = shared.holder();
    read_archive_holder(reader, length, ids, keys, &mut holder)
}

fn read_archive_holder<R: Read>(
    mut reader: R,
    length: u64,
    ids: &PathIds,
    keys: &mut Vec<u64>,
    holder: &mut Holder,
) -> Result<(), ArchiveError> {
    let header = read_header(&mut reader)?;
    let folders_size = header.folder_count * header.folder_record_size;
    let records_size =
        header.folder_count + header.folder_names_length + header.file_count * FILE_RECORD_SIZE;
    let tables = folders_size + records_size + header.file_names_length;
    if tables > length.saturating_sub(HEADER_SIZE) {
        return Err(ArchiveError::Malformed(
            "the tables are longer than the file",
        ));
    }
    if tables > MAX_ARCHIVE_TABLES {
        return Err(ArchiveError::TooMuch);
    }
    let folders = read_table(&mut reader, folders_size)?;
    let records = read_table(&mut reader, records_size)?;
    let names = read_table(&mut reader, header.file_names_length)?;
    let folder_names = folder_names(&header, &folders, &records)?;

    // File names in the order of the file records, which is the order of the folders, each folder repeated by its
    // file count. A count that promises more names than there are is stopped by the names.
    let mut file_names = names.split(|&byte| byte == 0);
    let before = keys.len();
    let mut file = Vec::new();
    'folders: for (folder, count) in &folder_names {
        for _ in 0..*count {
            let Some(name) = file_names.next() else {
                break 'folders;
            };
            if !holder.take() {
                keys.truncate(before);
                return Err(ArchiveError::TooMuch);
            }
            normalise_into(name, &mut file);
            keys.push(ids.key(folder, &file));
        }
    }
    Ok(())
}

/// libloot's `trim_and_normalise`: no slashes at either end, `/` as `\`, ASCII letters in lower case, other bytes
/// as they are.
fn normalise(path: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    normalise_into(path, &mut out);
    out
}

/// [`normalise`] into `out`, which is cleared first (so one buffer serves every file of an archive).
fn normalise_into(path: &[u8], out: &mut Vec<u8>) {
    out.clear();
    let is_slash = |byte: &u8| *byte == b'\\' || *byte == b'/';
    let start = path
        .iter()
        .position(|byte| !is_slash(byte))
        .unwrap_or(path.len());
    let end = path
        .iter()
        .rposition(|byte| !is_slash(byte))
        .map_or(start, |at| at + 1);
    out.extend(path[start..end.max(start)].iter().map(|&byte| match byte {
        b'/' => b'\\',
        other => other.to_ascii_lowercase(),
    }));
}

/// The files of the archives a plugin loads: sorted, each once, folders and files compared by their names.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PluginAssets {
    /// One key per file, sorted, without repeats.
    pub keys: Vec<u64>,
}

/// Read every archive in `archives` for one plugin. An archive that cannot be read is skipped and described in
/// `skipped`, as libloot logs it and goes on.
///
/// # Errors
/// [`ArchiveError::TooMuch`] when the files pass what is left of `budget`.
pub fn read_plugin_assets(
    archives: &[PathBuf],
    ids: &PathIds,
    budget: &mut usize,
    skipped: &mut Vec<(PathBuf, ArchiveError)>,
) -> Result<PluginAssets, ArchiveError> {
    let shared = SharedBudget::new(*budget, 1);
    let assets = read_plugin_assets_shared(archives, ids, &shared, skipped)?;
    *budget = budget.saturating_sub(assets.keys.len());
    Ok(assets)
}

/// [`read_plugin_assets`] with a budget shared by every plugin of the load order (and the threads that read them):
/// the plugin keeps as many files of it as it has distinct files.
///
/// # Errors
/// [`ArchiveError::TooMuch`] when the files pass what is left of `budget`.
pub fn read_plugin_assets_shared(
    archives: &[PathBuf],
    ids: &PathIds,
    budget: &SharedBudget,
    skipped: &mut Vec<(PathBuf, ArchiveError)>,
) -> Result<PluginAssets, ArchiveError> {
    let mut holder = budget.holder();
    let mut keys = Vec::new();
    for path in archives {
        match read_archive_at(path, ids, &mut keys, &mut holder) {
            Ok(()) => {}
            Err(ArchiveError::TooMuch) => return Err(ArchiveError::TooMuch),
            Err(error) => skipped.push((path.clone(), error)),
        }
    }
    keys.sort_unstable();
    keys.dedup();
    holder.keep(keys.len());
    Ok(PluginAssets { keys })
}

/// The bytes of a BSA of the given version (103, 104 or 105) that holds `folders`, each a folder name with the names
/// of its files, for tests: no data, only the tables.
///
/// # Panics
/// If a name is longer than 254 bytes (tests only).
#[must_use]
pub fn synthetic_bsa(version: u32, folders: &[(&str, &[&str])]) -> Vec<u8> {
    let record_size: u32 = if version == 105 { 24 } else { 16 };
    let folder_count = u32::try_from(folders.len()).expect("fits");
    let file_count: usize = folders.iter().map(|(_, files)| files.len()).sum();
    let folder_names_length: usize = folders.iter().map(|(name, _)| name.len() + 1).sum();
    let file_names_length: usize = folders
        .iter()
        .flat_map(|(_, files)| files.iter())
        .map(|file| file.len() + 1)
        .sum();
    let baseline =
        36 + folder_count * record_size + u32::try_from(file_names_length).expect("fits");
    let mut head = Vec::new();
    head.extend_from_slice(b"BSA\0");
    for word in [
        version,
        36,
        3,
        folder_count,
        u32::try_from(file_count).expect("fits"),
        u32::try_from(folder_names_length).expect("fits"),
        u32::try_from(file_names_length).expect("fits"),
        0,
    ] {
        head.extend_from_slice(&word.to_le_bytes());
    }
    let mut records = Vec::new();
    let mut folder_records = Vec::new();
    for (name, files) in folders {
        let offset = baseline + u32::try_from(records.len()).expect("fits");
        folder_records.extend_from_slice(&[0; 8]);
        folder_records.extend_from_slice(&u32::try_from(files.len()).expect("fits").to_le_bytes());
        if version == 105 {
            folder_records.extend_from_slice(&[0; 4]);
        }
        folder_records.extend_from_slice(&offset.to_le_bytes());
        if version == 105 {
            folder_records.extend_from_slice(&[0; 4]);
        }
        records.push(u8::try_from(name.len() + 1).expect("short name"));
        records.extend_from_slice(name.as_bytes());
        records.push(0);
        records.resize(records.len() + files.len() * 16, 0);
    }
    let mut out = head;
    out.extend_from_slice(&folder_records);
    out.extend_from_slice(&records);
    for (_, files) in folders {
        for file in *files {
            out.extend_from_slice(file.as_bytes());
            out.push(0);
        }
    }
    out
}

#[cfg(test)]
#[path = "archives_tests.rs"]
mod tests;
