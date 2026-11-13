//! `version()` and `product_version()` on executables and DLLs (cargo feature `executable-versions`).
//!
//! Reading a PE version resource and comparing versions the way LOOT does is left to
//! `loot-condition-interpreter` (MIT, by the LOOT authors). Two things are kept here:
//!
//! * the call is resolved against the `Data` directory by [`crate::env::FsEnv`] (case-insensitively, as for
//!   every other function) and handed to the interpreter as one call on one file;
//! * the file is checked first by [`preflight`], a bounded walk over the few headers the interpreter trusts,
//!   because the interpreter sizes buffers from fields in the file (a `u32` is up to 4 GiB each, the number of
//!   data directories up to 32 GiB) and, for `product_version`, can loop forever on a malformed string table.
//!   Anything the walk does not like is a [`ConditionError::Environment`], never a hang or an allocation.
//!
//! Plugin files (`.esp`, `.esm`) are not handled: their version lives in the description field and stays
//! unsupported, so an item that asks for it is still reported and not applied.
//!
//! [`synthetic_pe`] builds a small PE image with a version resource, for tests (no game files are needed).

use crate::condition::{Arg, ConditionError};
use loot_condition_interpreter::{Expression, GameType, State};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// Most data directories a PE header may declare (real images declare 16).
pub const MAX_DATA_DIRECTORIES: u32 = 1024;
/// Largest resource directory that is read, in bytes (the real ones are a few KiB to a few hundred KiB).
pub const MAX_RESOURCE_DIRECTORY: u32 = 16 * 1024 * 1024;
/// Largest version block, in bytes. `wLength` of a `VS_VERSIONINFO` is a `u16`, so no valid block is larger.
pub const MAX_VERSION_BLOCK: u32 = 0xFFFF;
/// Most entries looked at in one resource directory table.
const MAX_TABLE_ENTRIES: u16 = 256;

fn bad(path: &Path, why: &str) -> ConditionError {
    ConditionError::Environment(format!("{}: {why}", path.display()))
}

