//! Reading the records and archive tables of every installed plugin for a sort: on several threads, optionally
//! through a [`SortCache`] that keeps what was read for the next sort.
//!
//! Whether a limit is passed never depends on the threads or on the cache. The verdict is computed from the complete
//! counts, with the rule a reader one plugin after the other applies:
//!
//! * records: a plugin with more than [`MAX_RECORDS_PER_PLUGIN`] records, or more than [`MAX_RECORDS_TOTAL`] for all
//!   plugins together;
//! * archives: for some plugin, the distinct files of the plugins before it plus the files (with repeats) of its own
//!   archives pass [`MAX_ASSETS_TOTAL`].
//!
//! While reading, a shared budget taken in chunks bounds the memory; when it refuses, everything is read again one
//! plugin after the other with an exact budget and the cache is left alone.

use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use crate::archives::{self, ArchiveError, MAX_ASSETS_TOTAL, PathIds, PluginAssets};
use crate::parallel::{self, SharedBudget};
use crate::records::{
    self, MAX_RECORDS_PER_PLUGIN, MAX_RECORDS_TOTAL, NameIds, OwnerIds, PluginRecords, RecordsError,
};

/// What was read from plugins and archives by earlier sorts, for the next one. Optional: give it to
/// [`crate::api::SortOptions::with_cache`] when the same files are sorted more than once (several profiles, a
/// check after a sort).
///
/// A file is read again when its size, modification time or inode changed; nothing else is compared, so a tool
/// that rewrites a file in place within the same second and to the same size must use a new cache. Cloning is cheap
/// and the clones share the data; it can be used by several sorts at once from several threads. It holds the record
/// ids and archive file names of every file it has seen (8 bytes per record or file, plus each name once) until it is
/// dropped or [`SortCache::clear`]ed.
#[derive(Clone, Default)]
pub struct SortCache {
    inner: Arc<Mutex<CacheData>>,
}

impl fmt::Debug for SortCache {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let data = self.lock();
        f.debug_struct("SortCache")
            .field("plugins", &data.records.len())
            .field("archives", &data.archives.len())
            .finish()
    }
}

/// Two handles are equal when they share the same data.
impl PartialEq for SortCache {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }
}

impl Eq for SortCache {}

impl SortCache {
    /// An empty cache.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Forget everything read so far.
    pub fn clear(&self) {
        *self.lock() = CacheData::default();
    }

    /// Plugins and archives held.
    #[must_use]
    pub fn len(&self) -> (usize, usize) {
        let data = self.lock();
        (data.records.len(), data.archives.len())
    }

    /// True when nothing is held.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == (0, 0)
    }

    fn lock(&self) -> MutexGuard<'_, CacheData> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

#[derive(Default)]
struct CacheData {
    /// Plugin names as numbers; kept, so that cached record ids stay valid from one sort to the next.
    names: NameIds,
    /// Archive folder and file names as numbers, kept for the same reason.
    paths: Arc<PathIds>,
    records: HashMap<PathBuf, RecordEntry>,
    archives: HashMap<PathBuf, ArchiveEntry>,
}

struct RecordEntry {
    stamp: Stamp,
    owner: OwnerIds,
    records: PluginRecords,
}

struct ArchiveEntry {
    stamp: Stamp,
    /// File records read (with repeats).
    raw: usize,
    /// Sorted, without repeats.
    keys: Vec<u64>,
}

/// What says that a file is unchanged: its size, modification time and (on Unix) device and inode.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Stamp {
    length: u64,
    modified: Option<std::time::SystemTime>,
    node: (u64, u64),
}

fn stamp(path: &Path) -> Option<Stamp> {
    let meta = std::fs::metadata(path).ok()?;
    #[cfg(unix)]
    let node = {
        use std::os::unix::fs::MetadataExt;
        (meta.dev(), meta.ino())
    };
    #[cfg(not(unix))]
    let node = (0, 0);
    Some(Stamp {
        length: meta.len(),
        modified: meta.modified().ok(),
        node,
    })
}

/// One plugin whose records are wanted.
pub struct RecordJob<'a> {
    /// The plugin's name (for its own records' owner and for warnings).
    pub name: &'a str,
    /// Where it is; `None` when it is not read (not found, or its header is not readable).
    pub path: Option<&'a Path>,
    /// The masters of its header.
    pub masters: &'a [String],
}

/// A limit was passed (the sort reports `TooMuchWork`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OverLimit;

