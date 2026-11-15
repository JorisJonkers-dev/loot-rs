//! Benchmark of `api::sort` on synthetic load orders of realistic size (no game data).
//!
//! ```text
//! cargo run --release --example bench -- generate <plugins> <dir>
//! cargo run --release --example bench -- run <dir> [--runs N] [--threads N] [--cache] [--order-out FILE]
//! ```
//!
//! `generate` writes `<dir>/Data` with one base master of 400,000 records, three DLC-sized masters of 40,000 records,
//! and `<plugins> - 4` mods of 50 to 20,000 records each (a third of them overriding base records, so the overlap
//! edges are exercised), BSA archives for one mod in four (100 to 20,000 files), `<dir>/masterlist.yaml` with groups,
//! `after`, `req`, messages, tags, cleaning data and conditions for about a third of the plugins, and
//! `<dir>/plugins.txt`. Everything comes from a fixed seed, so two directories of the same size are the same.
//!
//! `run` sorts that load order `--runs` times (default 5) and prints the median and minimum wall time of `api::sort`
//! and the process's peak resident memory; `--cache` sorts with one `SortCache` (the first run fills it),
//! `--order-out` writes the sorted order, one name per line, to compare with another tool's. Generating and running
//! are separate processes so that the peak memory is the sort's (`task bench` does both).

use loot_rs::api::{Masterlist, PluginSlot, SortCache, SortOptions, sort};
use loot_rs::archives::synthetic_bsa;
use loot_rs::records::synthetic_records;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const BASE: &str = "FalloutNV.esm";
const DLCS: [&str; 3] = ["DeadMoney.esm", "HonestHearts.esm", "OldWorldBlues.esm"];
const BASE_RECORDS: u32 = 400_000;
const DLC_RECORDS: u32 = 40_000;

/// A small deterministic generator (`SplitMix64`), so the bench needs no dependency and every run is the same.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, bound: u32) -> u32 {
        u32::try_from(self.next() % u64::from(bound.max(1))).unwrap_or(0)
    }

    /// Roughly log-uniform between `low` and `high`: most mods are small, a few are large.
    fn log_between(&mut self, low: u32, high: u32) -> u32 {
        let span = f64::from(high) / f64::from(low);
        let fraction = f64::from(self.below(1_000_000)) / 1_000_000.0;
        let value = f64::from(low) * span.powf(fraction);
        // Truncation is the intent: a whole number of records or files.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let value = value as u32;
        value.clamp(low, high)
    }
}

enum Command {
    /// Write `Data/`, `masterlist.yaml` and `plugins.txt` for this many plugins into the directory.
    Generate { plugins: usize, dir: PathBuf },
    /// Sort the directory's load order this many times.
    Run {
        dir: PathBuf,
        runs: usize,
        order_out: Option<PathBuf>,
        cache: bool,
        threads: usize,
    },
}

const USAGE: &str = "usage: bench generate <plugins> <dir> | bench run <dir> [--runs N] [--threads N] [--cache] [--order-out FILE]";

fn parse_args() -> Result<Command, String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("generate") if args.len() == 3 => {
            let plugins: usize = args[1]
                .parse()
                .map_err(|_| format!("not a plugin count: {}", args[1]))?;
            if plugins < DLCS.len() + 2 {
                return Err(format!("at least {} plugins", DLCS.len() + 2));
            }
            Ok(Command::Generate {
                plugins,
                dir: args[2].clone().into(),
            })
        }
        Some("run") if args.len() >= 2 => {
            let mut runs = 5;
            let mut order_out = None;
            let mut cache = false;
            let mut threads = 0;
            let mut rest = args[2..].iter();
            while let Some(arg) = rest.next() {
                match arg.as_str() {
                    "--runs" => {
                        runs = rest
                            .next()
                            .and_then(|v| v.parse().ok())
                            .ok_or("--runs takes a number")?;
                    }
                    "--cache" => cache = true,
                    "--threads" => {
                        threads = rest
                            .next()
                            .and_then(|v| v.parse().ok())
                            .ok_or("--threads takes a number")?;
                    }
                    "--order-out" => {
                        order_out = Some(rest.next().ok_or("--order-out takes a file")?.into());
                    }
                    other => return Err(format!("unknown argument {other}")),
                }
            }
            Ok(Command::Run {
                dir: args[1].clone().into(),
                runs,
                order_out,
                cache,
                threads,
            })
        }
        _ => Err(USAGE.to_string()),
    }
}

