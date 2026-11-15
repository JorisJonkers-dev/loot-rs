//! `version()` and `product_version()` on executables and DLLs (cargo feature `executable-versions`), and
//! `is_executable()` (every build): the bounded PE reader below is compiled in every build, only the call into
//! `loot-condition-interpreter` needs the feature.
//!
//! Comparing versions the way LOOT does is left to `loot-condition-interpreter` (MIT, by the LOOT authors).
//! Reading the file is not, because the interpreter's PE reader trusts the file: it sizes buffers from `u32`
//! fields (up to 4 GiB each, the number of data directories up to 32 GiB), adds 32-bit offsets unchecked, reads
//! the PE header offset as 16 bits, and loops forever (confirmed by running it under a timeout) on a string whose
//! length field is 0. So the file is read here, once, with every size checked against fixed limits and against the
//! file, and only the `VS_VERSIONINFO` block that was found and validated is handed on, inside a minimal image
//! that is written to a private temporary directory and removed again. The interpreter therefore never opens the
//! file that was checked: there is nothing to swap between the check and the use, and the file name is never
//! part of the expression it parses.
//!
//! The call is resolved against the `Data` directory by [`crate::env::FsEnv`] (case-insensitively, as for every
//! other function). Anything the checks do not like is a [`ConditionError::Environment`], never a hang or a large
//! allocation. `version()` on a plugin (`.esp`, `.esm`) is not handled here: its version lives in the description field and
//! [`crate::env::FsEnv`] answers it in every build ([`crate::version`]). `product_version()` on a plugin stays
//! unsupported.
//!
//! [`PeSpec`] builds small PE images with a version resource, for tests (no game files are needed).

#[cfg(feature = "executable-versions")]
use crate::condition::Arg;
use crate::condition::ConditionError;
#[cfg(feature = "executable-versions")]
use loot_condition_interpreter::{Expression, GameType, State};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
#[cfg(feature = "executable-versions")]
use std::path::PathBuf;

/// Most data directories a PE header may declare (real images declare 16).
pub const MAX_DATA_DIRECTORIES: u32 = 1024;
/// Most sections a PE image may have (the PE format allows 96).
pub const MAX_SECTIONS: usize = 96;
/// Largest resource directory that is accepted, in bytes. Only the few tables on the way to the version block are
/// read from it, never the whole directory.
pub const MAX_RESOURCE_DIRECTORY: u32 = 256 * 1024 * 1024;
/// Largest version block, in bytes. `wLength` of a `VS_VERSIONINFO` is a `u16`, so no valid block is larger.
pub const MAX_VERSION_BLOCK: u32 = 0xFFFF;
/// Most entries looked at in one resource directory table.
pub const MAX_TABLE_ENTRIES: u16 = 256;
/// Most resource directory tables read while looking for the version block.
pub const MAX_TABLES: usize = 64;

fn bad(path: &Path, why: &str) -> ConditionError {
    ConditionError::Environment(format!("{}: {why}", path.display()))
}