fn u16_at(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(bytes.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

fn read_at(file: &mut File, at: u64, len: usize) -> Option<Vec<u8>> {
    file.seek(SeekFrom::Start(at)).ok()?;
    let mut buffer = vec![0; len];
    file.read_exact(&mut buffer).ok()?;
    Some(buffer)
}

/// The version block of the file: `Ok(None)` when it has no resource directory or no version resource.
fn version_block(
    path: &Path,
    file: &mut File,
    length: u64,
) -> Result<Option<Vec<u8>>, ConditionError> {
    let short = || bad(path, "truncated PE header");
    // Mirrors what the interpreter reads: a 16-bit PE header offset at 0x3C.
    let head = read_at(file, 0, 0x40).ok_or_else(short)?;
    if &head[..2] != b"MZ" {
        return Err(bad(path, "not a PE image"));
    }
    let pe = u64::from(u16_at(&head, 0x3C).ok_or_else(short)?);
    let coff = read_at(file, pe, 24).ok_or_else(short)?;
    if &coff[..4] != b"PE\0\0" {
        return Err(bad(path, "not a PE image"));
    }
    let sections = usize::from(u16_at(&coff, 6).ok_or_else(short)?);
    let optional_size = u64::from(u16_at(&coff, 20).ok_or_else(short)?);
    let optional = pe + 24;
    let magic = u16_at(&read_at(file, optional, 2).ok_or_else(short)?, 0).ok_or_else(short)?;
    let (count_at, dirs_at) = match magic {
        0x10B => (92, 96),
        0x20B => (108, 112),
        _ => return Err(bad(path, "unknown optional header magic")),
    };
    let count =
        u32_at(&read_at(file, optional + count_at, 4).ok_or_else(short)?, 0).ok_or_else(short)?;
    if count > MAX_DATA_DIRECTORIES {
        return Err(bad(path, "too many data directories"));
    }
    if count < 3 {
        return Ok(None);
    }
    let dir = read_at(file, optional + dirs_at + 16, 8).ok_or_else(short)?;
    let (dir_va, dir_size) = (
        u32_at(&dir, 0).ok_or_else(short)?,
        u32_at(&dir, 4).ok_or_else(short)?,
    );
    if dir_size == 0 {
        return Ok(None);
    }
    if dir_size > MAX_RESOURCE_DIRECTORY || u64::from(dir_size) > length {
        return Err(bad(
            path,
            "resource directory is larger than the limit or the file",
        ));
    }
    // The section table follows the optional header.
    let table = pe + 24 + optional_size;
    for index in 0..sections {
        let entry = read_at(file, table + 40 * index as u64, 40).ok_or_else(short)?;
        let virtual_size = u32_at(&entry, 8).ok_or_else(short)?;
        let va = u32_at(&entry, 12).ok_or_else(short)?;
        let raw_size = u32_at(&entry, 16).ok_or_else(short)?;
        let raw_at = u32_at(&entry, 20).ok_or_else(short)?;
        // The interpreter adds these in 32 bits: refuse what would overflow (a panic in a debug build).
        let actual = raw_size.min(virtual_size);
        let overflow = || bad(path, "section table overflows 32 bits");
        let end = va.checked_add(actual).ok_or_else(overflow)?;
        let dir_end = dir_va.checked_add(dir_size).ok_or_else(overflow)?;
        if dir_va < va || dir_end > end {
            continue;
        }
        raw_at.checked_add(dir_va - va).ok_or_else(overflow)?;
        let data = read_at(
            file,
            u64::from(raw_at) + u64::from(dir_va - va),
            dir_size as usize,
        )
        .ok_or_else(|| bad(path, "resource directory is cut off"))?;
        return version_from_resources(path, &data, va);
    }
    Ok(None)
}

/// Walk the resource tree for `RT_VERSION` (id 16), checking every size on the way, and return the block.
fn version_from_resources(
    path: &Path,
    tree: &[u8],
    section_va: u32,
) -> Result<Option<Vec<u8>>, ConditionError> {
    let broken = || bad(path, "malformed resource directory");
    let entries = |at: usize| -> Result<Vec<(u32, u32)>, ConditionError> {
        let named = usize::from(u16_at(tree, at + 12).ok_or_else(broken)?);
        let ids = usize::from(u16_at(tree, at + 14).ok_or_else(broken)?);
        if named + ids > usize::from(MAX_TABLE_ENTRIES) {
            return Err(bad(path, "resource directory table is too large"));
        }
        (0..named + ids)
            .map(|n| {
                let entry = at + 16 + 8 * n;
                Ok((
                    u32_at(tree, entry).ok_or_else(broken)?,
                    u32_at(tree, entry + 4).ok_or_else(broken)?,
                ))
            })
            .collect()
    };
    let offset = |value: u32| usize::try_from(value & 0x7FFF_FFFF).map_err(|_| broken());
    let mut found = None;
    for (id, target) in entries(0)? {
        if id != 16 || target & 0x8000_0000 == 0 {
            continue;
        }
        for (_, target) in entries(offset(target)?)? {
            let leaves = if target & 0x8000_0000 == 0 {
                vec![(0, target)]
            } else {
                entries(offset(target)?)?
            };
            for (_, leaf) in leaves {
                if leaf & 0x8000_0000 != 0 {
                    return Err(bad(path, "resource directory is nested too deep"));
                }
                let at = offset(leaf)?;
                let (rva, size) = (
                    u32_at(tree, at).ok_or_else(broken)?,
                    u32_at(tree, at + 4).ok_or_else(broken)?,
                );
                if size > MAX_VERSION_BLOCK {
                    return Err(bad(
                        path,
                        "version resource is larger than a version block can be",
                    ));
                }
                if found.is_none() {
                    let from = usize::try_from(rva.checked_sub(section_va).ok_or_else(broken)?)
                        .map_err(|_| broken())?;
                    found = Some(
                        tree.get(from..from + size as usize)
                            .ok_or_else(broken)?
                            .to_vec(),
                    );
                }
            }
        }
    }
    Ok(found)
}

fn utf16_key(block: &[u8], at: usize, key: &str) -> bool {
    let wide: Vec<u8> = key
        .encode_utf16()
        .chain([0])
        .flat_map(u16::to_le_bytes)
        .collect();
    block.get(at..at + wide.len()) == Some(wide.as_slice())
}

fn aligned(length: usize) -> usize {
    length + (4 - length % 4) % 4
}

/// Does the first string table of the block hold a `ProductVersion`, by a walk in which every step moves
/// forward? When it does, the interpreter reaches the string by the same steps and stops there; when it does
/// not, the interpreter may not stop at all, so it is not called.
fn has_product_version(block: &[u8]) -> bool {
    let mut at = 40 + usize::from(u16_at(block, 2).unwrap_or(0));
    while at + 6 <= block.len() {
        let Some(length) = u16_at(block, at)
            .map(usize::from)
            .filter(|length| *length >= 6)
        else {
            return false;
        };
        if utf16_key(block, at + 6, "StringFileInfo") {
            let table = at + 36;
            let Some(table_length) = u16_at(block, table)
                .map(usize::from)
                .filter(|length| *length >= 24)
            else {
                return false;
            };
            let end = (table + table_length).min(block.len());
            let mut string = table + 24;
            while string + 6 <= end {
                let Some(string_length) = u16_at(block, string)
                    .map(usize::from)
                    .filter(|length| *length >= 6)
                else {
                    return false;
                };
                if utf16_key(block, string + 6, "ProductVersion") {
                    return true;
                }
                string += aligned(string_length);
            }
            return false;
        }
        at += aligned(length);
    }
    false
}

/// Check that evaluating `function` on the file at `path` cannot take unbounded memory or time.
///
/// Returns `Ok(false)` when the file certainly has no value for the function (the condition is then false,
/// as in LOOT, and the interpreter is not called), `Ok(true)` when the interpreter may be called.
///
/// # Errors
/// [`ConditionError::Environment`] when the file is not a PE image or any size in it is out of bounds.
pub fn preflight(path: &Path, function: &str) -> Result<bool, ConditionError> {
    let mut file = File::open(path).map_err(|error| bad(path, &error.to_string()))?;
    let length = file
        .metadata()
        .map_err(|error| bad(path, &error.to_string()))?
        .len();
    let Some(block) = version_block(path, &mut file, length)? else {
        return Ok(false);
    };
    Ok(function != "product_version" || has_product_version(&block))
}

/// Evaluate `version(...)` or `product_version(...)` (`name`, with its `args`) on the file at `resolved`, which
/// the caller found under `Data`, if it did. A missing file is false.
///
/// # Errors
/// [`ConditionError::Syntax`] for a call the interpreter rejects, [`ConditionError::Unsupported`] for a
/// plugin file, [`ConditionError::Environment`] for a file that cannot be read as a PE image.
pub fn evaluate(name: &str, args: &[Arg], resolved: Option<&Path>) -> Result<bool, ConditionError> {
    if args.len() != 3 {
        return Err(ConditionError::Syntax(format!(
            "{name}() takes 3 arguments, got {}",
            args.len()
        )));
    }
    // By the name in the condition, so that a plugin that is not installed is as unsupported as one that is.
    let wanted = match &args[0] {
        Arg::Str(text) => text,
        Arg::Word(_) => {
            return Err(ConditionError::Syntax(format!(
                "{name}() expects a quoted string as argument 1"
            )));
        }
    };
    let extension = Path::new(wanted)
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase);
    if matches!(extension.as_deref(), Some("esp" | "esm" | "esl" | "ghost")) {
        return Err(ConditionError::Unsupported(name.to_string()));
    }
    let Some(path) = resolved.filter(|path| path.is_file()) else {
        return Ok(false);
    };
    let (Some(parent), Some(file_name)) =
        (path.parent(), path.file_name().and_then(|n| n.to_str()))
    else {
        return Err(bad(path, "no usable file name"));
    };
    if file_name.contains('"') {
        return Err(bad(path, "file name holds a quote"));
    }
    if !preflight(path, name)? {
        return Ok(false);
    }
    let rest: Vec<String> = args[1..].iter().map(ToString::to_string).collect();
    let text = format!("{name}(\"{file_name}\", {})", rest.join(", "));
    let expression: Expression = text
        .parse()
        .map_err(|error| ConditionError::Syntax(format!("{error}")))?;
    let state = State::new(GameType::FalloutNV, parent.to_path_buf());
    expression
        .eval(&state)
        .map_err(|error| ConditionError::Environment(error.to_string()))
}

/// A synthetic PE32 image whose only content is a version resource, for tests.
#[derive(Debug, Clone)]
pub struct PeSpec {
    /// The version block (`VS_VERSIONINFO`), see [`version_block_bytes`].
    pub block: Vec<u8>,
    /// Value of `NumberOfRvaAndSizes`.
    pub directories: u32,
    /// Size written in the resource data directory, `None` for the real size.
    pub directory_size: Option<u32>,
    /// Size written in the version data entry, `None` for the length of `block`.
    pub leaf_size: Option<u32>,
}

impl PeSpec {
    /// A well-formed image with the given file version and `ProductVersion` string.
    #[must_use]
    pub fn new(file: [u16; 4], product: &str) -> Self {
        Self {
            block: version_block_bytes(file, product),
            directories: 16,
            directory_size: None,
            leaf_size: None,
        }
    }

    /// The bytes of the image.
    ///
    /// # Panics
    /// When the block is larger than a `u32` (a test input that big is a mistake).
    #[must_use]
    pub fn build(&self) -> Vec<u8> {
        let tree = resource_tree(&self.block, self.leaf_size);
        let mut image = vec![0u8; 0x200];
        image[..2].copy_from_slice(b"MZ");
        image[0x3C..0x3E].copy_from_slice(&0x40u16.to_le_bytes());
        image[0x40..0x44].copy_from_slice(b"PE\0\0");
        image[0x44..0x46].copy_from_slice(&0x14Cu16.to_le_bytes()); // machine
        image[0x46..0x48].copy_from_slice(&1u16.to_le_bytes()); // sections
        image[0x54..0x56].copy_from_slice(&224u16.to_le_bytes()); // optional header size
        let optional = 0x58;
        image[optional..optional + 2].copy_from_slice(&0x10Bu16.to_le_bytes());
        image[optional + 92..optional + 96].copy_from_slice(&self.directories.to_le_bytes());
        let resource_dir = optional + 96 + 16;
        image[resource_dir..resource_dir + 4].copy_from_slice(&0x1000u32.to_le_bytes());
        let size = self
            .directory_size
            .unwrap_or(u32::try_from(tree.len()).expect("small"));
        image[resource_dir + 4..resource_dir + 8].copy_from_slice(&size.to_le_bytes());
        let section = 0x58 + 224;
        image[section..section + 6].copy_from_slice(b".rsrc\0");
        let raw = u32::try_from(tree.len()).expect("small");
        image[section + 8..section + 12].copy_from_slice(&raw.to_le_bytes());
        image[section + 12..section + 16].copy_from_slice(&0x1000u32.to_le_bytes());
        image[section + 16..section + 20].copy_from_slice(&raw.to_le_bytes());
        image[section + 20..section + 24].copy_from_slice(&0x200u32.to_le_bytes());
        image.extend_from_slice(&tree);
        image
    }
}

fn resource_tree(block: &[u8], leaf_size: Option<u32>) -> Vec<u8> {
    fn directory(out: &mut Vec<u8>, id: u32, target: u32) {
        out.extend_from_slice(&[0; 12]);
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&id.to_le_bytes());
        out.extend_from_slice(&target.to_le_bytes());
    }
    let mut tree = Vec::new();
    directory(&mut tree, 16, 0x8000_0018); // root: RT_VERSION -> table at 0x18
    directory(&mut tree, 1, 0x8000_0030); // names: 1 -> table at 0x30
    directory(&mut tree, 0x409, 0x48); // languages: 0x409 -> data entry at 0x48
    let size = leaf_size.unwrap_or(u32::try_from(block.len()).expect("small"));
    tree.extend_from_slice(&(0x1000u32 + 0x58).to_le_bytes()); // data RVA
    tree.extend_from_slice(&size.to_le_bytes());
    tree.extend_from_slice(&[0; 8]); // code page, reserved
    tree.extend_from_slice(block);
    tree
}

