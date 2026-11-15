//! The command line: argument parsing and the `check` and `sort` commands. `main` only forwards here, so everything
//! is testable without spawning the binary.

use crate::api::{Masterlist, PluginSlot, SortMode, SortOptions, Userlist, sort_with};
use crate::check::{Options, run};
use crate::env::{FsEnv, Slot, parse_plugins_txt};
use crate::{metadata, render};
use std::fs::File;
use std::io::{Read, Write};
use std::path::PathBuf;

/// Exit code: no hard violation.
pub const EXIT_OK: u8 = 0;
/// Exit code: a hard violation (unmet `req`, present `inc`, load-after violated; group violations with
/// `--strict-groups`).
pub const EXIT_VIOLATION: u8 = 1;
/// Exit code: bad arguments or unreadable input.
pub const EXIT_ERROR: u8 = 2;

/// Largest masterlist file that is read, in bytes.
pub const MAX_MASTERLIST_FILE: u64 = metadata::MAX_MASTERLIST_BYTES as u64;

/// Largest `plugins.txt` that is read, in bytes (a load order of 255 plugins is about 5 KB).
pub const MAX_PLUGINS_FILE: u64 = 1024 * 1024;

/// The text printed after an argument error.
pub const USAGE: &str = "usage: loot-rs check|sort --masterlist <file> [--prelude <file>] [--userlist <file>] --plugins <plugins.txt> --data <Data dir> [--game-dir <dir>] [--extra-data <dir>]... [--strict-groups] [--mode full|minimal] [--json]
       loot-rs contracts --json

check: reports the messages that apply, unmet req/inc entries and violated after/group constraints.
sort: prints the load order sorted with LOOT's algorithm (a port of libloot's plugin graph)
with the messages, tags and findings that apply to it. --mode minimal instead keeps the given order and moves
only the plugins that a hard constraint (master flag, header master, req, after) forces, and says why.
contracts --json: prints the JSON contracts this build emits and their schema versions.
Neither check nor sort writes anything.

exit codes: 0 no hard violation, 1 a hard violation (unmet req, present inc, load-after
violated; group violations too with --strict-groups), 2 bad arguments or unreadable input";

/// What to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    /// Report what applies to the load order as given.
    Check,
    /// Sort the load order by the metadata rules and report what applies to the result.
    Sort,
}

/// The arguments of `check` and `sort`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Args {
    /// The command.
    pub command: Command,
    /// The masterlist YAML file.
    pub masterlist: PathBuf,
    /// A prelude file that replaces the masterlist's `prelude:` section (`--prelude`).
    pub prelude: Option<PathBuf>,
    /// A userlist merged over the masterlist (`--userlist`).
    pub userlist: Option<PathBuf>,
    /// The `plugins.txt` that gives the load order.
    pub plugins: PathBuf,
    /// The game's `Data` directory.
    pub data: PathBuf,
    /// The game directory, what `..` in a condition names.
    pub game_dir: Option<PathBuf>,
    /// Further data directories, searched in this order before `data`.
    pub extra_data: Vec<PathBuf>,
    /// Switches for the check.
    pub options: Options,
    /// Print the report as JSON (see [`render::render_json`]) instead of text.
    pub json: bool,
    /// How `sort` orders the plugins (`--mode full|minimal`).
    pub mode: SortMode,
}

/// Parse the arguments that follow the program name.
///
/// # Errors
/// A message naming the problem: unknown command or flag, a missing value or a missing required flag.
pub fn parse_args(args: &[String]) -> Result<Args, String> {
    let mut args = args.iter();
    let command = match args.next().map(String::as_str) {
        Some("check") => Command::Check,
        Some("sort") => Command::Sort,
        Some(other) => return Err(format!("unknown command {other:?}")),
        None => return Err("missing command".into()),
    };
    let (mut masterlist, mut plugins, mut data) = (None, None, None);
    let (mut game_dir, mut extra_data, mut prelude, mut userlist) = (None, Vec::new(), None, None);
    let mut options = Options::default();
    let mut json = false;
    let mut mode = SortMode::Full;
    while let Some(flag) = args.next() {
        let mut value = |name: &str| {
            args.next()
                .map(PathBuf::from)
                .ok_or_else(|| format!("{name} needs a value"))
        };
        match flag.as_str() {
            "--masterlist" => masterlist = Some(value("--masterlist")?),
            "--prelude" => prelude = Some(value("--prelude")?),
            "--userlist" => userlist = Some(value("--userlist")?),
            "--plugins" => plugins = Some(value("--plugins")?),
            "--data" => data = Some(value("--data")?),
            "--game-dir" => game_dir = Some(value("--game-dir")?),
            "--extra-data" => extra_data.push(value("--extra-data")?),
            "--strict-groups" => options.strict_groups = true,
            "--json" => json = true,
            "--mode" => {
                mode = match args.next().map(String::as_str) {
                    Some("full") => SortMode::Full,
                    Some("minimal") => SortMode::MinimalChange,
                    _ => return Err("--mode needs full or minimal".into()),
                };
            }
            other => return Err(format!("unknown argument {other:?}")),
        }
    }
    Ok(Args {
        command,
        masterlist: masterlist.ok_or("--masterlist is required")?,
        prelude,
        userlist,
        plugins: plugins.ok_or("--plugins is required")?,
        data: data.ok_or("--data is required")?,
        game_dir,
        extra_data,
        options,
        json,
        mode,
    })
}