fn u16_at(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(bytes.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

/// `len` bytes at `at`, only if they lie inside the file (so the buffer is never larger than the file).
fn read_at(file: &mut File, length: u64, at: u64, len: usize) -> Option<Vec<u8>> {
    if at.checked_add(len as u64)? > length {
        return None;
    }
    file.seek(SeekFrom::Start(at)).ok()?;
    let mut buffer = vec![0; len];
    file.read_exact(&mut buffer).ok()?;
    Some(buffer)
}

/// What is in the resource directory table at `at`: `(name or id, offset or data entry)` pairs.
type Reader<'a> = dyn Fn(u64, usize) -> Option<Vec<u8>> + 'a;

fn is_table(target: u32) -> bool {
    target & 0x8000_0000 != 0
}

fn tables(
    read: &Reader,
    size: u32,
    count: &mut usize,
    at: u32,
) -> Option<Result<Vec<(u32, u32)>, &'static str>> {
    *count += 1;
    if *count > MAX_TABLES {
        return Some(Err("too many resource directory tables"));
    }
    let end = u64::from(at) + 16;
    if end > u64::from(size) {
        return None;
    }
    let head = read(u64::from(at), 16)?;
    let entries = usize::from(u16_at(&head, 12)?) + usize::from(u16_at(&head, 14)?);
    if entries > usize::from(MAX_TABLE_ENTRIES) {
        return Some(Err("resource directory table is too large"));
    }
    if end + 8 * entries as u64 > u64::from(size) {
        return None;
    }
    let raw = read(end, 8 * entries)?;
    Some(Ok((0..entries)
        .filter_map(|n| Some((u32_at(&raw, 8 * n)?, u32_at(&raw, 8 * n + 4)?)))
        .collect()))
}

/// Walk the resource directory of `size` bytes, read through `read` (offsets inside the directory), the way the
/// interpreter does: the first `RT_VERSION` (id 16) entry that is a table, its first entry that is a table, that
/// table's first entry that is a data entry. Returns that data entry's `(rva, size)`.
fn find_version_entry(read: &Reader, size: u32) -> Result<Option<(u32, u32)>, &'static str> {
    const BROKEN: &str = "malformed resource directory";
    let mut count = 0;
    let mut table = |at: u32| match tables(read, size, &mut count, at) {
        Some(result) => result,
        None => Err(BROKEN),
    };
    for (id, target) in table(0)? {
        if id != 16 || !is_table(target) {
            continue;
        }
        for (_, names) in table(target & 0x7FFF_FFFF)? {
            if !is_table(names) {
                continue;
            }
            for (_, leaf) in table(names & 0x7FFF_FFFF)? {
                if is_table(leaf) {
                    continue;
                }
                if u64::from(leaf) + 16 > u64::from(size) {
                    return Err(BROKEN);
                }
                let entry = read(u64::from(leaf), 16).ok_or(BROKEN)?;
                return match (u32_at(&entry, 0), u32_at(&entry, 4)) {
                    (Some(rva), Some(len)) => Ok(Some((rva, len))),
                    _ => Err(BROKEN),
                };
            }
        }
    }
    Ok(None)
}

/// One section header: `(virtual address, bytes in the file that count, file offset)`.
type Section = (u32, u32, u32);

/// The file offset of `size` bytes at `rva`, if one section holds them all.
fn rva_to_offset(
    sections: &[Section],
    rva: u32,
    size: u32,
    length: u64,
) -> Result<Option<u64>, &'static str> {
    for &(va, actual, raw) in sections {
        if rva >= va && u64::from(rva) + u64::from(size) <= u64::from(va) + u64::from(actual) {
            let offset = u64::from(raw) + u64::from(rva - va);
            if offset + u64::from(size) > length {
                return Err("data is cut off by the end of the file");
            }
            return Ok(Some(offset));
        }
    }
    Ok(None)
}

