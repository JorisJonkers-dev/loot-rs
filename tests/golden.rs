//! Golden tests of the command line output.
//!
//! Every directory under `tests/golden/` is one case:
//!
//! * `masterlist.yaml`, `plugins.txt`: the inputs;
//! * `data.txt`: the `Data` directory to build, one line each: `plugin NAME [esm] [masters=A,B]` writes a
//!   synthetic plugin, `file PATH text=TEXT` writes a file;
//! * `args` (optional): extra command line flags;
//! * `expected.stdout`, `expected.code`: what the binary must print and exit with. stderr must be empty.
//!
//! Run with `UPDATE_GOLDEN=1` to rewrite the expected files, then read the diff: a golden file is a claim
//! about LOOT's rules, so a change is reviewed, not accepted.

mod common;

use common::Fixture;
use loot_rs::plugin::{FLAG_ESM, synthetic_plugin};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn cases() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden");
    let mut dirs: Vec<PathBuf> = fs::read_dir(root)
        .expect("tests/golden exists")
        .map(|entry| entry.expect("entry").path())
        .filter(|path| path.is_dir())
        .collect();
    dirs.sort();
    dirs
}

fn build_data(fixture: &Fixture, spec: &str) {
    for line in spec.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let mut words = line.split_whitespace();
        match words.next() {
            Some("plugin") => {
                let name = words.next().expect("plugin name");
                let (mut flags, mut masters) = (0, Vec::new());
                for word in words {
                    if word == "esm" {
                        flags |= FLAG_ESM;
                    } else if let Some(list) = word.strip_prefix("masters=") {
                        masters = list.split(',').collect();
                    } else {
                        panic!("unknown plugin option {word:?} in {line:?}");
                    }
                }
                fixture.file(name, &synthetic_plugin(flags, &masters, None));
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

/// Run one case; returns (stdout, stderr, exit code).
fn run_case(dir: &Path) -> (String, String, i32) {
    let read = |name: &str| fs::read_to_string(dir.join(name)).unwrap_or_default();
    let fixture = Fixture::new();
    build_data(&fixture, &read("data.txt"));
    let masterlist = fixture.masterlist(&read("masterlist.yaml"));
    let plugins = fixture.plugins_txt(&read("plugins.txt"));
    let data = fixture.data();
    let mut command = Command::new(env!("CARGO_BIN_EXE_loot-rs"));
    command
        .arg("check")
        .args(["--masterlist", masterlist.to_str().expect("utf8")])
        .args(["--plugins", plugins.to_str().expect("utf8")])
        .args(["--data", data.to_str().expect("utf8")])
        .args(read("args").split_whitespace());
    let output = command.output().expect("run loot-rs");
    (
        String::from_utf8(output.stdout).expect("utf8"),
        String::from_utf8(output.stderr).expect("utf8"),
        output.status.code().expect("exit code"),
    )
}

#[test]
fn every_case_prints_what_its_golden_file_says() {
    let update = std::env::var_os("UPDATE_GOLDEN").is_some();
    let mut failures = Vec::new();
    for dir in cases() {
        let name = dir
            .file_name()
            .expect("name")
            .to_string_lossy()
            .into_owned();
        let (stdout, stderr, code) = run_case(&dir);
        if update {
            fs::write(dir.join("expected.stdout"), &stdout).expect("write");
            fs::write(dir.join("expected.code"), format!("{code}\n")).expect("write");
            continue;
        }
        let want_out = fs::read_to_string(dir.join("expected.stdout")).unwrap_or_default();
        let want_code: i32 = fs::read_to_string(dir.join("expected.code"))
            .unwrap_or_default()
            .trim()
            .parse()
            .unwrap_or(-1);
        if stdout != want_out || code != want_code || !stderr.is_empty() {
            failures.push(format!(
                "case {name}: exit {code} (want {want_code}), stderr {stderr:?}\n--- want\n{want_out}--- got\n{stdout}"
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn the_cases_cover_every_rule_kind_and_both_check_exit_codes() {
    let all: Vec<(String, String, i32)> = cases()
        .iter()
        .map(|dir| {
            let name = dir
                .file_name()
                .expect("name")
                .to_string_lossy()
                .into_owned();
            let out = fs::read_to_string(dir.join("expected.stdout")).expect("golden stdout");
            let code = fs::read_to_string(dir.join("expected.code"))
                .expect("golden code")
                .trim()
                .parse()
                .expect("code");
            (name, out, code)
        })
        .collect();
    assert!(all.len() >= 20, "{} cases", all.len());
    let seen = |needle: &str| all.iter().any(|(_, out, _)| out.contains(needle));
    for needle in [
        "missing requirement",
        "incompatible",
        "load after",
        "group order",
        "undefined group",
        "tags: +",
        "[say]",
        "[warn]",
        "[error]",
        "unsupported condition function",
        "not in the Data directory",
        ", inactive)",
        "Global messages",
    ] {
        assert!(seen(needle), "no golden case prints {needle:?}");
    }
    assert!(all.iter().any(|(_, _, code)| *code == 0));
    assert!(all.iter().any(|(_, _, code)| *code == 1));
    assert!(
        all.iter().all(|(_, _, code)| *code == 0 || *code == 1),
        "exit code 2 is covered by cli_errors.rs"
    );
    // A rule kind that is not violated must stay quiet.
    let quiet = |case: &str| {
        all.iter()
            .find(|(name, ..)| name == case)
            .expect("case")
            .1
            .clone()
    };
    for case in [
        "req_satisfied",
        "inc_absent",
        "after_satisfied",
        "group_ok",
        "dirty_clean",
    ] {
        let out = quiet(case);
        assert!(!out.contains("VIOLATION"), "{case}: {out}");
        assert!(out.contains("Hard violations: 0."), "{case}: {out}");
    }
}
