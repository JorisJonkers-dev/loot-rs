//! `loot-rs check`: report which masterlist rules apply to a load order. Read-only.

use std::io::{stderr, stdout};
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    ExitCode::from(loot_rs::cli::main_with(
        &args,
        &mut stdout().lock(),
        &mut stderr().lock(),
    ))
}
