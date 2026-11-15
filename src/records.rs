// SPDX-License-Identifier: GPL-3.0-or-later
//
// The meaning of the record ids (which records a plugin overrides, when two plugins have a record in common) is
// libloot's, taken from esplugin (https://github.com/Ortham/esplugin, `src/plugin.rs`, `src/record_id.rs`,
// GPL-3.0, Copyright (C) 2017 Oliver Hamlet) and libloot's `Plugin::do_records_overlap` and
// `override_record_count`. The reader below is written for this crate: it scans the record headers of a
// Fallout: New Vegas plugin without recursion and with every size checked against the file. See NOTICE.md.

//! The records of a plugin, as far as sorting needs them: for each record its `FormID` resolved against the masters
//! of the plugin (so that the same record has the same id in every plugin that changes it), and how many of them
//! belong to a master.
//!
//! A `FormID` is `mm oooooo`: the top byte indexes the plugin's own master list (an index past the list means the
//! plugin itself) and the low 24 bits are the object. Two plugins have a record in common when some record has the
//! same master name and object in both; a plugin overrides a record when its master byte names a master.

use std::collections::HashMap;
use std::fmt;
use std::fs::File;
use std::io::{self, BufReader, Read, Seek};
use std::path::Path;

use crate::parallel::SharedBudget;
use crate::plugin::{RECORD_HEADER_SIZE, header_data_size};

/// Most records read from one plugin (`FalloutNV.esm` has about 400,000).
pub const MAX_RECORDS_PER_PLUGIN: usize = 4_000_000;
/// Most records kept for all plugins together: 8 bytes each, so 128 MiB at the limit.
pub const MAX_RECORDS_TOTAL: usize = 16_000_000;

/// Why a plugin's records could not be read.
#[derive(Debug)]
#[non_exhaustive]
pub enum RecordsError {
    /// The file could not be read.
    Io(io::Error),
    /// The first record is not a plugin header.
    Header(String),
    /// A record or the header announces more data than the file holds, or a header is cut off.
    Truncated,
    /// More records than [`MAX_RECORDS_PER_PLUGIN`], or than the total that is left.
    TooMany,
}

impl fmt::Display for RecordsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "{error}"),
            Self::Header(text) => write!(f, "not a plugin: {text}"),
            Self::Truncated => write!(
                f,
                "a record is cut off or announces more data than the file holds"
            ),
            Self::TooMany => write!(f, "more records than the limit"),
        }
    }
}

impl std::error::Error for RecordsError {}

/// Names (compared without regard to case) as small numbers, so that a record id is one `u64`.
#[derive(Debug, Default)]
pub struct NameIds {
    ids: HashMap<String, u32>,
}

impl NameIds {
    /// The number of a name, new names get the next one.
    ///
    /// # Panics
    /// Never in practice: it takes four billion different names.
    pub fn id(&mut self, name: &str) -> u32 {
        let next = u32::try_from(self.ids.len()).expect("fewer than 2^32 names");
        *self.ids.entry(name.to_lowercase()).or_insert(next)
    }
}

/// The record ids of one plugin.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PluginRecords {
    /// One entry per record, sorted: the object index above and the number of the plugin that owns the record
    /// (a master's, for an overridden record) below.
    pub keys: Vec<u64>,
    /// Records whose master byte names one of the plugin's masters.
    pub overridden: usize,
}

/// Read the record ids of the plugin at `path` whose header lists `masters`.
///
/// `budget` is the number of records that may still be kept for all plugins together; it is reduced by what this
/// plugin holds.
///
/// # Errors
/// [`RecordsError`] for an unreadable or malformed file, or when a limit is passed.
pub fn read_records(
    path: &Path,
    own_name: &str,
    masters: &[String],
    names: &mut NameIds,
    budget: &mut usize,
) -> Result<PluginRecords, RecordsError> {
    let file = File::open(path).map_err(RecordsError::Io)?;
    let length = file.metadata().map_err(RecordsError::Io)?.len();
    read_records_from(file, length, own_name, masters, names, budget)
}

/// [`read_records`] on any reader that is `length` bytes long.
///
/// # Errors
/// [`RecordsError`] for a reader that fails or holds a malformed plugin, or when a limit is passed.
pub fn read_records_from<R: Read + Seek>(
    reader: R,
    length: u64,
    own_name: &str,
    masters: &[String],
    names: &mut NameIds,
    budget: &mut usize,
) -> Result<PluginRecords, RecordsError> {
    let ids = OwnerIds::new(own_name, masters, names);
    let shared = SharedBudget::new(*budget, 1);
    let records = read_records_shared(reader, length, &ids, &shared)?;
    *budget -= records.keys.len();
    Ok(records)
}

/// The numbers of a plugin and of its masters, taken from [`NameIds`] before the records are read, so that plugins
/// can be read on several threads.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OwnerIds {
    own: u32,
    /// Indices past the list (and past 255) mean the plugin itself.
    masters: Vec<u32>,
}

impl OwnerIds {
    /// The numbers of `own_name` and of the first 256 `masters`.
    pub fn new(own_name: &str, masters: &[String], names: &mut NameIds) -> Self {
        let own = names.id(own_name);
        let masters = masters
            .iter()
            .take(256)
            .map(|name| names.id(name))
            .collect();
        Self { own, masters }
    }
}

