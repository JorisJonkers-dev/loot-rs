//! Property-based tests of the condition parser and evaluator.

mod common;

use common::model::{Model, NAMES};
use loot_rs::condition::{Arg, ConditionError, Expr, MAX_NESTING, eval_str, evaluate, parse};
use proptest::prelude::*;

fn name() -> impl Strategy<Value = String> {
    prop_oneof![
        3 => proptest::sample::select(NAMES.to_vec()).prop_map(String::from),
        1 => "[a-zA-Z0-9_ ./()-]{0,12}",
    ]
}

/// A call of a supported function with well-formed arguments.
fn call() -> impl Strategy<Value = Expr> {
    prop_oneof![
        name().prop_map(|n| Expr::Call("file".into(), vec![Arg::Str(n)])),
        name().prop_map(|n| Expr::Call("active".into(), vec![Arg::Str(n)])),
        name().prop_map(|n| Expr::Call("many".into(), vec![Arg::Str(n)])),
        name().prop_map(|n| Expr::Call("many_active".into(), vec![Arg::Str(n)])),
        name().prop_map(|n| Expr::Call("readable".into(), vec![Arg::Str(n)])),
        name().prop_map(|n| Expr::Call("is_master".into(), vec![Arg::Str(n)])),
        (name(), 0u64..64).prop_map(|(n, size)| Expr::Call(
            "file_size".into(),
            vec![Arg::Str(n), Arg::Word(size.to_string())]
        )),
        (name(), any::<u32>()).prop_map(|(n, crc)| Expr::Call(
            "checksum".into(),
            vec![Arg::Str(n), Arg::Word(format!("{crc:X}"))]
        )),
    ]
}

fn expr() -> impl Strategy<Value = Expr> {
    call().prop_recursive(5, 48, 4, |inner| {
        prop_oneof![
            inner.clone().prop_map(|e| Expr::Not(Box::new(e))),
            (inner.clone(), inner.clone()).prop_map(|(a, b)| Expr::And(Box::new(a), Box::new(b))),
            (inner.clone(), inner).prop_map(|(a, b)| Expr::Or(Box::new(a), Box::new(b))),
        ]
    })
}

fn model() -> impl Strategy<Value = Model> {
    let names = proptest::collection::btree_set(
        proptest::sample::select(NAMES.to_vec()).prop_map(String::from),
        0..5,
    );
    (
        names.clone(),
        names.clone(),
        names,
        proptest::collection::btree_map(
            proptest::sample::select(NAMES.to_vec()).prop_map(String::from),
            0u64..64,
            0..4,
        ),
        proptest::collection::btree_map(
            proptest::sample::select(NAMES.to_vec()).prop_map(String::from),
            any::<u32>(),
            0..4,
        ),
        proptest::collection::btree_set(
            proptest::sample::select(NAMES.to_vec()).prop_map(String::from),
            0..3,
        ),
        proptest::collection::btree_set(
            proptest::sample::select(NAMES.to_vec()).prop_map(|n| format!("{n}#2")),
            0..3,
        ),
    )
        .prop_map(|(files, active, _, sizes, crcs, masters, doubles)| {
            let mut files = files;
            files.extend(doubles.clone());
            let mut active = active;
            active.extend(doubles);
            Model {
                files,
                active,
                sizes,
                crcs,
                masters,
            }
        })
}

fn value(expr: &Expr, world: &Model) -> bool {
    evaluate(expr, world).expect("supported functions over the model always answer")
}

fn and(a: &Expr, b: &Expr) -> Expr {
    Expr::And(Box::new(a.clone()), Box::new(b.clone()))
}
fn or(a: &Expr, b: &Expr) -> Expr {
    Expr::Or(Box::new(a.clone()), Box::new(b.clone()))
}
fn not(a: &Expr) -> Expr {
    Expr::Not(Box::new(a.clone()))
}

