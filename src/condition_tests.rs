//! Unit tests of `condition`.

use super::*;
use std::collections::HashMap;

#[derive(Default)]
struct Fake {
    files: Vec<&'static str>,
    active: Vec<&'static str>,
    sizes: HashMap<&'static str, u64>,
    crcs: HashMap<&'static str, u32>,
}

impl Env for Fake {
    fn count_files(&self, path: &str) -> Result<usize, ConditionError> {
        Ok(self.files.iter().filter(|file| **file == path).count())
    }
    fn count_active(&self, path: &str) -> Result<usize, ConditionError> {
        Ok(self.active.iter().filter(|file| **file == path).count())
    }
    fn file_size(&self, path: &str) -> Option<u64> {
        self.sizes.get(path).copied()
    }
    fn readable(&self, path: &str) -> bool {
        self.files.contains(&path)
    }
    fn is_master(&self, path: &str) -> bool {
        path == "m.esm"
    }
    fn checksum(&self, path: &str) -> Option<u32> {
        self.crcs.get(path).copied()
    }
}

fn fake() -> Fake {
    Fake {
        files: vec!["a.esp", "a.esp", "b.esp", "m.esm"],
        active: vec!["a.esp"],
        sizes: HashMap::from([("a.esp", 10)]),
        crcs: HashMap::from([("a.esp", 0xDEAD_BEEF)]),
    }
}

fn eval(text: &str) -> Result<bool, ConditionError> {
    eval_str(text, &fake())
}

#[test]
fn file_and_active() {
    assert_eq!(eval(r#"file("b.esp")"#), Ok(true));
    assert_eq!(eval(r#"file("zzz.esp")"#), Ok(false));
    assert_eq!(eval(r#"active("a.esp")"#), Ok(true));
    assert_eq!(eval(r#"active("b.esp")"#), Ok(false));
}

#[test]
fn many_counts_more_than_one() {
    assert_eq!(eval(r#"many("a.esp")"#), Ok(true));
    assert_eq!(eval(r#"many("b.esp")"#), Ok(false));
    assert_eq!(eval(r#"many_active("a.esp")"#), Ok(false));
}

#[test]
fn precedence_is_not_then_and_then_or() {
    // true or (false and false) = true; (true or false) and false would be false.
    assert_eq!(
        eval(r#"file("a.esp") or file("x") and file("y")"#),
        Ok(true)
    );
    assert_eq!(
        eval(r#"(file("a.esp") or file("x")) and file("y")"#),
        Ok(false)
    );
    assert_eq!(
        eval(r#"file("x") and file("y") or not file("x")"#),
        Ok(true)
    );
    assert_eq!(eval(r#"not not file("a.esp")"#), Ok(true));
}

#[test]
fn whitespace_and_line_breaks_are_allowed() {
    assert_eq!(
        eval("file( \"a.esp\" )\n and\n( not file(\"x\") )"),
        Ok(true)
    );
}

#[test]
fn size_checksum_master_and_readable() {
    assert_eq!(eval(r#"file_size("a.esp", 10)"#), Ok(true));
    assert_eq!(eval(r#"file_size("a.esp", 11)"#), Ok(false));
    assert_eq!(eval(r#"checksum("a.esp", DEADBEEF)"#), Ok(true));
    assert_eq!(eval(r#"checksum("a.esp", 0xdeadbeef)"#), Ok(true));
    assert_eq!(eval(r#"checksum("b.esp", DEADBEEF)"#), Ok(false));
    assert_eq!(eval(r#"is_master("m.esm")"#), Ok(true));
    assert_eq!(eval(r#"is_master("a.esp")"#), Ok(false));
    assert_eq!(eval(r#"readable("a.esp")"#), Ok(true));
}

#[test]
fn version_functions_are_reported_unsupported_never_guessed() {
    for text in [
        r#"version("a.esp", ==, "1.0")"#,
        r#"product_version("a.exe", >=, "1.0")"#,
        r#"filename_version("a (.+).7z", >, "1")"#,
        r#"description_contains("a.esp", "x")"#,
        r#"is_executable("a.exe")"#,
    ] {
        assert!(
            matches!(eval(text), Err(ConditionError::Unsupported(_))),
            "{text}"
        );
    }
}

#[test]
fn unsupported_only_matters_when_it_decides_the_result() {
    assert_eq!(
        eval(r#"file("x") and version("a.esp", ==, "1")"#),
        Ok(false)
    );
    assert_eq!(
        eval(r#"file("a.esp") or version("a.esp", ==, "1")"#),
        Ok(true)
    );
    assert!(eval(r#"file("a.esp") and version("a.esp", ==, "1")"#).is_err());
    assert!(eval(r#"file("x") or version("a.esp", ==, "1")"#).is_err());
    assert!(eval(r#"not version("a.esp", ==, "1")"#).is_err());
}

#[test]
fn syntax_errors() {
    for text in [
        r#"file("a.esp""#,
        r#"file("a.esp") and"#,
        r#"file "a.esp""#,
        r#"file("a.esp") file("b")"#,
        r#"file("unterminated)"#,
        r#"frobnicate("x")"#,
        r"file()",
        r#"checksum("a", zz)"#,
        "file(\"a\") # no",
    ] {
        assert!(
            matches!(eval(text), Err(ConditionError::Syntax(_))),
            "{text}"
        );
    }
}

#[test]
fn blank_condition_is_true() {
    assert_eq!(eval("  "), Ok(true));
}