/// The version block of the file: `Ok(None)` when it has no resource directory or no version resource. The block
/// is validated by [`validate_block`].
fn version_block(
    path: &Path,
    file: &mut File,
    length: u64,
) -> Result<Option<Vec<u8>>, ConditionError> {
    let short = || bad(path, "truncated PE header");
    let head = read_at(file, length, 0, 0x40).ok_or_else(short)?;
    if &head[..2] != b"MZ" {
        return Err(bad(path, "not a PE image"));
    }
    let pe = u64::from(u32_at(&head, 0x3C).ok_or_else(short)?);
    let coff = read_at(file, length, pe, 24).ok_or_else(short)?;
    if &coff[..4] != b"PE\0\0" {
        return Err(bad(path, "not a PE image"));
    }
    let sections = usize::from(u16_at(&coff, 6).ok_or_else(short)?);
    let optional_size = u64::from(u16_at(&coff, 20).ok_or_else(short)?);
    if sections > MAX_SECTIONS {
        return Err(bad(path, "too many sections"));
    }
    let optional = pe + 24;
    let magic =
        u16_at(&read_at(file, length, optional, 2).ok_or_else(short)?, 0).ok_or_else(short)?;
    let (count_at, dirs_at) = match magic {
        0x10B => (92, 96),
        0x20B => (108, 112),
        _ => return Err(bad(path, "unknown optional header magic")),
    };
    if optional_size < count_at + 4 {
        return Err(bad(path, "optional header is too small"));
    }
    let count = u32_at(
        &read_at(file, length, optional + count_at, 4).ok_or_else(short)?,
        0,
    )
    .ok_or_else(short)?;
    if count > MAX_DATA_DIRECTORIES {
        return Err(bad(path, "too many data directories"));
    }
    if count < 3 {
        return Ok(None);
    }
    if optional_size < dirs_at + 24 {
        return Err(bad(
            path,
            "optional header is too small for its data directories",
        ));
    }
    let dir = read_at(file, length, optional + dirs_at + 16, 8).ok_or_else(short)?;
    let (dir_va, dir_size) = (
        u32_at(&dir, 0).ok_or_else(short)?,
        u32_at(&dir, 4).ok_or_else(short)?,
    );
    if dir_size == 0 {
        return Ok(None);
    }
    if dir_size > MAX_RESOURCE_DIRECTORY {
        return Err(bad(path, "resource directory is larger than the limit"));
    }
    let table = read_at(file, length, optional + optional_size, 40 * sections).ok_or_else(short)?;
    let mut mapped: Vec<Section> = Vec::with_capacity(sections);
    for entry in table.as_chunks::<40>().0 {
        let field = |at| u32_at(entry, at).ok_or_else(short);
        let (virtual_size, va, raw_size, raw_at) = (field(8)?, field(12)?, field(16)?, field(20)?);
        mapped.push((va, raw_size.min(virtual_size), raw_at));
    }
    let map = |rva, size| rva_to_offset(&mapped, rva, size, length).map_err(|why| bad(path, why));
    let Some(base) = map(dir_va, dir_size)? else {
        return Ok(None);
    };
    let file = std::cell::RefCell::new(file);
    let read = |at: u64, len: usize| read_at(&mut file.borrow_mut(), length, base + at, len);
    let Some((rva, size)) = find_version_entry(&read, dir_size).map_err(|why| bad(path, why))?
    else {
        return Ok(None);
    };
    if size > MAX_VERSION_BLOCK {
        return Err(bad(
            path,
            "version resource is larger than a version block can be",
        ));
    }
    let Some(at) = map(rva, size)? else {
        return Err(bad(path, "version resource is outside every section"));
    };
    let block = read_at(&mut file.borrow_mut(), length, at, size as usize).ok_or_else(short)?;
    validate_block(&block).map_err(|why| bad(path, why))?;
    Ok(Some(block))
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

/// Check the outer structure of a `VS_VERSIONINFO` block the way the interpreter will read it: `wLength` is the
/// block, the key is `VS_VERSION_INFO`, the value is absent or a `VS_FIXEDFILEINFO` with its signature.
fn validate_block(block: &[u8]) -> Result<(), &'static str> {
    if block.len() < 40 {
        return Err("version block is too short");
    }
    if usize::from(u16_at(block, 0).unwrap_or(0)) != block.len() {
        return Err("version block length does not match its resource");
    }
    if !utf16_key(block, 6, "VS_VERSION_INFO") {
        return Err("version block has the wrong key");
    }
    match usize::from(u16_at(block, 2).unwrap_or(1)) {
        0 => Ok(()),
        52 if block.len() >= 92 && u32_at(block, 40) == Some(0xFEEF_04BD) => Ok(()),
        _ => Err("version block has no valid fixed file info"),
    }
}

/// Does the first string table of the first `StringFileInfo` hold a `ProductVersion`? Every length is checked
/// against its parent and must be at least its own header, so each step moves forward and stays inside; this is the
/// walk the interpreter makes to the same string, where it stops. When the first table has none, the interpreter's
/// walk is not safe to follow, so it is not asked and the answer is `false`.
///
/// # Errors
/// A sentence when a length is too small or runs past its parent.
fn has_product_version(block: &[u8]) -> Result<bool, &'static str> {
    const MALFORMED: &str = "malformed version block";
    let mut at = 40 + usize::from(u16_at(block, 2).ok_or(MALFORMED)?);
    while at + 6 <= block.len() {
        let length = usize::from(u16_at(block, at).ok_or(MALFORMED)?);
        if length < 6 || at + length > block.len() {
            return Err(MALFORMED);
        }
        if utf16_key(block, at + 6, "StringFileInfo") {
            let end = at + length;
            let table = at + 36;
            if table == end {
                at += aligned(length);
                continue;
            }
            let table_length = usize::from(u16_at(block, table).ok_or(MALFORMED)?);
            if table_length < 24 || table + table_length > end {
                return Err(MALFORMED);
            }
            let end = table + table_length;
            let mut string = table + 24;
            while string + 6 <= end {
                let string_length = usize::from(u16_at(block, string).ok_or(MALFORMED)?);
                if string_length < 6 || string + string_length > end {
                    return Err(MALFORMED);
                }
                if utf16_key(block, string + 6, "ProductVersion") {
                    let value = 2 * usize::from(u16_at(block, string + 2).ok_or(MALFORMED)?);
                    return if string + 36 + value <= end {
                        Ok(true)
                    } else {
                        Err(MALFORMED)
                    };
                }
                string += aligned(string_length);
            }
            return Ok(false);
        }
        at += aligned(length);
    }
    Ok(false)
}

