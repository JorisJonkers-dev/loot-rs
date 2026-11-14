//! Tests against the real LOOT masterlist for Fallout: New Vegas. Ignored by default.
//!
//! The masterlist is community data (CC0-1.0, <https://github.com/loot/falloutnv>). It is never committed:
//! point `LOOT_RS_MASTERLIST` at a copy, for example
//!
//! ```sh
//! curl -sL -o /tmp/masterlist.yaml https://raw.githubusercontent.com/loot/falloutnv/v0.29/masterlist.yaml
//! LOOT_RS_MASTERLIST=/tmp/masterlist.yaml cargo test --release --test real_masterlist -- --ignored --nocapture
//! ```
//!
//! `LOOT_RS_BUDGET_SECS` (default 10) is the time allowed for checking 400 plugins. The weekly workflow
//! `.github/workflows/masterlist.yml` runs this.

mod common;

use common::Fixture;
use loot_rs::check::{Options, run};
use loot_rs::condition::{Arg, ConditionError, Expr, UNSUPPORTED_FUNCTIONS, evaluate, parse};
use loot_rs::env::{FsEnv, parse_plugins_txt};
use loot_rs::metadata::{Metadata, NamePattern, parse as parse_masterlist};
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

/// The two tests parse the whole masterlist: never at the same time.
static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn masterlist_text() -> String {
    let path = std::env::var("LOOT_RS_MASTERLIST")
        .or_else(|_| std::env::var("LOOT_FNV_MASTERLIST"))
        .expect("set LOOT_RS_MASTERLIST to a copy of the falloutnv masterlist.yaml");
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{path}: {error}"))
}

/// Every condition string in the metadata.
fn conditions(metadata: &Metadata) -> Vec<&str> {
    let mut all = Vec::new();
    let messages = metadata
        .globals
        .iter()
        .chain(metadata.plugins.iter().flat_map(|entry| entry.msg.iter()));
    all.extend(messages.filter_map(|message| message.condition.as_deref()));
    for entry in &metadata.plugins {
        all.extend(entry.tag.iter().filter_map(|tag| tag.condition.as_deref()));
        let files = entry.after.iter().chain(&entry.req).chain(&entry.inc);
        for file in files {
            all.extend(file.condition.as_deref());
            all.extend(file.constraint.as_deref());
        }
    }
    all
}

fn functions(expr: &Expr, found: &mut Vec<String>) {
    match expr {
        Expr::Call(name, _) => found.push(name.clone()),
        Expr::Not(inner) => functions(inner, found),
        Expr::And(left, right) | Expr::Or(left, right) => {
            functions(left, found);
            functions(right, found);
        }
    }
}

/// The calls of `version()` whose first argument names a plugin.
fn plugin_version_calls(expr: &Expr, found: &mut Vec<Expr>) {
    match expr {
        Expr::Call(name, args) => {
            let on_plugin = matches!(args.first(), Some(Arg::Str(path))
                if path.to_ascii_lowercase().ends_with(".esp") || path.to_ascii_lowercase().ends_with(".esm"));
            if name == "version" && on_plugin {
                found.push(expr.clone());
            }
        }
        Expr::Not(inner) => plugin_version_calls(inner, found),
        Expr::And(left, right) | Expr::Or(left, right) => {
            plugin_version_calls(left, found);
            plugin_version_calls(right, found);
        }
    }
}

/// `version()` on a plugin is evaluated from the description, never reported as unsupported.
fn assert_plugin_versions_are_evaluated(texts: &[&str]) {
    let mut plugin_calls = Vec::new();
    for text in texts {
        if let Ok(expr) = parse(text) {
            plugin_version_calls(&expr, &mut plugin_calls);
        }
    }
    let empty = std::env::temp_dir().join(format!("loot-rs-real-{}", std::process::id()));
    std::fs::create_dir_all(&empty).expect("a scratch directory");
    let env = FsEnv::new(&empty, Vec::new());
    for call in &plugin_calls {
        assert!(
            !matches!(evaluate(call, &env), Err(ConditionError::Unsupported(_))),
            "{call} is unsupported"
        );
    }
    let _ = std::fs::remove_dir_all(&empty);
    println!(
        "version() on a plugin: {} calls, all evaluated",
        plugin_calls.len()
    );
}

