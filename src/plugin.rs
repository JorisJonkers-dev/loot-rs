// SPDX-License-Identifier: MIT
//
// Copied from `tools/fnvorder/src/plugin.rs` in JorisJonkers-dev/NV-Linux-Modding
// (fnvorder, licensed MIT OR Apache-2.0; the MIT option is taken here), written from the
// public plugin file-format description with no LOOT code. Copyright (c) the fnvorder authors.
// This file stays a separate module under the MIT terms (see NOTICE.md); the rest of this
// crate is GPL-3.0-only. Do not edit it here without noting the change in NOTICE.md.
//
//! Reader for the `TES4` header record at the start of a Fallout 3 / New Vegas plugin.
//!
//! Layout (little endian), written from the public file-format description and checked
//! only against synthetic files built in the tests:
//!
//! ```text
//! record header, 24 bytes: "TES4", data size u32, flags u32, form id u32,
//!                          version-control u32, form version u16, unknown u16
//! then subrecords inside the data size: type[4], size u16, payload
//!   HEDR 12 bytes  float version, u32 record count, u32 next object id
//!   CNAM zstring   author        SNAM zstring description
//!   MAST zstring   one per master, each followed by a DATA u64 (ignored)
//! ```
//!
//! A `XXXX` subrecord carries the real size (u32) of the subrecord that follows it; it is
//! handled so an oversized description cannot derail the master list.

use std::fmt;
use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

/// Bytes in a record header of this game generation.
pub const RECORD_HEADER_SIZE: usize = 24;

/// Header data larger than this is refused instead of read (real headers are a few KiB).
pub const MAX_HEADER_DATA: usize = 4 * 1024 * 1024;

/// Record flag: the plugin is a master (ESM).
pub const FLAG_ESM: u32 = 0x1;
/// Record flag: localized strings (Skyrim and later; not used by New Vegas).
pub const FLAG_LOCALIZED: u32 = 0x80;
/// Record flag: light master (ESL). New Vegas has no light plugins; it is read for completeness.
pub const FLAG_ESL: u32 = 0x200;

/// What can be wrong with a plugin header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginError {
    /// Fewer bytes than a record header.
    TooShort,
    /// The first record is not `TES4`.
    BadMagic([u8; 4]),
    /// The declared data size runs past the end of the input or past [`MAX_HEADER_DATA`].
    Truncated,
}

impl fmt::Display for PluginError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooShort => write!(f, "file is shorter than a record header"),
            Self::BadMagic(magic) => write!(
                f,
                "first record is {:?}, expected \"TES4\"",
                String::from_utf8_lossy(magic)
            ),
            Self::Truncated => write!(f, "header record data is truncated or implausibly large"),
        }
    }
}

impl std::error::Error for PluginError {}

/// The parts of a plugin header the load order needs.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct PluginHeader {
    /// Record flags.
    pub flags: u32,
    /// Form version from the record header (the version-control info is not kept).
    pub form_version: u16,
    /// `HEDR` version float, if the subrecord was present.
    pub version: Option<f32>,
    /// `HEDR` record count.
    pub record_count: Option<u32>,
    /// Author (`CNAM`).
    pub author: Option<String>,
    /// Description (`SNAM`).
    pub description: Option<String>,
    /// Master file names in the order the plugin lists them.
    pub masters: Vec<String>,
}

impl PluginHeader {
    /// True when the ESM flag is set.
    #[must_use]
    pub const fn is_master(&self) -> bool {
        self.flags & FLAG_ESM != 0
    }

    /// True when the ESL flag is set.
    #[must_use]
    pub const fn is_light(&self) -> bool {
        self.flags & FLAG_ESL != 0
    }
}

fn u32_at(data: &[u8], at: usize) -> Option<u32> {
    let bytes = data.get(at..at.checked_add(4)?)?;
    Some(u32::from_le_bytes(bytes.try_into().ok()?))
}

fn u16_at(data: &[u8], at: usize) -> Option<u16> {
    let bytes = data.get(at..at.checked_add(2)?)?;
    Some(u16::from_le_bytes(bytes.try_into().ok()?))
}

