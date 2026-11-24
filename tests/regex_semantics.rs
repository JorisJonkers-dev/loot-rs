//! Plugin-name regular expressions: loot-rs (the `regex` crate) against libloot (regress, ECMAScript with the `iu`
//! flags) on a fixed table of the constructs LOOT documents and of the places where the two engines are known to
//! differ. The expected answers in `tests/regex/cases.tsv` were produced by libloot itself
//! (`libloot-driver --regex-table`); this test needs no libloot. A row whose answer differs is listed in
//! `KNOWN_DIFFERENCES` with the reason, which docs/COMPATIBILITY.md repeats.

mod common;

use common::{covers, ids};

use loot_rs::metadata::compile_plugin_name;

/// `(pattern, name)` rows where loot-rs answers differently from libloot, on purpose or by a documented limit.
const KNOWN_DIFFERENCES: &[(&str, &str)] = &[
    // Look-around: the `regex` crate has none, so loot-rs cannot compile the entry and skips it with a warning; the
    // weekly real-masterlist run fails when the masterlist has one (none in the New Vegas masterlist, 2026-11).
    (r"(?!Old)\w+\.esp", "New.esp"),
];

fn json_string(text: &str) -> String {
    let inner = text
        .strip_prefix('"')
        .and_then(|t| t.strip_suffix('"'))
        .expect("a JSON string");
    let mut out = String::new();
    let mut chars = inner.chars();
    let mut high: Option<u32> = None;
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next().expect("an escape") {
            'n' => out.push('\n'),
            't' => out.push('\t'),
            'r' => out.push('\r'),
            'u' => {
                let code = u32::from_str_radix(&chars.by_ref().take(4).collect::<String>(), 16)
                    .expect("hex");
                if (0xD800..0xDC00).contains(&code) {
                    high = Some(code);
                    continue;
                }
                let code = high.take().map_or(code, |high| {
                    0x10000 + ((high - 0xD800) << 10) + (code - 0xDC00)
                });
                out.push(char::from_u32(code).expect("a character"));
            }
            other => out.push(other),
        }
    }
    out
}

fn ours(pattern: &str, name: &str) -> &'static str {
    match compile_plugin_name(pattern) {
        Ok(regex) if regex.is_match(name) => "true",
        Ok(_) => "false",
        Err(_) => "error",
    }
}

#[test]
fn plugin_name_expressions_match_as_libloot_matches_them() {
    covers(&[ids::metadata::plugin::NAME_REGEX]);
    let table = include_str!("regex/cases.tsv");
    let mut differences = Vec::new();
    let mut rows = 0;
    for line in table
        .lines()
        .filter(|line| !line.starts_with('#') && !line.is_empty())
    {
        let mut fields = line.split('\t');
        let (pattern, name, theirs) = (
            json_string(fields.next().expect("pattern")),
            json_string(fields.next().expect("name")),
            fields.next().expect("libloot's answer"),
        );
        rows += 1;
        let known = KNOWN_DIFFERENCES.contains(&(pattern.as_str(), name.as_str()));
        if (ours(&pattern, &name) != theirs) != known {
            differences.push(format!(
                "{pattern:?} on {name:?}: libloot {theirs}, loot-rs {}, listed {known}",
                ours(&pattern, &name)
            ));
        }
    }
    assert!(rows > 50, "the table was read");
    assert!(differences.is_empty(), "{}", differences.join("\n"));
}
