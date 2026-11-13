//! The command line: argument parsing and the `check` command. `main` only forwards here, so everything
//! is testable without spawning the binary.

use crate::check::{Options, run};
use crate::env::{FsEnv, parse_plugins_txt};
use crate::{metadata, render};
use std::fs;
use std::io::Write;
use std::path::PathBuf;

/// Exit code: no hard violation.
pub const EXIT_OK: u8 = 0;
/// Exit code: a hard violation (unmet `req`, present `inc`, load-after violated; group violations with
/// `--strict-groups`).
pub const EXIT_VIOLATION: u8 = 1;
/// Exit code: bad arguments or unreadable input.
pub const EXIT_ERROR: u8 = 2;

/// The text printed after an argument error.
pub const USAGE: &str = "usage: loot-rs check --masterlist <file> --plugins <plugins.txt> --data <Data dir> [--strict-groups] [--json]

Reports the messages that apply, unmet req/inc entries and violated after/group constraints.
It does not sort and does not write anything.

exit codes: 0 no hard violation, 1 a hard violation (unmet req, present inc, load-after
violated; group violations too with --strict-groups), 2 bad arguments or unreadable input";

/// The arguments of `check`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Args {
    /// The masterlist YAML file.
    pub masterlist: PathBuf,
    /// The `plugins.txt` that gives the load order.
    pub plugins: PathBuf,
    /// The game's `Data` directory.
    pub data: PathBuf,
    /// Switches for the check.
    pub options: Options,
    /// Print the report as JSON (see [`render::render_json`]) instead of text.
    pub json: bool,
}

/// Parse the arguments that follow the program name.
///
/// # Errors
/// A message naming the problem: unknown command or flag, a missing value or a missing required flag.
pub fn parse_args(args: &[String]) -> Result<Args, String> {
    let mut args = args.iter();
    match args.next().map(String::as_str) {
        Some("check") => {}
        Some(other) => return Err(format!("unknown command {other:?}")),
        None => return Err("missing command".into()),
    }
    let (mut masterlist, mut plugins, mut data) = (None, None, None);
    let mut options = Options::default();
    let mut json = false;
    while let Some(flag) = args.next() {
        let mut value = |name: &str| {
            args.next()
                .map(PathBuf::from)
                .ok_or_else(|| format!("{name} needs a value"))
        };
        match flag.as_str() {
            "--masterlist" => masterlist = Some(value("--masterlist")?),
            "--plugins" => plugins = Some(value("--plugins")?),
            "--data" => data = Some(value("--data")?),
            "--strict-groups" => options.strict_groups = true,
            "--json" => json = true,
            other => return Err(format!("unknown argument {other:?}")),
        }
    }
    Ok(Args {
        masterlist: masterlist.ok_or("--masterlist is required")?,
        plugins: plugins.ok_or("--plugins is required")?,
        data: data.ok_or("--data is required")?,
        options,
        json,
    })
}

/// Read the inputs, run the check and write the report to `out`.
///
/// # Errors
/// A message for an unreadable or invalid input; the caller prints it and exits with [`EXIT_ERROR`].
pub fn execute(args: &Args, out: &mut dyn Write) -> Result<u8, String> {
    let yaml = fs::read_to_string(&args.masterlist)
        .map_err(|error| format!("{}: {error}", args.masterlist.display()))?;
    let metadata = metadata::parse(&yaml).map_err(|error| format!("masterlist: {error}"))?;
    let list =
        fs::read(&args.plugins).map_err(|error| format!("{}: {error}", args.plugins.display()))?;
    if !args.data.is_dir() {
        return Err(format!("{}: not a directory", args.data.display()));
    }
    let slots = parse_plugins_txt(&String::from_utf8_lossy(&list));
    let report = run(&metadata, &FsEnv::new(&args.data, slots), args.options);
    let text = if args.json {
        render::render_json(&report)
    } else {
        render::render(&report)
    };
    out.write_all(text.as_bytes())
        .map_err(|error| format!("cannot write the report: {error}"))?;
    Ok(if report.hard_count() > 0 {
        EXIT_VIOLATION
    } else {
        EXIT_OK
    })
}

/// Run the program on `args` (without the program name): the report goes to `out`, problems to `err`.
/// Returns the exit code.
pub fn main_with(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> u8 {
    let parsed = match parse_args(args) {
        Ok(parsed) => parsed,
        Err(error) => {
            let _ = writeln!(err, "loot-rs: {error}\n\n{USAGE}");
            return EXIT_ERROR;
        }
    };
    execute(&parsed, out).unwrap_or_else(|error| {
        let _ = writeln!(err, "loot-rs: {error}");
        EXIT_ERROR
    })
}

#[cfg(test)]
#[path = "cli_tests.rs"]
mod tests;
