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

#[test]
fn display_prints_the_canonical_text_and_parses_back() {
    let text = r#"file("a.esp") and (not active("b.esp") or checksum("c.esp", DEADBEEF))"#;
    let expr = parse(text).expect("parses");
    assert_eq!(
        expr.to_string(),
        r#"file("a.esp") and (not active("b.esp") or checksum("c.esp", DEADBEEF))"#
    );
    assert_eq!(parse(&expr.to_string()), Ok(expr));
    let loose = parse(r#"file("a") or file("b") and not file("c")"#).expect("parses");
    assert_eq!(
        loose.to_string(),
        r#"file("a") or file("b") and not file("c")"#
    );
}

#[test]
fn display_keeps_the_parentheses_that_the_grammar_needs_and_no_others() {
    for (text, printed) in [
        ("(a() or b()) and c()", "(a() or b()) and c()"),
        ("a() and (b() and c())", "a() and (b() and c())"),
        ("(a() and b()) and c()", "a() and b() and c()"),
        ("a() or (b() or c())", "a() or (b() or c())"),
        ("(a() or b()) or c()", "a() or b() or c()"),
        ("not (a() or b())", "not (a() or b())"),
        ("not (not a())", "not not a()"),
        ("not (a() and b()) or c()", "not (a() and b()) or c()"),
        (
            "(a() and b()) or (c() and d())",
            "a() and b() or c() and d()",
        ),
        ("a() and not b() and c()", "a() and not b() and c()"),
    ] {
        let expr = parse(text).expect("parses");
        assert_eq!(expr.to_string(), printed, "{text}");
        assert_eq!(parse(printed), Ok(expr), "{printed}");
    }
}

#[test]
fn a_long_chain_of_operators_prints_as_one_level_and_parses_back() {
    // The fuzzer found that every operator of a chain printed as one more level of parentheses, so a condition
    // that parsed (a chain is not nested) printed as text over the nesting limit.
    let chain = (0..MAX_NESTING * 2)
        .map(|index| format!("file(\"{index}.esp\")"))
        .collect::<Vec<_>>()
        .join(" or ");
    let expr = parse(&chain).expect("a chain is not nested");
    let printed = expr.to_string();
    assert_eq!(printed, chain);
    assert_eq!(parse(&printed), Ok(expr));
    let nots = format!("{}file(\"a\")", "not ".repeat(MAX_NESTING - 1));
    let expr = parse(&nots).expect("parses");
    assert_eq!(parse(&expr.to_string()), Ok(expr));
}

#[test]
fn display_of_calls_and_arguments() {
    assert_eq!(parse("file()").map(|e| e.to_string()), Ok("file()".into()));
    assert_eq!(
        parse(r#"file_size("a.esp", 10)"#)
            .expect("parses")
            .to_string(),
        r#"file_size("a.esp", 10)"#
    );
    assert_eq!(Arg::Str("x y".into()).to_string(), "\"x y\"");
    assert_eq!(Arg::Word(">=".into()).to_string(), ">=");
    assert_eq!(
        Expr::Call("is_executable".into(), vec![]).to_string(),
        "is_executable()"
    );
}

#[test]
fn nesting_is_limited() {
    let ok = format!(
        "{}file(\"a.esp\"){}",
        "(".repeat(MAX_NESTING - 1),
        ")".repeat(MAX_NESTING - 1)
    );
    assert_eq!(eval(&ok), Ok(true));
    let too_deep = format!(
        "{}file(\"a.esp\"){}",
        "(".repeat(MAX_NESTING + 1),
        ")".repeat(MAX_NESTING + 1)
    );
    assert!(
        matches!(eval(&too_deep), Err(ConditionError::Syntax(text)) if text.contains("nested"))
    );
    let nots = format!("{}file(\"a.esp\")", "not ".repeat(MAX_NESTING + 1));
    assert!(matches!(eval(&nots), Err(ConditionError::Syntax(_))));
}

#[test]
fn the_number_of_operators_is_limited() {
    let chain = |count: usize| vec![r#"file("a.esp")"#; count].join(" and ");
    assert_eq!(eval(&chain(500)), Ok(true));
    // Short calls fit more operators than the text limit would allow with long ones.
    let error = parse(&format!("{}f()", "f()and ".repeat(MAX_NODES))).expect_err("too many");
    assert!(
        matches!(&error, ConditionError::Syntax(text) if text.contains("more than")),
        "{error}"
    );
}

#[test]
fn text_over_the_length_limit_is_refused_before_it_is_read() {
    let long = format!("file(\"{}\")", "a".repeat(MAX_CONDITION_BYTES));
    assert!(
        matches!(parse(&long), Err(ConditionError::Syntax(text)) if text.contains("longer than"))
    );
}

#[test]
fn a_very_long_chain_is_refused_and_dropped_without_overflowing_the_stack() {
    let hostile = vec!["not file(\"a\")"; 100_000].join(" or ");
    assert!(parse(&hostile).is_err());
}

#[test]
fn errors_read_as_sentences() {
    assert_eq!(
        ConditionError::Syntax("x".into()).to_string(),
        "invalid condition: x"
    );
    assert_eq!(
        ConditionError::Unsupported("version".into()).to_string(),
        "unsupported condition function version()"
    );
    assert_eq!(
        ConditionError::Environment("boom".into()).to_string(),
        "cannot evaluate condition: boom"
    );
}

#[test]
fn argument_kinds_and_counts_are_checked() {
    for text in [
        r"file(abc)",
        r#"file("a", "b")"#,
        r#"file_size("a")"#,
        r#"file_size("a", "10")"#,
        r#"file_size("a", ten)"#,
        r"file_size(10, 10)",
        r#"checksum("a")"#,
        r#"checksum("a", "DEADBEEF")"#,
        r#"is_master("a", 1)"#,
        r#"file("a",)"#,
        r#"file("a" "b")"#,
    ] {
        assert!(
            matches!(eval(text), Err(ConditionError::Syntax(_))),
            "{text}"
        );
    }
}

#[test]
fn text_that_is_not_a_condition_is_a_syntax_error() {
    for text in [
        "(",
        ")",
        "()",
        "not",
        "and",
        r#"file("a") or"#,
        r#"file("a") @"#,
        r#"file("a")) "#,
        r#""just a string""#,
        "file(\"a\") and and file(\"b\")",
        "\"unterminated",
    ] {
        assert!(
            matches!(parse(text), Err(ConditionError::Syntax(_))),
            "{text}"
        );
    }
}

#[test]
fn and_or_follow_three_valued_logic_over_errors() {
    let unsupported = r#"version("a.esp", ==, "1")"#;
    let cases = [
        (format!("file(\"zz\") and {unsupported}"), Ok(false)),
        (format!("{unsupported} and file(\"zz\")"), Ok(false)),
        (format!("file(\"a.esp\") or {unsupported}"), Ok(true)),
        (format!("{unsupported} or file(\"a.esp\")"), Ok(true)),
    ];
    for (text, expected) in cases {
        assert_eq!(eval(&text), expected, "{text}");
    }
    for text in [
        format!("file(\"a.esp\") and {unsupported}"),
        format!("file(\"zz\") or {unsupported}"),
    ] {
        assert!(
            matches!(eval(&text), Err(ConditionError::Unsupported(name)) if name == "version"),
            "{text}"
        );
    }
}

#[test]
fn every_unsupported_function_name_is_listed_and_unknown_ones_are_not() {
    for name in UNSUPPORTED_FUNCTIONS {
        assert_eq!(
            eval(&format!("{name}()")),
            Err(ConditionError::Unsupported(name.to_string()))
        );
    }
    assert_eq!(UNSUPPORTED_FUNCTIONS.len(), 5);
    assert!(matches!(eval("versions()"), Err(ConditionError::Syntax(_))));
}

#[test]
fn environment_errors_are_passed_on() {
    struct Broken;
    impl Env for Broken {
        fn count_files(&self, _: &str) -> Result<usize, ConditionError> {
            Err(ConditionError::Environment("no".into()))
        }
        fn count_active(&self, _: &str) -> Result<usize, ConditionError> {
            Err(ConditionError::Environment("no".into()))
        }
        fn file_size(&self, _: &str) -> Option<u64> {
            None
        }
        fn readable(&self, _: &str) -> bool {
            false
        }
        fn is_master(&self, _: &str) -> bool {
            false
        }
        fn checksum(&self, _: &str) -> Option<u32> {
            None
        }
    }
    for text in [
        r#"file("a")"#,
        r#"many("a")"#,
        r#"active("a")"#,
        r#"many_active("a")"#,
    ] {
        assert_eq!(
            eval_str(text, &Broken),
            Err(ConditionError::Environment("no".into())),
            "{text}"
        );
    }
    assert_eq!(
        eval_str(r#"readable("a") or not file_size("a", 1)"#, &Broken),
        Ok(true)
    );
}