proptest! {
    #![proptest_config(common::proptest_config(256))]

    // ---- round trips -------------------------------------------------------------------------

    #[test]
    fn printing_then_parsing_gives_back_the_expression(e in expr()) {
        prop_assert_eq!(parse(&e.to_string()), Ok(e));
    }

    #[test]
    fn printing_is_a_fixed_point(e in expr()) {
        let once = e.to_string();
        let again = parse(&once).expect("parses").to_string();
        prop_assert_eq!(once, again);
    }

    #[test]
    fn extra_whitespace_between_tokens_changes_nothing(e in expr(), pad in "[ \t\r\n]{1,3}") {
        // Names may hold spaces, so pad only at the token boundaries that are outside quotes.
        let text = e.to_string();
        let mut padded = String::new();
        let mut quoted = false;
        for c in text.chars() {
            if c == '"' { quoted = !quoted; }
            padded.push(c);
            if !quoted && (c == '(' || c == ')' || c == ',') { padded.push_str(&pad); }
        }
        prop_assert_eq!(parse(&padded), parse(&text));
    }

    // ---- never panics ------------------------------------------------------------------------

    #[test]
    fn arbitrary_text_never_panics(text in ".{0,200}") {
        let _ = parse(&text);
        let _ = eval_str(&text, &Model::default());
    }

    #[test]
    fn arbitrary_token_soup_never_panics(tokens in proptest::collection::vec(
        proptest::sample::select(vec![
            "file", "active", "many", "version", "not", "and", "or", "(", ")", ",", "\"", "\"a.esp\"",
            "==", ">=", "1.0", "DEADBEEF", "0x1F", " ", "\n", "checksum", "file_size", "#", "é", "\\",
        ]),
        0..40,
    )) {
        let text: String = tokens.concat();
        let _ = parse(&text);
        let _ = eval_str(&text, &Model::default());
    }

    #[test]
    fn a_parse_error_is_always_a_syntax_error(text in ".{0,100}") {
        if let Err(error) = parse(&text) {
            prop_assert!(matches!(error, ConditionError::Syntax(_)));
        }
    }

    #[test]
    fn nesting_beyond_the_limit_is_refused_not_crashed(depth in 0usize..400, with_not in any::<bool>()) {
        let open = if with_not { "not (" } else { "(" };
        let text = format!("{}file(\"a.esp\"){}", open.repeat(depth), ")".repeat(depth));
        let result = parse(&text);
        if depth >= MAX_NESTING {
            prop_assert!(result.is_err());
        } else if depth < MAX_NESTING / 2 {
            prop_assert!(result.is_ok());
        }
    }

    // ---- boolean laws ------------------------------------------------------------------------

    #[test]
    fn and_is_conjunction_or_is_disjunction_not_is_negation(a in expr(), b in expr(), w in model()) {
        prop_assert_eq!(value(&and(&a, &b), &w), value(&a, &w) && value(&b, &w));
        prop_assert_eq!(value(&or(&a, &b), &w), value(&a, &w) || value(&b, &w));
        prop_assert_eq!(value(&not(&a), &w), !value(&a, &w));
    }

    #[test]
    fn double_negation(a in expr(), w in model()) {
        prop_assert_eq!(value(&not(&not(&a)), &w), value(&a, &w));
    }

    #[test]
    fn commutativity(a in expr(), b in expr(), w in model()) {
        prop_assert_eq!(value(&and(&a, &b), &w), value(&and(&b, &a), &w));
        prop_assert_eq!(value(&or(&a, &b), &w), value(&or(&b, &a), &w));
    }

    #[test]
    fn associativity(a in expr(), b in expr(), c in expr(), w in model()) {
        prop_assert_eq!(value(&and(&and(&a, &b), &c), &w), value(&and(&a, &and(&b, &c)), &w));
        prop_assert_eq!(value(&or(&or(&a, &b), &c), &w), value(&or(&a, &or(&b, &c)), &w));
    }

    #[test]
    fn distributivity(a in expr(), b in expr(), c in expr(), w in model()) {
        prop_assert_eq!(value(&and(&a, &or(&b, &c)), &w), value(&or(&and(&a, &b), &and(&a, &c)), &w));
        prop_assert_eq!(value(&or(&a, &and(&b, &c)), &w), value(&and(&or(&a, &b), &or(&a, &c)), &w));
    }

    #[test]
    fn de_morgan(a in expr(), b in expr(), w in model()) {
        prop_assert_eq!(value(&not(&and(&a, &b)), &w), value(&or(&not(&a), &not(&b)), &w));
        prop_assert_eq!(value(&not(&or(&a, &b)), &w), value(&and(&not(&a), &not(&b)), &w));
    }

    #[test]
    fn idempotence_absorption_and_complement(a in expr(), b in expr(), w in model()) {
        prop_assert_eq!(value(&and(&a, &a), &w), value(&a, &w));
        prop_assert_eq!(value(&or(&a, &a), &w), value(&a, &w));
        prop_assert_eq!(value(&and(&a, &or(&a, &b)), &w), value(&a, &w));
        prop_assert_eq!(value(&or(&a, &and(&a, &b)), &w), value(&a, &w));
        prop_assert!(!value(&and(&a, &not(&a)), &w));
        prop_assert!(value(&or(&a, &not(&a)), &w));
    }

    #[test]
    fn evaluating_the_text_agrees_with_evaluating_the_tree(e in expr(), w in model()) {
        prop_assert_eq!(eval_str(&e.to_string(), &w), Ok(value(&e, &w)));
    }

    #[test]
    fn precedence_not_binds_tighter_than_and_tighter_than_or(a in call(), b in call(), c in call(), w in model()) {
        let text = format!("{a} or {b} and not {c}");
        let expected = value(&a, &w) || (value(&b, &w) && !value(&c, &w));
        prop_assert_eq!(eval_str(&text, &w), Ok(expected));
    }

    // ---- the functions against the model -----------------------------------------------------

    #[test]
    fn functions_agree_with_the_model(n in name(), w in model()) {
        let q = |text: String| eval_str(&text, &w);
        let singles = usize::from(w.files.contains(&n)) + usize::from(w.files.contains(&format!("{n}#2")));
        prop_assert_eq!(q(format!("file(\"{n}\")")), Ok(singles > 0));
        prop_assert_eq!(q(format!("many(\"{n}\")")), Ok(singles > 1));
        prop_assert_eq!(q(format!("readable(\"{n}\")")), Ok(w.files.contains(&n)));
        prop_assert_eq!(q(format!("is_master(\"{n}\")")), Ok(w.masters.contains(&n)));
        let size = w.sizes.get(&n).copied();
        prop_assert_eq!(q(format!("file_size(\"{n}\", {})", size.unwrap_or(99))), Ok(size.is_some()));
        if let Some(crc) = w.crcs.get(&n) {
            prop_assert_eq!(q(format!("checksum(\"{n}\", {crc:X})")), Ok(true));
            prop_assert_eq!(q(format!("checksum(\"{n}\", 0x{crc:x})")), Ok(true));
            prop_assert_eq!(q(format!("checksum(\"{n}\", {:X})", crc.wrapping_add(1))), Ok(false));
        }
    }

    // ---- errors are never guessed away -------------------------------------------------------

    #[test]
    fn an_unsupported_function_only_matters_when_it_decides_the_result(a in expr(), w in model()) {
        let unsupported = Expr::Call("version".into(), vec![Arg::Str("a.esp".into()), Arg::Word("==".into()), Arg::Str("1".into())]);
        let u = |e: Expr| evaluate(&e, &w);
        let truth = value(&a, &w);
        prop_assert_eq!(u(and(&a, &unsupported)), if truth { Err(ConditionError::Unsupported("version".into())) } else { Ok(false) });
        prop_assert_eq!(u(and(&unsupported, &a)), if truth { Err(ConditionError::Unsupported("version".into())) } else { Ok(false) });
        prop_assert_eq!(u(or(&a, &unsupported)), if truth { Ok(true) } else { Err(ConditionError::Unsupported("version".into())) });
        prop_assert_eq!(u(or(&unsupported, &a)), if truth { Ok(true) } else { Err(ConditionError::Unsupported("version".into())) });
        prop_assert_eq!(u(not(&unsupported)), Err(ConditionError::Unsupported("version".into())));
    }
}