/// The records of one plugin: `(form id, data size)` with the master byte set for overrides.
fn records(rng: &mut Rng, own: u32, overrides: &[(u8, u32)], new_records: u32) -> Vec<(u32, u32)> {
    let mut list = Vec::with_capacity(overrides.len() + new_records as usize);
    for &(master, object) in overrides {
        list.push((u32::from(master) << 24 | object, 40 + rng.below(200)));
    }
    for object in 1..=new_records {
        list.push((own << 24 | object, 40 + rng.below(200)));
    }
    list
}

fn write(path: &Path, bytes: &[u8]) {
    std::fs::write(path, bytes).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
}

fn mod_name(index: usize) -> String {
    format!("Mod {index:04}.esp")
}

/// Write the plugins and archives of the load order; returns the load order.
fn generate_data(data: &Path, plugins: usize, rng: &mut Rng) -> Vec<PluginSlot> {
    let master_flag = 1;
    write(
        &data.join(BASE),
        &synthetic_records(
            master_flag,
            &[],
            &[(*b"NPC_", &records(rng, 0, &[], BASE_RECORDS))],
        ),
    );
    let mut order = vec![PluginSlot::new(BASE, true)];
    for dlc in DLCS {
        let overrides: Vec<(u8, u32)> = (0..2_000)
            .map(|_| (0, 1 + rng.below(BASE_RECORDS)))
            .collect();
        let list = records(rng, 1, &overrides, DLC_RECORDS);
        write(
            &data.join(dlc),
            &synthetic_records(master_flag, &[BASE], &[(*b"NPC_", &list)]),
        );
        order.push(PluginSlot::new(dlc, true));
    }
    for index in 0..plugins - 1 - DLCS.len() {
        let name = mod_name(index);
        let size = rng.log_between(50, 20_000);
        // A third of the mods change base records; those that do change a few hundred of a hot range, so that
        // many pairs overlap.
        let overrides: Vec<(u8, u32)> = if index % 3 == 0 {
            (0..size / 4).map(|_| (0, 1 + rng.below(20_000))).collect()
        } else {
            Vec::new()
        };
        let list = records(rng, 1, &overrides, size);
        write(
            &data.join(&name),
            &synthetic_records(0, &[BASE], &[(*b"NPC_", &list)]),
        );
        if index % 4 == 0 {
            let files = rng.log_between(100, 20_000);
            let names: Vec<String> = (0..files)
                .map(|file| format!("{}{file}.nif", if file % 2 == 0 { "shared" } else { "own" }))
                .collect();
            let refs: Vec<&str> = names.iter().map(String::as_str).collect();
            let folder = format!("meshes\\mod{}", index % 16);
            let stem = name.trim_end_matches(".esp");
            write(
                &data.join(format!("{stem}.bsa")),
                &synthetic_bsa(104, &[(folder.as_str(), &refs)]),
            );
        }
        order.push(PluginSlot::new(name, index % 10 != 9));
    }
    order
}

