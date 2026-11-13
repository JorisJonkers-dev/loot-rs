//! Fixtures: a throwaway game directory with synthetic plugins, written by the test.

#![allow(dead_code)]

pub mod model;

use loot_rs::plugin::{FLAG_ESM, synthetic_plugin};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// A temporary directory that is removed when dropped.
pub struct Fixture {
    pub root: PathBuf,
}

impl Fixture {
    pub fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "loot-rs-test-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(root.join("Data")).expect("create fixture");
        Self { root }
    }

    pub fn data(&self) -> PathBuf {
        self.root.join("Data")
    }

    /// Write a synthetic plugin into `Data`; names ending in `.esm` get the master flag.
    pub fn plugin(&self, name: &str, masters: &[&str]) -> &Self {
        let flags = if name.to_lowercase().ends_with(".esm") {
            FLAG_ESM
        } else {
            0
        };
        self.file(name, &synthetic_plugin(flags, masters, None))
    }

    /// Write any file under `Data` (parent directories are created).
    pub fn file(&self, relative: &str, bytes: &[u8]) -> &Self {
        let path = self.data().join(relative);
        fs::create_dir_all(path.parent().expect("has parent")).expect("create dirs");
        fs::write(path, bytes).expect("write file");
        self
    }

    pub fn plugins_txt(&self, text: &str) -> PathBuf {
        let path = self.root.join("plugins.txt");
        fs::write(&path, text).expect("write plugins.txt");
        path
    }

    pub fn masterlist(&self, yaml: &str) -> PathBuf {
        let path = self.root.join("masterlist.yaml");
        fs::write(&path, yaml).expect("write masterlist");
        path
    }

    /// Every file under the root with its contents, for "nothing was written" checks.
    pub fn snapshot(&self) -> Vec<(PathBuf, Vec<u8>)> {
        fn walk(dir: &Path, out: &mut Vec<(PathBuf, Vec<u8>)>) {
            let mut entries: Vec<_> = fs::read_dir(dir)
                .expect("read dir")
                .map(|entry| entry.expect("entry").path())
                .collect();
            entries.sort();
            for path in entries {
                if path.is_dir() {
                    walk(&path, out);
                } else {
                    let bytes = fs::read(&path).expect("read");
                    out.push((path, bytes));
                }
            }
        }
        let mut out = Vec::new();
        walk(&self.root, &mut out);
        out
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

/// Property-test settings: a fixed seed, so every run generates the same cases and a failure is
/// reproducible. Set `LOOT_RS_PROPTEST_SEED` to a number to explore other cases locally.
pub fn proptest_config(cases: u32) -> proptest::test_runner::Config {
    let seed = std::env::var("LOOT_RS_PROPTEST_SEED")
        .ok()
        .and_then(|text| text.parse::<u64>().ok())
        .unwrap_or(0x100_7F00D);
    let cases = std::env::var("PROPTEST_CASES")
        .ok()
        .and_then(|text| text.parse::<u32>().ok())
        .unwrap_or(cases);
    proptest::test_runner::Config {
        cases,
        rng_seed: proptest::test_runner::RngSeed::Fixed(seed),
        ..proptest::test_runner::Config::default()
    }
}