/// Read the record ids of the plugin at `path`, taking what it keeps from `budget` (shared by every plugin of the
/// load order). On an error nothing stays taken.
///
/// # Errors
/// [`RecordsError`] for an unreadable or malformed file, or when a limit is passed.
pub fn read_records_at(
    path: &Path,
    ids: &OwnerIds,
    budget: &SharedBudget,
) -> Result<PluginRecords, RecordsError> {
    let file = File::open(path).map_err(RecordsError::Io)?;
    let length = file.metadata().map_err(RecordsError::Io)?.len();
    read_records_shared(file, length, ids, budget)
}

fn read_records_shared<R: Read + Seek>(
    reader: R,
    length: u64,
    ids: &OwnerIds,
    budget: &SharedBudget,
) -> Result<PluginRecords, RecordsError> {
    let mut reader = BufReader::with_capacity(1 << 16, reader);
    let mut header = [0u8; RECORD_HEADER_SIZE];
    match read_header_bytes(&mut reader, &mut header)? {
        Some(()) => {}
        None => return Err(RecordsError::Header("the file is empty".to_string())),
    }
    let size =
        header_data_size(&header).map_err(|error| RecordsError::Header(error.to_string()))?;
    let mut position = RECORD_HEADER_SIZE as u64 + size as u64;
    if position > length {
        return Err(RecordsError::Truncated);
    }
    skip(&mut reader, size as u64)?;

    let mut holder = budget.holder();
    let mut keys: Vec<u64> = Vec::new();
    let mut overridden = 0;
    while read_header_bytes(&mut reader, &mut header)?.is_some() {
        position += RECORD_HEADER_SIZE as u64;
        if &header[..4] == b"GRUP" {
            // The records of a group follow its header: nothing to skip, nothing to track.
            continue;
        }
        let data = u64::from(u32::from_le_bytes([
            header[4], header[5], header[6], header[7],
        ]));
        let form_id = u32::from_le_bytes([header[12], header[13], header[14], header[15]]);
        position += data;
        if position > length {
            return Err(RecordsError::Truncated);
        }
        skip(&mut reader, data)?;
        if form_id == 0 {
            continue;
        }
        if keys.len() >= MAX_RECORDS_PER_PLUGIN || !holder.take() {
            return Err(RecordsError::TooMany);
        }
        let object = u64::from(form_id & 0x00FF_FFFF);
        let owner = match ids.masters.get((form_id >> 24) as usize) {
            Some(&master) => {
                overridden += 1;
                master
            }
            None => ids.own,
        };
        keys.push(object << 32 | u64::from(owner));
    }
    keys.sort_unstable();
    holder.keep(keys.len());
    Ok(PluginRecords { keys, overridden })
}

/// Fill `header`; `None` at the end of the file, an error for a header that is cut off.
fn read_header_bytes<R: Read>(
    reader: &mut R,
    header: &mut [u8; RECORD_HEADER_SIZE],
) -> Result<Option<()>, RecordsError> {
    let mut filled = 0;
    while filled < header.len() {
        let read = reader
            .read(&mut header[filled..])
            .map_err(RecordsError::Io)?;
        if read == 0 {
            return if filled == 0 {
                Ok(None)
            } else {
                Err(RecordsError::Truncated)
            };
        }
        filled += read;
    }
    Ok(Some(()))
}

fn skip<R: Read + Seek>(reader: &mut BufReader<R>, bytes: u64) -> Result<(), RecordsError> {
    let bytes = i64::try_from(bytes).map_err(|_| RecordsError::Truncated)?;
    reader.seek_relative(bytes).map_err(RecordsError::Io)
}

/// A group of records for [`synthetic_records`]: its label and `(form id, data size)` of each record.
pub type SyntheticGroup<'a> = ([u8; 4], &'a [(u32, u32)]);

/// Bytes of a plugin with a header and the given records, for tests: one `GRUP` per record type, each record with
/// an empty subrecord area of the given size. `groups` are `(label, records)`, a record is `(form id, data size)`.
///
/// # Panics
/// If a size does not fit in 32 bits (tests only).
#[must_use]
pub fn synthetic_records(flags: u32, masters: &[&str], groups: &[SyntheticGroup]) -> Vec<u8> {
    let mut out = crate::plugin::synthetic_plugin(flags, masters, None);
    for (label, records) in groups {
        let mut body = Vec::new();
        for &(form_id, data) in *records {
            body.extend_from_slice(b"NPC_");
            body.extend_from_slice(&data.to_le_bytes());
            body.extend_from_slice(&0u32.to_le_bytes());
            body.extend_from_slice(&form_id.to_le_bytes());
            body.extend_from_slice(&[0; 8]);
            body.resize(body.len() + data as usize, 0);
        }
        out.extend_from_slice(b"GRUP");
        let size = u32::try_from(RECORD_HEADER_SIZE + body.len()).expect("fits");
        out.extend_from_slice(&size.to_le_bytes());
        out.extend_from_slice(label);
        out.extend_from_slice(&[0; 12]);
        out.extend_from_slice(&body);
    }
    out
}

#[cfg(test)]
#[path = "records_tests.rs"]
mod tests;