/// The validated version block of the file, when `function` can have a value for it.
fn read_block(path: &Path, function: &str) -> Result<Option<Vec<u8>>, ConditionError> {
    let mut file = File::open(path).map_err(|error| bad(path, &error.to_string()))?;
    let length = file
        .metadata()
        .map_err(|error| bad(path, &error.to_string()))?
        .len();
    let Some(block) = version_block(path, &mut file, length)? else {
        return Ok(None);
    };
    if function == "product_version"
        && !has_product_version(&block).map_err(|why| bad(path, why))?
    {
        return Ok(None);
    }
    Ok(Some(block))
}

/// Check that evaluating `function` on the file at `path` cannot take unbounded memory or time.
///
/// Returns `Ok(false)` when the file certainly has no value for the function (the condition is then false,
/// as in LOOT, and the interpreter is not called), `Ok(true)` when it may be called.
///
/// # Errors
/// [`ConditionError::Environment`] when the file is not a PE image or any size in it is out of bounds.
pub fn preflight(path: &Path, function: &str) -> Result<bool, ConditionError> {
    read_block(path, function).map(|block| block.is_some())
}

/// `is_executable(path)`: whether the file at `path` reads as a Windows executable, as `loot-condition-interpreter`
/// decides it (its PE reader walks the headers, the section table and the version resource if there is one without
/// an error). The walk here is this module's bounded one, so a file the interpreter would read with an absurd size
/// is refused (false) instead of being read. A missing file or a directory is false.
#[must_use]
pub fn is_executable(path: &Path) -> bool {
    path.is_file() && read_block(path, "version").is_ok()
}

/// A private directory under the temporary directory (mode 0700, created fresh, so nothing can be waiting in it),
/// removed with its content when dropped.
#[cfg(feature = "executable-versions")]
struct Scratch(PathBuf);

#[cfg(feature = "executable-versions")]
impl Scratch {
    fn new() -> std::io::Result<Self> {
        use std::os::unix::fs::DirBuilderExt;
        use std::sync::atomic::{AtomicUsize, Ordering};
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.subsec_nanos());
        let path = std::env::temp_dir().join(format!(
            "loot-rs-pe-{}-{}-{nanos}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::DirBuilder::new().mode(0o700).create(&path)?;
        Ok(Self(path))
    }
}

#[cfg(feature = "executable-versions")]
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Evaluate `version(...)` or `product_version(...)` (`name`, with its `args`) on the file at `resolved`, which
/// the caller found under `Data`, if it did. A missing file is false.
///
/// # Errors
/// [`ConditionError::Syntax`] for a call the interpreter rejects, [`ConditionError::Unsupported`] for a
/// plugin file, [`ConditionError::Environment`] for a file that cannot be read as a PE image.
#[cfg(feature = "executable-versions")]
pub fn evaluate(name: &str, args: &[Arg], resolved: Option<&Path>) -> Result<bool, ConditionError> {
    let syntax = |text: String| ConditionError::Syntax(text);
    if args.len() != 3 {
        return Err(syntax(format!(
            "{name}() takes 3 arguments, got {}",
            args.len()
        )));
    }
    // By the name in the condition, so that a plugin that is not installed is as unsupported as one that is.
    let Arg::Str(wanted) = &args[0] else {
        return Err(syntax(format!(
            "{name}() expects a quoted string as argument 1"
        )));
    };
    let extension = Path::new(wanted)
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase);
    if matches!(extension.as_deref(), Some("esp" | "esm" | "esl" | "ghost")) {
        return Err(ConditionError::Unsupported(name.to_string()));
    }
    // The rest of the call is rebuilt as text for the interpreter, so it must be plain: a version in quotes with no
    // control character, and one of the six comparison operators.
    let mut rest = Vec::new();
    for arg in &args[1..] {
        match arg {
            Arg::Str(text) if !text.chars().any(char::is_control) && !text.contains('"') => {
                rest.push(arg.to_string());
            }
            Arg::Word(op) if matches!(op.as_str(), "==" | "!=" | "<" | ">" | "<=" | ">=") => {
                rest.push(arg.to_string());
            }
            _ => {
                return Err(syntax(format!(
                    "{name}() has an argument that is not a plain version or comparison"
                )));
            }
        }
    }
    let Some(path) = resolved.filter(|path| path.is_file()) else {
        return Ok(false);
    };
    let Some(block) = read_block(path, name)? else {
        return Ok(false);
    };
    let scratch =
        Scratch::new().map_err(|error| bad(path, &format!("no scratch directory: {error}")))?;
    let image = PeSpec::with_block(block).build();
    std::fs::write(scratch.0.join("v.dll"), image)
        .map_err(|error| bad(path, &format!("scratch file: {error}")))?;
    let text = format!("{name}(\"v.dll\", {})", rest.join(", "));
    let expression: Expression = text.parse().map_err(|error| syntax(format!("{error}")))?;
    let state = State::new(GameType::FalloutNV, scratch.0.clone());
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
    /// File offset of the `PE` signature (at least `0x40`).
    pub pe_offset: usize,
    /// Bytes in the section before the resource directory, so that the directory does not start its section.
    pub lead: usize,
}