fn utf16(text: &str) -> Vec<u8> {
    text.encode_utf16()
        .chain([0])
        .flat_map(u16::to_le_bytes)
        .collect()
}

fn pad4(bytes: &mut Vec<u8>) {
    while !bytes.len().is_multiple_of(4) {
        bytes.push(0);
    }
}

fn node(key: &str, value_length: u16, kind: u16, value: &[u8], children: &[u8]) -> Vec<u8> {
    let mut out = vec![0, 0];
    out.extend_from_slice(&value_length.to_le_bytes());
    out.extend_from_slice(&kind.to_le_bytes());
    out.extend_from_slice(&utf16(key));
    pad4(&mut out);
    out.extend_from_slice(value);
    pad4(&mut out);
    out.extend_from_slice(children);
    let length = u16::try_from(out.len()).expect("small block");
    out[..2].copy_from_slice(&length.to_le_bytes());
    out
}

/// A `VS_VERSIONINFO` block with `file` (major, minor, patch, build) as the fixed file version and a
/// `StringFileInfo` holding `ProductVersion`.
///
/// # Panics
/// When `product` is longer than a version block can hold.
#[must_use]
pub fn version_block_bytes(file: [u16; 4], product: &str) -> Vec<u8> {
    let mut fixed = Vec::new();
    fixed.extend_from_slice(&0xFEEF_04BDu32.to_le_bytes());
    fixed.extend_from_slice(&0x0001_0000u32.to_le_bytes());
    fixed.extend_from_slice(&((u32::from(file[0]) << 16) | u32::from(file[1])).to_le_bytes());
    fixed.extend_from_slice(&((u32::from(file[2]) << 16) | u32::from(file[3])).to_le_bytes());
    fixed.resize(52, 0);
    let value = utf16(product);
    let wide_chars = u16::try_from(value.len() / 2).expect("short");
    let string = node("ProductVersion", wide_chars, 1, &value, &[]);
    let table = node("040904b0", 0, 1, &[], &string);
    let info = node("StringFileInfo", 0, 1, &[], &table);
    node("VS_VERSION_INFO", 52, 0, &fixed, &info)
}

#[cfg(test)]
#[path = "exe_version_tests.rs"]
mod tests;