/// Read a file of at most `limit` bytes, never more than `limit + 1` into memory.
fn read_limited(path: &std::path::Path, limit: u64) -> Result<Vec<u8>, String> {
    let describe = |error: std::io::Error| format!("{}: {error}", path.display());
    let file = File::open(path).map_err(describe)?;
    let mut bytes = Vec::new();
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(describe)?;
    if bytes.len() as u64 > limit {
        return Err(format!(
            "{}: larger than the limit of {limit} bytes",
            path.display()
        ));
    }
    Ok(bytes)
}

/// Read the inputs, run the check and write the report to `out`.
///
/// # Errors
/// A message for an unreadable or invalid input; the caller prints it and exits with [`EXIT_ERROR`].
pub fn execute(args: &Args, out: &mut dyn Write) -> Result<u8, String> {
    let read_text = |path: &std::path::Path| {
        String::from_utf8(read_limited(path, MAX_MASTERLIST_FILE)?)
            .map_err(|_| format!("{}: stream did not contain valid UTF-8", path.display()))
    };
    let mut yaml = read_text(&args.masterlist)?;
    if let Some(prelude) = &args.prelude {
        yaml = metadata::replace_prelude(&yaml, &read_text(prelude)?);
    }
    let mut metadata = metadata::parse(&yaml).map_err(|error| format!("masterlist: {error}"))?;
    let user_yaml = match &args.userlist {
        Some(path) => Some(read_text(path)?),
        None => None,
    };
    if let Some(text) = &user_yaml {
        let user = metadata::parse(text).map_err(|error| format!("userlist: {error}"))?;
        metadata = metadata::merge(&user, &metadata);
    }
    let list = read_limited(&args.plugins, MAX_PLUGINS_FILE)?;
    if !args.data.is_dir() {
        return Err(format!("{}: not a directory", args.data.display()));
    }
    let slots = parse_plugins_txt(&String::from_utf8_lossy(&list));
    if args.command == Command::Sort {
        return execute_sort(args, &yaml, user_yaml.as_deref(), &slots, out);
    }
    let env = FsEnv::new(&args.data, slots)
        .with_game_dir(args.game_dir.as_deref())
        .with_additional_data_dirs(&args.extra_data);
    let mut report = run(&metadata, &env, args.options);
    report.metadata_warnings.extend(env.case_warnings());
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

fn execute_sort(
    args: &Args,
    yaml: &str,
    user_yaml: Option<&str>,
    slots: &[Slot],
    out: &mut dyn Write,
) -> Result<u8, String> {
    let masterlist = Masterlist::parse(yaml).map_err(|error| error.to_string())?;
    let userlist = user_yaml
        .map(Userlist::parse)
        .transpose()
        .map_err(|error| format!("userlist: {error}"))?;
    let load_order: Vec<PluginSlot> = slots
        .iter()
        .map(|slot| PluginSlot::new(slot.name.clone(), slot.active))
        .collect();
    let mut options = SortOptions::default()
        .with_strict_groups(args.options.strict_groups)
        .with_mode(args.mode)
        .with_additional_data_dirs(args.extra_data.iter());
    options.game_dir.clone_from(&args.game_dir);
    let result = sort_with(
        &load_order,
        &masterlist,
        userlist.as_ref(),
        &args.data,
        &options,
    )
    .map_err(|error| format!("cannot sort: {error}"))?;
    let text = if args.json {
        render::render_sort_json(&result)
    } else {
        render::render_sort(&result)
    };
    out.write_all(text.as_bytes())
        .map_err(|error| format!("cannot write the result: {error}"))?;
    let hard = result
        .plugins
        .iter()
        .flat_map(|plugin| &plugin.findings)
        .any(|finding| finding.hard);
    Ok(if hard { EXIT_VIOLATION } else { EXIT_OK })
}

/// `contracts --json`: print the contracts this build emits and their schema versions (ADR-0006).
fn contracts(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> u8 {
    if args != ["--json"] {
        let _ = writeln!(
            err,
            "loot-rs: contracts takes exactly one option: --json\n\n{USAGE}"
        );
        return EXIT_ERROR;
    }
    match out.write_all(render::contracts_json().as_bytes()) {
        Ok(()) => EXIT_OK,
        Err(error) => {
            let _ = writeln!(err, "loot-rs: cannot write the contracts: {error}");
            EXIT_ERROR
        }
    }
}

/// Run the program on `args` (without the program name): the report goes to `out`, problems to `err`.
/// Returns the exit code.
pub fn main_with(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> u8 {
    if args.first().map(String::as_str) == Some("contracts") {
        return contracts(&args[1..], out, err);
    }
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
