//! The load order and the `Data` directory that conditions and rules are evaluated against.
//!
//! Paths in metadata are relative to the `Data` directory and are matched case-insensitively
//! (the game's file system is, a Linux one is not), including `..` for the game directory.

use crate::condition::{ConditionError, Env};
use crate::metadata::{compile_whole_name, looks_like_regex};
use crate::plugin;
use std::cell::RefCell;
use std::collections::HashMap;
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

static CRC_TABLE: [u32; 256] = crc_table();

/// CRC-32 (IEEE, as used by zip and by LOOT's file checksums) of `bytes`.
#[must_use]
pub fn crc32(bytes: &[u8]) -> u32 {
    !bytes.iter().fold(!0u32, |crc, &byte| {
        CRC_TABLE[usize::from(crc.to_le_bytes()[0] ^ byte)] ^ (crc >> 8)
    })
}

/// A load order over a `Data` directory on disk.
pub struct FsEnv {
    data: PathBuf,
    slots: Vec<Slot>,
    listings: RefCell<HashMap<PathBuf, Vec<String>>>,
}

impl FsEnv {
    /// Create the environment for `slots` (in load order) installed under `data`.
    #[must_use]
    pub fn new(data: &Path, slots: Vec<Slot>) -> Self {
        Self {
            data: data.to_path_buf(),
            slots,
            listings: RefCell::new(HashMap::new()),
        }
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

    fn listing(&self, dir: &Path) -> Result<Vec<String>, ConditionError> {
        if let Some(cached) = self.listings.borrow().get(dir) {
            return Ok(cached.clone());
        }
        let names = match fs::read_dir(dir) {
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
        self.listings
            .borrow_mut()
            .insert(dir.to_path_buf(), names.clone());
        Ok(names)
    }

    /// Resolve a `/`-separated path under `Data`, matching each component case-insensitively.
    #[must_use]
    pub fn resolve(&self, relative: &str) -> Option<PathBuf> {
        let mut current = self.data.clone();
        for part in relative.split('/').filter(|part| !part.is_empty()) {
            match part {
                "." => {}
                ".." => {
                    current.pop();
                }
                name => {
                    let direct = current.join(name);
                    if direct.exists() {
                        current = direct;
                    } else {
                        let lower = name.to_lowercase();
                        let found = self
                            .listing(&current)
                            .ok()?
                            .into_iter()
                            .find(|candidate| candidate.to_lowercase() == lower)?;
                        current = current.join(found);
                    }
                }
            }
        }
        Some(current)
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
        let regex = compile_whole_name(pattern).map_err(ConditionError::Syntax)?;
        let Some(dir) = self.resolve(dir) else {
            return Ok(0);
        };
        Ok(self
            .listing(&dir)?
            .iter()
            .filter(|name| regex.is_match(name))
            .count())
    }

    fn count_active(&self, path: &str) -> Result<usize, ConditionError> {
        if !looks_like_regex(path) {
            return Ok(usize::from(self.is_active(path)));
        }
        let (dir, pattern) = Self::regex_parts(path);
        if !dir.is_empty() {
            return Ok(0);
        }
        let regex = compile_whole_name(pattern).map_err(ConditionError::Syntax)?;
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
        path.is_file()
            .then(|| fs::read(path).ok())
            .flatten()
            .map(|bytes| crc32(&bytes))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc32_matches_the_standard_check_value() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b""), 0);
    }

    #[test]
    fn plugins_txt_active_only_style() {
        let slots = parse_plugins_txt("# header\r\nA.esm\r\n\r\nB.esp\r\n");
        assert_eq!(slots.len(), 2);
        assert!(slots.iter().all(|slot| slot.active));
    }

    #[test]
    fn plugins_txt_starred_style() {
        let slots = parse_plugins_txt("*A.esm\n B.esp\n*C.esp\n");
        let flags: Vec<bool> = slots.iter().map(|slot| slot.active).collect();
        assert_eq!(flags, [true, false, true]);
        assert_eq!(slots[0].name, "A.esm");
    }
}
