// SPDX-License-Identifier: GPL-3.0-only
//! `esplugin-diff <Data dir>`: reads plugin file names (one per line) from standard input, compares each plugin and
//! each pair with esplugin, and prints one line of counts: `{"n_plugins":N,"n_header_differences":H,
//! "n_record_differences":R}`. No name, path or text is printed. Set `ESPLUGIN_DIFF_DETAIL=<file>` to write the
//! differences (with names) to that file, for the owner to read (`oracle_run.py` sets it under L2).
//! Exit status: 0 no difference, 1 differences, 2 usage or I/O error.

use std::io::{BufRead, Write};
use std::path::PathBuf;

fn main() {
    let Some(data) = std::env::args_os().nth(1).map(PathBuf::from) else {
        eprintln!("usage: esplugin-diff <Data dir>   (plugin names on standard input)");
        std::process::exit(2);
    };
    let names: Vec<String> = std::io::stdin()
        .lock()
        .lines()
        .map_while(Result::ok)
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty())
        .collect();
    let (count, found) = esplugin_diff::compare_all(&data, &names);
    if let Some(path) = std::env::var_os("ESPLUGIN_DIFF_DETAIL") {
        let mut text = String::new();
        for line in found.header.iter().chain(&found.records) {
            text.push_str(line);
            text.push('\n');
        }
        if std::fs::File::create(path)
            .and_then(|mut file| file.write_all(text.as_bytes()))
            .is_err()
        {
            eprintln!("esplugin-diff: cannot write the detail file");
            std::process::exit(2);
        }
    }
    println!(
        "{{\"n_plugins\":{count},\"n_header_differences\":{},\"n_record_differences\":{}}}",
        found.header.len(),
        found.records.len()
    );
    std::process::exit(i32::from(!found.is_empty()));
}
