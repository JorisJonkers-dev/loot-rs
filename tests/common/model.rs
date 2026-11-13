//! An in-memory world for the condition language: a model of what exists, with no file system.

use loot_rs::condition::{ConditionError, Env};
use std::collections::{BTreeMap, BTreeSet};

/// The names the generators draw from, so that conditions hit and miss.
pub const NAMES: [&str; 6] = ["a.esp", "b.esp", "c.esm", "d.esp", "sub/e.dll", "f.esm"];

#[derive(Debug, Clone, Default)]
pub struct Model {
    pub files: BTreeSet<String>,
    pub active: BTreeSet<String>,
    pub sizes: BTreeMap<String, u64>,
    pub crcs: BTreeMap<String, u32>,
    pub masters: BTreeSet<String>,
}

impl Env for Model {
    fn count_files(&self, path: &str) -> Result<usize, ConditionError> {
        Ok(usize::from(self.files.contains(path))
            + usize::from(self.files.contains(&format!("{path}#2"))))
    }
    fn count_active(&self, path: &str) -> Result<usize, ConditionError> {
        Ok(usize::from(self.active.contains(path))
            + usize::from(self.active.contains(&format!("{path}#2"))))
    }
    fn file_size(&self, path: &str) -> Option<u64> {
        self.sizes.get(path).copied()
    }
    fn readable(&self, path: &str) -> bool {
        self.files.contains(path)
    }
    fn is_master(&self, path: &str) -> bool {
        self.masters.contains(path)
    }
    fn checksum(&self, path: &str) -> Option<u32> {
        self.crcs.get(path).copied()
    }
}
