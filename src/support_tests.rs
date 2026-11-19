//! Helpers shared by the unit tests: a temporary game directory with synthetic plugins.

use crate::plugin::{FLAG_ESM, synthetic_plugin};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// A directory under the system temporary directory, removed on drop.
pub struct TempDir(pub PathBuf);

impl TempDir {
    pub fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "loot-rs-unit-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).expect("create temp dir");
        Self(path)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    /// Write a file (creating parent directories).
    pub fn file(&self, relative: &str, bytes: &[u8]) -> PathBuf {
        let path = self.0.join(relative);
        fs::create_dir_all(path.parent().expect("parent")).expect("create dirs");
        fs::write(&path, bytes).expect("write");
        path
    }

    /// Whether this directory's file system tells names apart by case (Linux's usually does; macOS's APFS and
    /// Windows's NTFS usually do not, so two names that differ only in case are one file there).
    pub fn case_sensitive(&self) -> bool {
        let probe = self.file("case-probe", b"");
        let other = self.0.join("CASE-PROBE");
        let sensitive = !other.exists();
        let _ = fs::remove_file(probe);
        sensitive
    }

    /// Write a synthetic plugin; `.esm` names get the master flag.
    pub fn plugin(&self, name: &str, masters: &[&str]) -> PathBuf {
        let flags = u32::from(name.to_lowercase().ends_with(".esm")) * FLAG_ESM;
        self.file(name, &synthetic_plugin(flags, masters, None))
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