/// The record ids of every plugin of `jobs`; a plugin that cannot be read is noted in `warnings` and counts as one
/// without records.
///
/// # Errors
/// [`OverLimit`] by the rule in the module documentation.
pub fn read_records(
    jobs: &[RecordJob],
    threads: usize,
    cache: Option<&SortCache>,
    warnings: &mut Vec<String>,
) -> Result<Vec<PluginRecords>, OverLimit> {
    let mut local = CacheData::default();
    let mut guard = cache.map(SortCache::lock);
    let data = guard.as_deref_mut().unwrap_or(&mut local);
    // Owner ids and cache hits, under the lock; the reading happens without it.
    let mut owners = Vec::with_capacity(jobs.len());
    let mut found: Vec<Option<PluginRecords>> = Vec::with_capacity(jobs.len());
    let mut stamps = Vec::with_capacity(jobs.len());
    for job in jobs {
        let owner = OwnerIds::new(job.name, job.masters, &mut data.names);
        let stamp = job.path.and_then(stamp);
        let hit = job
            .path
            .zip(stamp.as_ref())
            .and_then(|(path, stamp)| data.records.get(path).filter(|entry| &entry.stamp == stamp))
            .filter(|entry| entry.owner == owner)
            .map(|entry| entry.records.clone());
        found.push(hit);
        owners.push(owner);
        stamps.push(stamp);
    }
    drop(guard);

    let misses: Vec<usize> = (0..jobs.len())
        .filter(|&index| found[index].is_none() && jobs[index].path.is_some())
        .collect();
    let read = |threads: usize, chunk: usize, indices: &[usize]| {
        let budget = SharedBudget::new(MAX_RECORDS_TOTAL, chunk);
        parallel::map(indices, threads, |&index| {
            let path = jobs[index].path.unwrap_or_else(|| Path::new(""));
            records::read_records_at(path, &owners[index], &budget)
        })
    };
    let refused = |results: &[Result<PluginRecords, RecordsError>]| {
        results
            .iter()
            .any(|result| matches!(result, Err(RecordsError::TooMany)))
    };
    let results = read(threads, 1 << 16, &misses);
    if refused(&results) {
        // Exact, one plugin after the other, and nothing taken from the cache or put into it.
        let all: Vec<usize> = (0..jobs.len())
            .filter(|&index| jobs[index].path.is_some())
            .collect();
        let mut merged: Vec<Option<Result<PluginRecords, RecordsError>>> =
            (0..jobs.len()).map(|_| None).collect();
        for (index, result) in all.iter().copied().zip(read(1, 1, &all)) {
            merged[index] = Some(result);
        }
        return finish_records(jobs, merged, warnings);
    }

    let mut merged: Vec<Option<Result<PluginRecords, RecordsError>>> =
        found.into_iter().map(|hit| hit.map(Ok)).collect();
    let mut fresh = Vec::new();
    for (index, result) in misses.into_iter().zip(results) {
        if let (Ok(records), Some(stamp), Some(path)) = (&result, &stamps[index], jobs[index].path)
        {
            fresh.push((path.to_path_buf(), stamp.clone(), index, records.clone()));
        }
        merged[index] = Some(result);
    }
    let total: usize = merged
        .iter()
        .flatten()
        .filter_map(|result| result.as_ref().ok())
        .map(|records| records.keys.len())
        .sum();
    let too_large = merged
        .iter()
        .flatten()
        .filter_map(|result| result.as_ref().ok())
        .any(|records| records.keys.len() > MAX_RECORDS_PER_PLUGIN);
    if total > MAX_RECORDS_TOTAL || too_large {
        return Err(OverLimit);
    }
    if let Some(cache) = cache {
        let mut data = cache.lock();
        for (path, stamp, index, records) in fresh {
            data.records.insert(
                path,
                RecordEntry {
                    stamp,
                    owner: owners[index].clone(),
                    records,
                },
            );
        }
    }
    finish_records(jobs, merged, warnings)
}

fn finish_records(
    jobs: &[RecordJob],
    merged: Vec<Option<Result<PluginRecords, RecordsError>>>,
    warnings: &mut Vec<String>,
) -> Result<Vec<PluginRecords>, OverLimit> {
    let mut all = Vec::with_capacity(jobs.len());
    for (job, result) in jobs.iter().zip(merged) {
        match result {
            None => all.push(PluginRecords::default()),
            Some(Ok(records)) => all.push(records),
            Some(Err(RecordsError::TooMany)) => return Err(OverLimit),
            Some(Err(error)) => {
                warnings.push(format!(
                    "{}: records not readable ({error}); it is treated as having none",
                    job.name
                ));
                all.push(PluginRecords::default());
            }
        }
    }
    Ok(all)
}

/// The archives of one plugin, read or from the cache: per archive, the file records read and the sorted keys.
type ArchiveRead = Result<(usize, Vec<u64>), ArchiveError>;