/// The characters of Windows-1252 bytes 0x80 to 0x9F; the five bytes it leaves undefined stay the C1 control of
/// the same number, as the WHATWG encoding (which esplugin uses through `encoding_rs`) decodes them.
const WINDOWS_1252_HIGH: [char; 32] = [
    '\u{20AC}', '\u{0081}', '\u{201A}', '\u{0192}', '\u{201E}', '\u{2026}', '\u{2020}', '\u{2021}',
    '\u{02C6}', '\u{2030}', '\u{0160}', '\u{2039}', '\u{0152}', '\u{008D}', '\u{017D}', '\u{008F}',
    '\u{0090}', '\u{2018}', '\u{2019}', '\u{201C}', '\u{201D}', '\u{2022}', '\u{2013}', '\u{2014}',
    '\u{02DC}', '\u{2122}', '\u{0161}', '\u{203A}', '\u{0153}', '\u{009D}', '\u{017E}', '\u{0178}',
];

/// Decode a zero-terminated Windows-1252 string, as esplugin decodes plugin strings: bytes below 0x80 and from
/// 0xA0 on are the code point of the same number, 0x80 to 0x9F are [`WINDOWS_1252_HIGH`].
fn zstring(payload: &[u8]) -> String {
    let end = payload
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(payload.len());
    payload[..end]
        .iter()
        .map(|&byte| match byte {
            0x80..=0x9F => WINDOWS_1252_HIGH[usize::from(byte - 0x80)],
            _ => char::from(byte),
        })
        .collect()
}

/// Number of header-data bytes announced by the first 24 bytes of a plugin.
///
/// # Errors
/// Returns [`PluginError`] when the bytes are not a `TES4` record header or the size is
/// implausible.
pub fn header_data_size(record_header: &[u8]) -> Result<usize, PluginError> {
    if record_header.len() < RECORD_HEADER_SIZE {
        return Err(PluginError::TooShort);
    }
    let magic: [u8; 4] = [
        record_header[0],
        record_header[1],
        record_header[2],
        record_header[3],
    ];
    if &magic != b"TES4" {
        return Err(PluginError::BadMagic(magic));
    }
    let size = u32_at(record_header, 4).ok_or(PluginError::TooShort)?;
    let size = usize::try_from(size).map_err(|_| PluginError::Truncated)?;
    if size > MAX_HEADER_DATA {
        return Err(PluginError::Truncated);
    }
    Ok(size)
}

/// Parse the header from the start of a plugin file's bytes.
///
/// Subrecords that run past the header data end the walk; whatever was read before is kept.
///
/// # Errors
/// Returns [`PluginError`] when the bytes do not start with a complete `TES4` record.
pub fn parse_header(bytes: &[u8]) -> Result<PluginHeader, PluginError> {
    let size = header_data_size(bytes)?;
    let data = bytes
        .get(RECORD_HEADER_SIZE..RECORD_HEADER_SIZE + size)
        .ok_or(PluginError::Truncated)?;
    let mut header = PluginHeader {
        flags: u32_at(bytes, 8).ok_or(PluginError::TooShort)?,
        form_version: u16_at(bytes, 20).ok_or(PluginError::TooShort)?,
        version: None,
        record_count: None,
        author: None,
        description: None,
        masters: Vec::new(),
    };

    let mut at = 0usize;
    let mut pending_size: Option<usize> = None;
    while let (Some(kind), Some(declared)) = (data.get(at..at + 4), u16_at(data, at + 4)) {
        let length = pending_size.take().unwrap_or_else(|| usize::from(declared));
        let start = at + 6;
        let Some(payload) = data.get(start..start + length) else {
            break;
        };
        match kind {
            b"XXXX" => {
                pending_size = u32_at(payload, 0).and_then(|value| usize::try_from(value).ok());
            }
            b"HEDR" => {
                header.version = u32_at(payload, 0).map(f32::from_bits);
                header.record_count = u32_at(payload, 4);
            }
            b"CNAM" => header.author = Some(zstring(payload)),
            b"SNAM" => header.description = Some(zstring(payload)),
            b"MAST" => header.masters.push(zstring(payload)),
            _ => {}
        }
        at = start + length;
    }
    Ok(header)
}

