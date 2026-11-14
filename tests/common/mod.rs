//! Fixtures: a throwaway game directory with synthetic plugins, written by the test.

#![allow(dead_code)]

pub mod model;

use loot_rs::archives::synthetic_bsa;
use loot_rs::plugin::{FLAG_ESM, synthetic_plugin};
use loot_rs::records::synthetic_records;
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

/// Build the `Data` directory a golden case describes (`data.txt`): `plugin NAME [esm] [masters=A,B]` writes a
/// synthetic plugin (`records=HEX,HEX` adds a group with a record of each of those form ids), `file PATH text=TEXT`
/// writes a file, `bsa NAME.bsa folder=file,file;folder=file` writes a version 104 archive that lists those files.
pub fn build_data(fixture: &Fixture, spec: &str) {
    for line in spec.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let mut words = line.split_whitespace();
        match words.next() {
            Some("plugin") => {
                let name = words.next().expect("plugin name");
                let (mut flags, mut masters, mut form_ids) = (0, Vec::new(), Vec::new());
                for word in words {
                    if word == "esm" {
                        flags |= FLAG_ESM;
                    } else if let Some(list) = word.strip_prefix("masters=") {
                        masters = list.split(',').collect();
                    } else if let Some(list) = word.strip_prefix("records=") {
                        form_ids = list
                            .split(',')
                            .map(|id| (u32::from_str_radix(id, 16).expect("hex form id"), 0))
                            .collect();
                    } else {
                        panic!("unknown plugin option {word:?} in {line:?}");
                    }
                }
                let bytes = if form_ids.is_empty() {
                    synthetic_plugin(flags, &masters, None)
                } else {
                    synthetic_records(flags, &masters, &[(*b"NPC_", &form_ids)])
                };
                fixture.file(name, &bytes);
            }
            Some("bsa") => {
                // `bsa NAME.bsa folder=file,file;folder=file`: a version 104 archive with those files.
                let name = words.next().expect("bsa name");
                let spec = words.next().expect("bsa folders");
                let folders: Vec<(&str, Vec<&str>)> = spec
                    .split(';')
                    .map(|folder| {
                        let (name, files) = folder.split_once('=').expect("folder=files");
                        (name, files.split(',').filter(|f| !f.is_empty()).collect())
                    })
                    .collect();
                let refs: Vec<(&str, &[&str])> = folders
                    .iter()
                    .map(|(name, files)| (*name, files.as_slice()))
                    .collect();
                fixture.file(name, &synthetic_bsa(104, &refs));
            }
            Some("file") => {
                let path = words.next().expect("file path");
                let text = words
                    .next()
                    .and_then(|word| word.strip_prefix("text="))
                    .expect("file text=");
                fixture.file(path, text.as_bytes());
            }
            other => panic!("unknown data line {other:?}"),
        }
    }
}
