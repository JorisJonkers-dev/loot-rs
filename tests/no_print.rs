// SPDX-License-Identifier: GPL-3.0-only
//! Library code never prints or ends the process: output goes to the writers a caller passes, and only `main.rs`
//! turns a result into an exit code.

use std::fs;
use std::path::Path;

const FORBIDDEN: [&str; 8] = [
    "println!",
    "print!(",
    "eprintln!",
    "eprint!(",
    "dbg!(",
    "process::exit",
    "process::abort",
    "std::io::stdout",
];

fn sources(dir: &Path, found: &mut Vec<std::path::PathBuf>) {
    for entry in fs::read_dir(dir).expect("read src") {
        let path = entry.expect("entry").path();
        if path.is_dir() {
            sources(&path, found);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            found.push(path);
        }
    }
}

#[test]
fn no_library_source_prints_or_exits() {
    let mut files = Vec::new();
    sources(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut files,
    );
    let mut hits = Vec::new();
    for file in files {
        let name = file
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        if name == "main.rs" || name.ends_with("_tests.rs") {
            continue;
        }
        let text = fs::read_to_string(&file).expect("read source");
        for (number, line) in text.lines().enumerate() {
            let code = line.trim_start();
            if code.starts_with("//") {
                continue;
            }
            for word in FORBIDDEN {
                if code.contains(word) {
                    hits.push(format!("{}:{}: {word}", file.display(), number + 1));
                }
            }
        }
    }
    assert!(
        hits.is_empty(),
        "library code prints or exits:\n{}",
        hits.join("\n")
    );
}