fn fill(file: &mut File, buffer: &mut [u8]) -> io::Result<usize> {
    let mut filled = 0;
    while filled < buffer.len() {
        let read = file.read(&mut buffer[filled..])?;
        if read == 0 {
            break;
        }
        filled += read;
    }
    Ok(filled)
}

/// Read and parse the header of a plugin file, reading only as many bytes as needed.
///
/// # Errors
/// Returns an I/O error for unreadable files; malformed headers become
/// [`io::ErrorKind::InvalidData`] wrapping a [`PluginError`].
pub fn read_header(path: &Path) -> io::Result<PluginHeader> {
    let invalid = |error: PluginError| io::Error::new(io::ErrorKind::InvalidData, error);
    let mut file = File::open(path)?;
    let mut buffer = vec![0u8; RECORD_HEADER_SIZE];
    let filled = fill(&mut file, &mut buffer)?;
    buffer.truncate(filled);
    let size = header_data_size(&buffer).map_err(invalid)?;
    // The size comes from the file itself: check it against the real length before allocating by it.
    let length = file.metadata()?.len();
    if u64::try_from(RECORD_HEADER_SIZE + size).map_or(true, |needed| needed > length) {
        return Err(invalid(PluginError::Truncated));
    }
    buffer.resize(RECORD_HEADER_SIZE + size, 0);
    let filled = fill(&mut file, &mut buffer[RECORD_HEADER_SIZE..])?;
    if filled < size {
        return Err(invalid(PluginError::Truncated));
    }
    parse_header(&buffer).map_err(invalid)
}

/// Build the bytes of a minimal plugin (header record only). Used by the unit and
/// integration tests; not a general plugin writer.
///
/// # Panics
/// Panics if the content does not fit the 16-bit subrecord sizes (tests only).
#[must_use]
pub fn synthetic_plugin(flags: u32, masters: &[&str], description: Option<&str>) -> Vec<u8> {
    synthetic_plugin_bytes(flags, masters, description.map(str::as_bytes))
}

/// [`synthetic_plugin`] with the description given as the bytes to store (for example Windows-1252).
///
/// # Panics
/// If a field is longer than a subrecord can hold (tests only).
#[must_use]
pub fn synthetic_plugin_bytes(flags: u32, masters: &[&str], description: Option<&[u8]>) -> Vec<u8> {
    fn subrecord(out: &mut Vec<u8>, kind: [u8; 4], payload: &[u8]) {
        out.extend_from_slice(&kind);
        let length = u16::try_from(payload.len()).expect("test subrecord fits in u16");
        out.extend_from_slice(&length.to_le_bytes());
        out.extend_from_slice(payload);
    }
    let mut data = Vec::new();
    let mut hedr = Vec::new();
    hedr.extend_from_slice(&1.34f32.to_le_bytes());
    hedr.extend_from_slice(&0u32.to_le_bytes());
    hedr.extend_from_slice(&0x800u32.to_le_bytes());
    subrecord(&mut data, *b"HEDR", &hedr);
    subrecord(&mut data, *b"CNAM", b"tester\0");
    if let Some(text) = description {
        let mut payload = text.to_vec();
        payload.push(0);
        subrecord(&mut data, *b"SNAM", &payload);
    }
    for master in masters {
        let mut payload = master.as_bytes().to_vec();
        payload.push(0);
        subrecord(&mut data, *b"MAST", &payload);
        subrecord(&mut data, *b"DATA", &0u64.to_le_bytes());
    }
    let mut out = Vec::new();
    out.extend_from_slice(b"TES4");
    let size = u32::try_from(data.len()).expect("test header fits in u32");
    out.extend_from_slice(&size.to_le_bytes());
    out.extend_from_slice(&flags.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&15u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&data);
    out
}

#[cfg(test)]
#[path = "plugin_tests.rs"]
mod tests;