/// The files of the archives of each plugin (`archives[i]` are plugin `i`'s, `names[i]` its name for warnings). An
/// archive that cannot be read is noted and skipped, as libloot does.
///
/// # Errors
/// [`OverLimit`] by the rule in the module documentation.
pub fn read_assets(
    names: &[&str],
    archives: &[Vec<PathBuf>],
    threads: usize,
    cache: Option<&SortCache>,
    warnings: &mut Vec<String>,
) -> Result<Vec<PluginAssets>, OverLimit> {
    let mut local = CacheData::default();
    let mut guard = cache.map(SortCache::lock);
    let data = guard.as_deref_mut().unwrap_or(&mut local);
    let ids = Arc::clone(&data.paths);
    // Every archive once (two plugins can load the same one), with its stamp and a cache hit.
    let mut distinct: Vec<PathBuf> = archives.iter().flatten().cloned().collect();
    distinct.sort();
    distinct.dedup();
    let stamps: Vec<Option<Stamp>> = distinct.iter().map(|path| stamp(path)).collect();
    let hits: Vec<Option<(usize, Vec<u64>)>> = distinct
        .iter()
        .zip(&stamps)
        .map(|(path, stamp)| {
            let stamp = stamp.as_ref()?;
            data.archives
                .get(path)
                .filter(|entry| &entry.stamp == stamp)
                .map(|entry| (entry.raw, entry.keys.clone()))
        })
        .collect();
    drop(guard);

    let misses: Vec<usize> = (0..distinct.len())
        .filter(|&at| hits[at].is_none())
        .collect();
    let budget = SharedBudget::new(MAX_ASSETS_TOTAL, 1 << 12);
    let read: Vec<ArchiveRead> = parallel::map(&misses, threads, |&at| {
        let mut holder = budget.holder();
        let mut keys = Vec::new();
        archives::read_archive_with(&distinct[at], &ids, &mut keys, &mut holder)?;
        let raw = keys.len();
        keys.sort_unstable();
        keys.dedup();
        holder.keep(keys.len());
        Ok((raw, keys))
    });
    if read
        .iter()
        .any(|result| matches!(result, Err(ArchiveError::TooMuch)))
    {
        return read_assets_exactly(names, archives, warnings);
    }
    let mut results: Vec<Option<ArchiveRead>> = hits.into_iter().map(|hit| hit.map(Ok)).collect();
    let mut fresh = Vec::new();
    for (at, result) in misses.into_iter().zip(read) {
        if let (Ok((raw, keys)), Some(stamp)) = (&result, &stamps[at]) {
            fresh.push((distinct[at].clone(), stamp.clone(), *raw, keys.clone()));
        }
        results[at] = Some(result);
    }

    let position = |path: &PathBuf| distinct.binary_search(path).ok();
    let mut all = Vec::with_capacity(names.len());
    let mut before = 0usize;
    for (name, found) in names.iter().zip(archives) {
        let mut raw = 0usize;
        let mut keys: Vec<u64> = Vec::new();
        for path in found {
            match position(path).and_then(|at| results[at].as_ref()) {
                Some(Ok((count, archive_keys))) => {
                    raw += count;
                    keys.extend_from_slice(archive_keys);
                }
                Some(Err(error)) => {
                    let file = path
                        .file_name()
                        .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
                    warnings.push(format!("{name}: archive {file} not read ({error})"));
                }
                None => {}
            }
        }
        if before.saturating_add(raw) > MAX_ASSETS_TOTAL {
            return Err(OverLimit);
        }
        keys.sort_unstable();
        keys.dedup();
        before += keys.len();
        all.push(PluginAssets { keys });
    }
    if let Some(cache) = cache {
        let mut data = cache.lock();
        for (path, stamp, raw, keys) in fresh {
            data.archives
                .insert(path, ArchiveEntry { stamp, raw, keys });
        }
    }
    Ok(all)
}

/// One plugin after the other with an exact budget, the cache left alone.
fn read_assets_exactly(
    names: &[&str],
    archives: &[Vec<PathBuf>],
    warnings: &mut Vec<String>,
) -> Result<Vec<PluginAssets>, OverLimit> {
    let ids = PathIds::default();
    let budget = SharedBudget::new(MAX_ASSETS_TOTAL, 1);
    let mut all = Vec::with_capacity(names.len());
    for (name, found) in names.iter().zip(archives) {
        let mut skipped = Vec::new();
        match archives::read_plugin_assets_shared(found, &ids, &budget, &mut skipped) {
            Ok(assets) => all.push(assets),
            Err(_) => return Err(OverLimit),
        }
        for (path, error) in skipped {
            let file = path
                .file_name()
                .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
            warnings.push(format!("{name}: archive {file} not read ({error})"));
        }
    }
    Ok(all)
}
