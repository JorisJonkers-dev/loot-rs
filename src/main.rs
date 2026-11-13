//! `loot-fnv check`: report which masterlist rules apply to a load order. Read-only.

use loot_fnv::check::{Options, run};
use loot_fnv::env::{FsEnv, parse_plugins_txt};
use loot_fnv::{metadata, render};
use std::path::PathBuf;
use std::process::ExitCode;
use std::{env, fs};

const USAGE: &str = "usage: loot-fnv check --masterlist <file> --plugins <plugins.txt> --data <Data dir> [--strict-groups]

Reports the messages that apply, unmet req/inc entries and violated after/group constraints.
It does not sort and does not write anything.

exit codes: 0 no hard violation, 1 a hard violation (unmet req, present inc, load-after
violated; group violations too with --strict-groups), 2 bad arguments or unreadable input";

struct Args {
    masterlist: PathBuf,
    plugins: PathBuf,
    data: PathBuf,
    options: Options,
}

fn parse_args(args: &[String]) -> Result<Args, String> {
    let mut args = args.iter();
    match args.next().map(String::as_str) {
        Some("check") => {}
        Some(other) => return Err(format!("unknown command {other:?}")),
        None => return Err("missing command".into()),
    }
    let (mut masterlist, mut plugins, mut data) = (None, None, None);
    let mut options = Options::default();
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
            other => return Err(format!("unknown argument {other:?}")),
        }
    }
    Ok(Args {
        masterlist: masterlist.ok_or("--masterlist is required")?,
        plugins: plugins.ok_or("--plugins is required")?,
        data: data.ok_or("--data is required")?,
        options,
    })
}

fn execute(args: &Args) -> Result<ExitCode, String> {
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
    print!("{}", render::render(&report));
    Ok(if report.hard_count() > 0 {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    })
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let parsed = match parse_args(&args) {
        Ok(parsed) => parsed,
        Err(error) => {
            eprintln!("loot-fnv: {error}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    execute(&parsed).unwrap_or_else(|error| {
        eprintln!("loot-fnv: {error}");
        ExitCode::from(2)
    })
}
