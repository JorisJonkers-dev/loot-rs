//! Golden tests of `loot-rs sort`. Same layout as `tests/golden/`: every directory under `tests/golden_sort/` is a
//! case with `masterlist.yaml`, `plugins.txt`, `data.txt`, optional `args`, `expected.stdout`, `expected.code` and
//! `expected.json` (what `--json` prints). `UPDATE_GOLDEN=1` rewrites the expected files; read the diff.
//!
//! The expected order is what the port of libloot's algorithm gives (`docs/COMPATIBILITY.md`); the graph code
//! itself is tested against libloot's own unit tests in `src/sort/plugins_tests.rs`. `scripts/oracle/` compares
//! these cases with libloot when libloot is available.

mod common;

use common::{Fixture, build_data};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn cases() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden_sort");
    let mut dirs: Vec<PathBuf> = fs::read_dir(root)
        .expect("tests/golden_sort exists")
        .map(|entry| entry.expect("entry").path())
        .filter(|path| path.is_dir())
        .collect();
    dirs.sort();
    dirs
}

fn run_case(dir: &Path, json: bool) -> (String, String, i32) {
    let read = |name: &str| fs::read_to_string(dir.join(name)).unwrap_or_default();
    let fixture = Fixture::new();
    build_data(&fixture, &read("data.txt"));
    let masterlist = fixture.masterlist(&read("masterlist.yaml"));
    let plugins = fixture.plugins_txt(&read("plugins.txt"));
    let data = fixture.data();
    let mut command = Command::new(env!("CARGO_BIN_EXE_loot-rs"));
    command
        .arg("sort")
        .args(["--masterlist", masterlist.to_str().expect("utf8")])
        .args(["--plugins", plugins.to_str().expect("utf8")])
        .args(["--data", data.to_str().expect("utf8")])
        .args(read("args").split_whitespace());
    if json {
        command.arg("--json");
    }
    let output = command.output().expect("run loot-rs");
    (
        String::from_utf8(output.stdout).expect("utf8"),
        String::from_utf8(output.stderr).expect("utf8"),
        output.status.code().expect("exit code"),
    )
}

#[test]
fn every_case_prints_what_its_golden_files_say() {
    let update = std::env::var_os("UPDATE_GOLDEN").is_some();
    let mut failures = Vec::new();
    for dir in cases() {
        let name = dir
            .file_name()
            .expect("name")
            .to_string_lossy()
            .into_owned();
        let (stdout, stderr, code) = run_case(&dir, false);
        let (json, json_err, json_code) = run_case(&dir, true);
        if update {
            fs::write(dir.join("expected.stdout"), &stdout).expect("write");
            fs::write(dir.join("expected.json"), &json).expect("write");
            fs::write(dir.join("expected.code"), format!("{code}\n")).expect("write");
            continue;
        }
        let want = |file: &str| fs::read_to_string(dir.join(file)).unwrap_or_default();
        let want_code: i32 = want("expected.code").trim().parse().unwrap_or(-1);
        if stdout != want("expected.stdout") || code != want_code || !stderr.is_empty() {
            failures.push(format!(
                "case {name}: exit {code} (want {want_code}), stderr {stderr:?}\n--- want\n{}--- got\n{stdout}",
                want("expected.stdout")
            ));
        }
        if json != want("expected.json") || json_code != code || !json_err.is_empty() {
            failures.push(format!(
                "case {name} (--json): exit {json_code}\n--- want\n{}--- got\n{json}",
                want("expected.json")
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn the_cases_cover_every_rule_source_and_both_exit_codes() {
    let json: String = cases()
        .iter()
        .map(|dir| fs::read_to_string(dir.join("expected.json")).expect("golden json"))
        .collect();
    for needle in [
        "\"source\":\"header-master\"",
        "\"source\":\"after\"",
        "\"source\":\"req\"",
        "\"source\":\"group\"",
        "\"dropped_group_rules\":[{",
        "\"status\":\"dirty\"",
        "\"not_installed\":[\"",
        "\"tags_add\":[\"",
    ] {
        assert!(json.contains(needle), "no case shows {needle}");
    }
    let codes: Vec<String> = cases()
        .iter()
        .map(|dir| fs::read_to_string(dir.join("expected.code")).expect("code"))
        .collect();
    assert!(codes.iter().any(|code| code.trim() == "0"));
    assert!(codes.iter().any(|code| code.trim() == "1"));
}
