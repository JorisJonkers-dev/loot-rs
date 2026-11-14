// SPDX-License-Identifier: GPL-3.0-or-later
//
// Ported to Rust from libloot (https://github.com/loot/libloot, `src/plugin/mod.rs`, `extract_version`,
// GPL-3.0-or-later, Copyright (C) the libloot authors) and from loot-condition-interpreter
// (https://github.com/loot/loot-condition-interpreter, `src/function/version.rs`, MIT, Copyright (C) the LOOT
// authors), see NOTICE.md.

//! The version of a plugin and the comparison of versions, as LOOT does them.
//!
//! * [`extract_version`] reads a plugin's version out of its description field, with libloot's expressions in
//!   libloot's order: a timestamp, then `version:` and a pseudo-semver, then a bare pseudo-semver, then `v` or
//!   `version:` and a number.
//! * [`Version`] compares two version strings as the condition interpreter does (Pseudosem), so that
//!   `version("TaleOfTwoWastelands.esm", "3.2.0", >=)` agrees with LOOT.
//!
//! Both are ported rather than taken from the crates: `libloot` is not a dependency, and the interpreter's `Version`
//! is private to it. The default build therefore gains no dependency.

use regex::Regex;
use std::cmp::Ordering;
use std::sync::OnceLock;

/// What a plugin's description says its version is, or `None` when it says none.
///
/// The text is searched with four expressions in turn and the first that matches decides; the version is the
/// first capture, trimmed.
///
/// # Panics
/// Never in practice: the expressions are fixed and valid.
#[must_use]
pub fn extract_version(description: &str) -> Option<String> {
    static REGEXES: OnceLock<[Regex; 4]> = OnceLock::new();
    let regexes = REGEXES.get_or_init(|| {
        // The range of strings Pseudosem v1.0.1 supports, without space separators (they make extraction from
        // sentences too hard). The last group stops a version followed by a comma from matching. `[0-9]` and not
        // `\d`: libloot's expressions are ECMAScript, where `\d` is ASCII.
        let pseudosem = r"([0-9]+(?:\.[0-9]+)+(?:[-._:]?[A-Za-z0-9]+)*)(?:[^,]|$)";
        let build = |text: &str| Regex::new(&format!("(?i){text}")).expect("a fixed expression");
        [
            build(r"([0-9]{1,2}/[0-9]{1,2}/[0-9]{1,4} [0-9]{1,2}:[0-9]{1,2}:[0-9]{1,2})"),
            build(&format!(r"version:?\s{pseudosem}")),
            build(&format!(r"(?:^|v|\s){pseudosem}")),
            build(r"(?:^|v|version:\s*)([0-9]+)"),
        ]
    });
    regexes.iter().find_map(|regex| {
        regex
            .captures(description)
            .and_then(|captures| captures.get(1))
            .map(|found| found.as_str().trim().to_owned())
    })
}

#[derive(Clone, Debug)]
enum ReleaseId {
    Numeric(u32),
    NonNumeric(String),
}

impl From<&str> for ReleaseId {
    fn from(string: &str) -> Self {
        string
            .trim()
            .parse()
            .map_or_else(|_| Self::NonNumeric(string.to_lowercase()), Self::Numeric)
    }
}

fn numeric_equals(number: u32, text: &str) -> bool {
    text.trim().parse().is_ok_and(|other: u32| other == number)
}

impl PartialEq for ReleaseId {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Numeric(a), Self::Numeric(b)) => a == b,
            (Self::NonNumeric(a), Self::NonNumeric(b)) => a == b,
            (Self::Numeric(n), Self::NonNumeric(s)) | (Self::NonNumeric(s), Self::Numeric(n)) => {
                numeric_equals(*n, s)
            }
        }
    }
}

/// Like `u32::from_str_radix`, but stops at the first non-digit instead of failing, and takes no sign. Returns the
/// number and how many bytes were left after the digits.
fn leading_number(id: &str) -> (Option<u32>, usize) {
    if let Some((digits, remainder)) = id.split_once(|c: char| !c.is_ascii_digit()) {
        if digits.is_empty() {
            (None, id.len())
        } else {
            (digits.trim().parse().ok(), remainder.len() + 1)
        }
    } else {
        (id.trim().parse().ok(), 0)
    }
}