/// A masterlist with groups and entries for about a third of the plugins, with conditions.
fn masterlist(plugins: usize, rng: &mut Rng) -> String {
    let mut text = String::from("prelude:\n  common:\n    - &dirty\n      util: 'FNVEdit v4.0'\n");
    text.push_str("groups:\n  - name: default\n");
    for group in 0..12 {
        let _ = writeln!(
            text,
            "  - name: g{group}\n    after: [{}]",
            if group == 0 {
                "default".to_string()
            } else {
                format!("g{}", group - 1)
            }
        );
    }
    text.push_str("globals:\n  - type: say\n    content: 'Read the guide.'\n    condition: 'file(\"FalloutNV.esm\")'\n");
    text.push_str("plugins:\n");
    let _ = writeln!(text, "  - name: '{BASE}'\n    tag: [Delev, Relev]");
    let mods = plugins - 1 - DLCS.len();
    for index in (0..mods).filter(|index| index % 3 == 1) {
        let name = mod_name(index);
        let _ = writeln!(text, "  - name: '{name}'\n    group: g{}", rng.below(12));
        if index > 10 {
            let _ = writeln!(
                text,
                "    after: ['{}']",
                mod_name(index - 1 - rng.below(10) as usize)
            );
        }
        if index % 7 == 1 {
            let _ = writeln!(
                text,
                "    req: ['{BASE}']\n    inc: ['Missing {index}.esp']"
            );
        }
        let _ = writeln!(
            text,
            "    msg:\n      - type: warn\n        content: 'Needs a patch.'\n        condition: 'active(\"{}\") and not file(\"Patch {index}.esp\")'",
            mod_name(index.saturating_sub(1))
        );
        let _ = writeln!(
            text,
            "    tag:\n      - name: Names\n        condition: 'many(\"Mod 0.*\\.esp\")'"
        );
        let _ = writeln!(
            text,
            "    dirty:\n      - <<: *dirty\n        crc: 0x{:08X}\n        itm: 3",
            rng.next() & 0xFFFF_FFFF
        );
    }
    let _ = writeln!(
        text,
        "  - name: 'Mod 9.*\\.esp'\n    msg:\n      - type: say\n        content: 'A regex entry.'"
    );
    text
}

fn peak_memory_kib() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    status
        .lines()
        .find_map(|line| line.strip_prefix("VmHWM:"))
        .and_then(|value| value.trim().trim_end_matches("kB").trim().parse().ok())
}

fn generate(plugins: usize, dir: &Path) {
    let data = dir.join("Data");
    std::fs::create_dir_all(&data).expect("create the data directory");
    let mut rng = Rng(0x5EED);
    let order = generate_data(&data, plugins, &mut rng);
    write(
        &dir.join("masterlist.yaml"),
        masterlist(plugins, &mut rng).as_bytes(),
    );
    let mut list = String::new();
    for slot in &order {
        let _ = writeln!(list, "{}{}", if slot.active { "*" } else { "" }, slot.name);
    }
    write(&dir.join("plugins.txt"), list.as_bytes());
}

fn run(dir: &Path, runs: usize, order_out: Option<&Path>, cache: bool, threads: usize) {
    let text = std::fs::read_to_string(dir.join("plugins.txt")).expect("plugins.txt");
    let order: Vec<PluginSlot> = text
        .lines()
        .map(|line| match line.strip_prefix('*') {
            Some(name) => PluginSlot::new(name, true),
            None => PluginSlot::new(line, false),
        })
        .collect();
    let data = dir.join("Data");
    let started = Instant::now();
    let masterlist =
        Masterlist::from_file(&dir.join("masterlist.yaml")).expect("the masterlist parses");
    let parse = started.elapsed();
    let mut options = SortOptions::default().with_threads(threads);
    if cache {
        options = options.with_cache(SortCache::new());
    }
    let mut times: Vec<Duration> = Vec::with_capacity(runs);
    let mut last = Vec::new();
    for _ in 0..runs.max(1) {
        let started = Instant::now();
        let output = sort(&order, &masterlist, &data, &options).expect("the load order sorts");
        times.push(started.elapsed());
        last = output.order;
    }
    times.sort();
    let median = times[times.len() / 2];
    println!(
        "loot-rs plugins={} cache={cache} runs={} masterlist_ms={:.1} sort_median_ms={:.1} sort_min_ms={:.1} peak_rss_mib={} threads={}",
        order.len(),
        times.len(),
        parse.as_secs_f64() * 1000.0,
        median.as_secs_f64() * 1000.0,
        times[0].as_secs_f64() * 1000.0,
        peak_memory_kib().map_or_else(|| "?".to_string(), |kib| (kib / 1024).to_string()),
        loot_rs::parallel::threads(threads),
    );
    if let Some(path) = order_out {
        write(path, (last.join("\n") + "\n").as_bytes());
    }
}

fn main() {
    match parse_args() {
        Ok(Command::Generate { plugins, dir }) => generate(plugins, &dir),
        Ok(Command::Run {
            dir,
            runs,
            order_out,
            cache,
            threads,
        }) => run(&dir, runs, order_out.as_deref(), cache, threads),
        Err(text) => {
            eprintln!("{text}");
            std::process::exit(2);
        }
    }
}