impl PeSpec {
    /// A well-formed image with the given file version and `ProductVersion` string.
    #[must_use]
    pub fn new(file: [u16; 4], product: &str) -> Self {
        Self::with_block(version_block_bytes(file, product))
    }

    /// A well-formed image around a version block.
    #[must_use]
    pub fn with_block(block: Vec<u8>) -> Self {
        Self {
            block,
            directories: 16,
            directory_size: None,
            leaf_size: None,
            pe_offset: 0x40,
            lead: 0,
        }
    }

    /// The bytes of the image.
    ///
    /// # Panics
    /// When the block is larger than a `u32` (a test input that big is a mistake).
    #[must_use]
    pub fn build(&self) -> Vec<u8> {
        let tree = resource_tree(&self.block, self.leaf_size, self.lead);
        let small = |n: usize| u32::try_from(n).expect("small");
        let pe = self.pe_offset;
        let optional = pe + 24;
        let section = optional + 224;
        let raw_at = (section + 40).next_multiple_of(0x200);
        let mut image = vec![0u8; raw_at];
        image[..2].copy_from_slice(b"MZ");
        image[0x3C..0x40].copy_from_slice(&small(pe).to_le_bytes());
        image[pe..pe + 4].copy_from_slice(b"PE\0\0");
        image[pe + 4..pe + 6].copy_from_slice(&0x14Cu16.to_le_bytes()); // machine
        image[pe + 6..pe + 8].copy_from_slice(&1u16.to_le_bytes()); // sections
        image[pe + 20..pe + 22].copy_from_slice(&224u16.to_le_bytes()); // optional header size
        image[optional..optional + 2].copy_from_slice(&0x10Bu16.to_le_bytes());
        image[optional + 92..optional + 96].copy_from_slice(&self.directories.to_le_bytes());
        let resource_dir = optional + 96 + 16;
        image[resource_dir..resource_dir + 4]
            .copy_from_slice(&small(0x1000 + self.lead).to_le_bytes());
        let size = self.directory_size.unwrap_or(small(tree.len()));
        image[resource_dir + 4..resource_dir + 8].copy_from_slice(&size.to_le_bytes());
        image[section..section + 6].copy_from_slice(b".rsrc\0");
        let raw = small(self.lead + tree.len());
        image[section + 8..section + 12].copy_from_slice(&raw.to_le_bytes());
        image[section + 12..section + 16].copy_from_slice(&0x1000u32.to_le_bytes());
        image[section + 16..section + 20].copy_from_slice(&raw.to_le_bytes());
        image[section + 20..section + 24].copy_from_slice(&small(raw_at).to_le_bytes());
        image.resize(raw_at + self.lead, 0);
        image.extend_from_slice(&tree);
        image
    }
}

fn resource_tree(block: &[u8], leaf_size: Option<u32>, lead: usize) -> Vec<u8> {
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
    let rva = u32::try_from(0x1000 + lead + 0x58).expect("small");
    tree.extend_from_slice(&rva.to_le_bytes()); // data RVA
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

#[cfg(all(test, feature = "executable-versions"))]
#[path = "exe_version_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "exe_version_reader_tests.rs"]
mod reader_tests;