fn compare_mixed(number: u32, text: &str) -> Option<Ordering> {
    match leading_number(text) {
        // A non-numeric id with the same leading digits and more after them is the greater.
        (Some(other), rest) => match number.partial_cmp(&other) {
            Some(Ordering::Equal) if rest > 0 => Some(Ordering::Less),
            order => order,
        },
        // Numbers are less than ids that start with no digits.
        (None, _) => Some(Ordering::Less),
    }
}

impl PartialOrd for ReleaseId {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        match (self, other) {
            (Self::Numeric(a), Self::Numeric(b)) => a.partial_cmp(b),
            (Self::NonNumeric(a), Self::NonNumeric(b)) => a.partial_cmp(b),
            (Self::Numeric(n), Self::NonNumeric(s)) => compare_mixed(*n, s),
            (Self::NonNumeric(s), Self::Numeric(n)) => compare_mixed(*n, s).map(Ordering::reverse),
        }
    }
}

#[derive(Clone, Debug, PartialEq, PartialOrd)]
enum PreReleaseId {
    Numeric(u32),
    NonNumeric(String),
}

impl From<&str> for PreReleaseId {
    fn from(string: &str) -> Self {
        string
            .trim()
            .parse()
            .map_or_else(|_| Self::NonNumeric(string.to_lowercase()), Self::Numeric)
    }
}

/// A version as LOOT compares them: release ids separated by `.` (or `,`), then pre-release ids.
#[derive(Debug, Clone)]
pub struct Version {
    release: Vec<ReleaseId>,
    pre_release: Vec<PreReleaseId>,
}

const fn is_separator(c: char) -> bool {
    matches!(c, '-' | ' ' | ':' | '_')
}

const fn is_pre_release_separator(c: char) -> bool {
    c == '.' || is_separator(c)
}

fn trim_metadata(version: &str) -> &str {
    if version.is_empty() {
        "0"
    } else {
        version
            .split_once('+')
            .map_or(version, |(prefix, _)| prefix)
    }
}

fn split_version(string: &str) -> (&str, &str) {
    // `0, 1, 2, 3` (OBSE, SKSE) means `0.1.2.3`.
    static COMMAS: OnceLock<Regex> = OnceLock::new();
    let commas = COMMAS
        .get_or_init(|| Regex::new("[0-9]+, [0-9]+, [0-9]+, [0-9]+").expect("a fixed expression"));
    if commas.is_match(string) {
        return (string, "");
    }
    string.split_once(is_separator).unwrap_or((string, ""))
}

impl From<&str> for Version {
    fn from(string: &str) -> Self {
        let (release, pre_release) = split_version(trim_metadata(string));
        Self {
            release: release.split(['.', ',']).map(ReleaseId::from).collect(),
            pre_release: pre_release
                .split_terminator(is_pre_release_separator)
                .map(PreReleaseId::from)
                .collect(),
        }
    }
}

fn padded(a: &[ReleaseId], b: &[ReleaseId]) -> (Vec<ReleaseId>, Vec<ReleaseId>) {
    let (mut a, mut b) = (a.to_vec(), b.to_vec());
    let length = a.len().max(b.len());
    a.resize(length, ReleaseId::Numeric(0));
    b.resize(length, ReleaseId::Numeric(0));
    (a, b)
}

impl PartialEq for Version {
    fn eq(&self, other: &Self) -> bool {
        let (a, b) = padded(&self.release, &other.release);
        a == b && self.pre_release == other.pre_release
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        let (a, b) = padded(&self.release, &other.release);
        match a.partial_cmp(&b) {
            Some(Ordering::Equal) | None => {
                match (self.pre_release.is_empty(), other.pre_release.is_empty()) {
                    (true, false) => Some(Ordering::Greater),
                    (false, true) => Some(Ordering::Less),
                    _ => self.pre_release.partial_cmp(&other.pre_release),
                }
            }
            order => order,
        }
    }
}

/// Compare `actual` with `given` by one of the six condition operators (`==`, `!=`, `<`, `>`, `<=`, `>=`).
/// `None` for another operator.
#[must_use]
pub fn compare(actual: &str, operator: &str, given: &str) -> Option<bool> {
    let (actual, given) = (Version::from(actual), Version::from(given));
    Some(match operator {
        "==" => actual == given,
        "!=" => actual != given,
        "<" => actual < given,
        ">" => actual > given,
        "<=" => actual <= given,
        ">=" => actual >= given,
        _ => return None,
    })
}

#[cfg(test)]
#[path = "version_tests.rs"]
mod tests;