#[test]
#[ignore = "needs the real masterlist: set LOOT_RS_MASTERLIST"]
fn the_real_masterlist_parses_and_every_condition_in_it_is_valid_syntax() {
    let _guard = ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let text = masterlist_text();
    println!(
        "expanded nodes: {} (limit {})",
        loot_rs::metadata::expanded_node_count(&text).expect("within the limits"),
        loot_rs::metadata::MAX_EXPANDED_NODES
    );
    let started = Instant::now();
    let metadata = parse_masterlist(&text).expect("the real masterlist parses");
    println!("parse time: {:?}", started.elapsed());

    let entries = metadata.plugins.len() + metadata.warnings.len();
    let regexes = metadata
        .plugins
        .iter()
        .filter(|entry| matches!(entry.pattern, NamePattern::Regex(_)))
        .count();
    println!(
        "entries: {entries} plugin entries ({regexes} regular expressions, {} skipped by this engine)",
        metadata.warnings.len()
    );
    println!(
        "groups: {}, bash tags: {}, globals: {}",
        metadata.groups.len(),
        metadata.bash_tags.len(),
        metadata.globals.len()
    );
    for warning in &metadata.warnings {
        println!("skipped: {warning}");
    }

    let texts = conditions(&metadata);
    let mut syntax_errors = Vec::new();
    let mut by_function: BTreeMap<String, usize> = BTreeMap::new();
    let mut unsupported_conditions = 0;
    for text in &texts {
        match parse(text) {
            Ok(expr) => {
                let mut names = Vec::new();
                functions(&expr, &mut names);
                names.sort();
                names.dedup();
                if names
                    .iter()
                    .any(|name| UNSUPPORTED_FUNCTIONS.contains(&name.as_str()))
                {
                    unsupported_conditions += 1;
                }
                for name in names {
                    *by_function.entry(name).or_default() += 1;
                }
            }
            Err(error) => syntax_errors.push(format!("{text:?}: {error}")),
        }
    }
    assert_plugin_versions_are_evaluated(&texts);
    println!("conditions: {} in total", texts.len());
    println!(
        "unsupported: {unsupported_conditions} conditions use a function this evaluator does not implement"
    );
    for (name, count) in &by_function {
        let mark = if UNSUPPORTED_FUNCTIONS.contains(&name.as_str()) {
            " (unsupported)"
        } else {
            ""
        };
        println!("  {name}(): {count}{mark}");
    }

    assert!(
        entries > 500,
        "only {entries} entries: is this the falloutnv masterlist?"
    );
    assert!(
        syntax_errors.is_empty(),
        "conditions that do not parse:\n{}",
        syntax_errors.join("\n")
    );
    // A name that is neither implemented nor listed as unsupported is a typo in the masterlist (the
    // current one has `notfile(`) or a function added after this evaluator: it is reported here, and
    // evaluating it is a syntax error that the checker notes, never a crash.
    let known = [
        "file",
        "active",
        "many",
        "many_active",
        "readable",
        "is_master",
        "file_size",
        "checksum",
    ];
    let unknown: Vec<&String> = by_function
        .keys()
        .filter(|name| {
            !known.contains(&name.as_str()) && !UNSUPPORTED_FUNCTIONS.contains(&name.as_str())
        })
        .collect();
    println!("unknown functions: {unknown:?}");
    assert!(unknown.len() <= 3, "{unknown:?}");
}

#[test]
#[ignore = "needs the real masterlist: set LOOT_RS_MASTERLIST"]
fn four_hundred_plugins_are_checked_against_the_real_masterlist_within_the_time_budget() {
    let _guard = ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let metadata = parse_masterlist(&masterlist_text()).expect("the real masterlist parses");
    let budget = Duration::from_secs(
        std::env::var("LOOT_RS_BUDGET_SECS")
            .ok()
            .and_then(|text| text.parse().ok())
            .unwrap_or(10),
    );

    // The first 400 plugin names the masterlist spells out, written as synthetic plugins.
    let names: Vec<String> = metadata
        .plugins
        .iter()
        .filter_map(|entry| match &entry.pattern {
            NamePattern::Exact(name)
                if name.to_lowercase().ends_with(".esp")
                    || name.to_lowercase().ends_with(".esm") =>
            {
                Some(name.clone())
            }
            _ => None,
        })
        .filter(|name| !name.contains('/') && !name.contains('\\'))
        .take(400)
        .collect();
    assert_eq!(
        names.len(),
        400,
        "the masterlist names fewer than 400 plugins"
    );
    let fixture = Fixture::new();
    fixture.plugin("FalloutNV.esm", &[]);
    for name in &names {
        fixture.plugin(name, &["FalloutNV.esm"]);
    }
    let mut order = String::from("FalloutNV.esm\n");
    for name in &names {
        order.push_str(name);
        order.push('\n');
    }
    let env = FsEnv::new(&fixture.data(), parse_plugins_txt(&order));

    let started = Instant::now();
    let report = run(&metadata, &env, Options::default());
    let elapsed = started.elapsed();

    let with_entries = report
        .plugins
        .iter()
        .filter(|plugin| plugin.entries_matched > 0)
        .count();
    println!(
        "plugins: {} checked, {with_entries} with entries, {} findings",
        report.plugins.len(),
        report.findings().count()
    );
    println!("budget: {elapsed:?} of {budget:?}");
    println!("hard violations: {}", report.hard_count());
    for (name, count) in report.unsupported_functions() {
        println!("unsupported {name}(): {count} items not applied");
    }

    assert!(
        report.plugins.len() >= 400,
        "{} plugins checked",
        report.plugins.len()
    );
    assert!(
        with_entries >= 400,
        "every name came from an entry, so every plugin matches one"
    );
    assert!(
        elapsed < budget,
        "checking took {elapsed:?}, budget {budget:?}"
    );
}
