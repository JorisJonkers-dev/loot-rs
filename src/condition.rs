//! Parser and evaluator for LOOT condition strings.
//!
//! Syntax reference: <https://loot-api.readthedocs.io/en/latest/metadata/conditions.html>.
//!
//! Supported functions: `file`, `active`, `many`, `many_active`, `file_size`, `readable`,
//! `is_master` and `checksum`, combined with `and`, `or`, `not` and parentheses (`not` binds
//! tightest, then `and`, then `or`). Functions that need version parsing, executable resources
//! or plugin descriptions (`version`, `product_version`, `filename_version`,
//! `description_contains`, `is_executable`) parse, but evaluate to
//! [`ConditionError::Unsupported`]: they are never guessed.
//!
//! `and` / `or` follow boolean logic with errors treated as unknown: `false and <unsupported>`
//! is `false`, `true or <unsupported>` is `true`, anything else involving an error is that
//! error.

use std::fmt;

/// What can go wrong with a condition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConditionError {
    /// The text is not a valid condition.
    Syntax(String),
    /// A function that this evaluator does not implement.
    Unsupported(String),
    /// The environment could not answer (for example an unreadable directory).
    Environment(String),
}

impl fmt::Display for ConditionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Syntax(text) => write!(f, "invalid condition: {text}"),
            Self::Unsupported(name) => write!(f, "unsupported condition function {name}()"),
            Self::Environment(text) => write!(f, "cannot evaluate condition: {text}"),
        }
    }
}

impl std::error::Error for ConditionError {}

/// An argument of a function call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Arg {
    /// A double-quoted string, without the quotes.
    Str(String),
    /// A bare token: a number, a hexadecimal checksum or a comparison operator.
    Word(String),
}

/// A parsed condition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expr {
    /// A function call.
    Call(String, Vec<Arg>),
    /// `not`.
    Not(Box<Expr>),
    /// `and`.
    And(Box<Expr>, Box<Expr>),
    /// `or`.
    Or(Box<Expr>, Box<Expr>),
}

/// The facts a condition can ask about. Paths are as written in the condition: relative to
/// the game's `Data` directory, `/`-separated, and possibly a regular expression in the last
/// component.
pub trait Env {
    /// Number of files matching a path or filename regex.
    ///
    /// # Errors
    /// When the directory cannot be read or the regex is invalid.
    fn count_files(&self, path: &str) -> Result<usize, ConditionError>;
    /// Number of active plugins matching a name or filename regex.
    ///
    /// # Errors
    /// When the regex is invalid.
    fn count_active(&self, path: &str) -> Result<usize, ConditionError>;
    /// Size in bytes of a file, `None` when it does not exist.
    fn file_size(&self, path: &str) -> Option<u64>;
    /// True when the path is a readable file or directory.
    fn readable(&self, path: &str) -> bool;
    /// True when the path is an installed plugin with the master flag set.
    fn is_master(&self, path: &str) -> bool;
    /// CRC-32 of a file's contents, `None` when it cannot be read.
    fn checksum(&self, path: &str) -> Option<u32>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    Open,
    Close,
    Comma,
    Str(String),
    Word(String),
}

fn tokenize(text: &str) -> Result<Vec<Token>, ConditionError> {
    let mut tokens = Vec::new();
    let mut chars = text.chars().peekable();
    while let Some(&c) = chars.peek() {
        match c {
            c if c.is_whitespace() => {
                chars.next();
            }
            '(' => {
                chars.next();
                tokens.push(Token::Open);
            }
            ')' => {
                chars.next();
                tokens.push(Token::Close);
            }
            ',' => {
                chars.next();
                tokens.push(Token::Comma);
            }
            '"' => {
                chars.next();
                let mut value = String::new();
                loop {
                    match chars.next() {
                        Some('"') => break,
                        Some(c) => value.push(c),
                        None => {
                            return Err(ConditionError::Syntax("unterminated string".into()));
                        }
                    }
                }
                tokens.push(Token::Str(value));
            }
            c if c.is_alphanumeric() || "_.=!<>+-".contains(c) => {
                let mut word = String::new();
                while let Some(&c) = chars.peek() {
                    if c.is_alphanumeric() || "_.=!<>+-".contains(c) {
                        word.push(c);
                        chars.next();
                    } else {
                        break;
                    }
                }
                tokens.push(Token::Word(word));
            }
            other => {
                return Err(ConditionError::Syntax(format!(
                    "unexpected character {other:?}"
                )));
            }
        }
    }
    Ok(tokens)
}

struct Parser {
    tokens: Vec<Token>,
    at: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.at)
    }

    fn next(&mut self) -> Option<Token> {
        let token = self.tokens.get(self.at).cloned();
        self.at += 1;
        token
    }

    fn peek_keyword(&self, keyword: &str) -> bool {
        matches!(self.peek(), Some(Token::Word(word)) if word == keyword)
    }

    fn expr(&mut self) -> Result<Expr, ConditionError> {
        let mut left = self.and_expr()?;
        while self.peek_keyword("or") {
            self.at += 1;
            left = Expr::Or(Box::new(left), Box::new(self.and_expr()?));
        }
        Ok(left)
    }

    fn and_expr(&mut self) -> Result<Expr, ConditionError> {
        let mut left = self.unary()?;
        while self.peek_keyword("and") {
            self.at += 1;
            left = Expr::And(Box::new(left), Box::new(self.unary()?));
        }
        Ok(left)
    }

    fn unary(&mut self) -> Result<Expr, ConditionError> {
        match self.next() {
            Some(Token::Word(word)) if word == "not" => Ok(Expr::Not(Box::new(self.unary()?))),
            Some(Token::Open) => {
                let inner = self.expr()?;
                match self.next() {
                    Some(Token::Close) => Ok(inner),
                    _ => Err(ConditionError::Syntax("missing closing parenthesis".into())),
                }
            }
            Some(Token::Word(name)) => {
                if self.next() != Some(Token::Open) {
                    return Err(ConditionError::Syntax(format!("expected ( after {name}")));
                }
                let mut args = Vec::new();
                if self.peek() == Some(&Token::Close) {
                    self.at += 1;
                    return Ok(Expr::Call(name, args));
                }
                loop {
                    match self.next() {
                        Some(Token::Str(text)) => args.push(Arg::Str(text)),
                        Some(Token::Word(text)) => args.push(Arg::Word(text)),
                        _ => {
                            return Err(ConditionError::Syntax(format!(
                                "bad argument in {name}()"
                            )));
                        }
                    }
                    match self.next() {
                        Some(Token::Comma) => {}
                        Some(Token::Close) => break,
                        _ => return Err(ConditionError::Syntax(format!("unterminated {name}()"))),
                    }
                }
                Ok(Expr::Call(name, args))
            }
            other => Err(ConditionError::Syntax(format!("unexpected {other:?}"))),
        }
    }
}

/// Parse a condition string.
///
/// # Errors
/// Returns [`ConditionError::Syntax`] for text that is not a condition.
pub fn parse(text: &str) -> Result<Expr, ConditionError> {
    let mut parser = Parser {
        tokens: tokenize(text)?,
        at: 0,
    };
    let expr = parser.expr()?;
    if parser.at < parser.tokens.len() {
        return Err(ConditionError::Syntax(
            "trailing text after the condition".into(),
        ));
    }
    Ok(expr)
}

/// Functions that exist in LOOT but are not implemented here.
pub const UNSUPPORTED_FUNCTIONS: [&str; 5] = [
    "version",
    "product_version",
    "filename_version",
    "description_contains",
    "is_executable",
];

fn string_arg<'a>(name: &str, args: &'a [Arg], index: usize) -> Result<&'a str, ConditionError> {
    match args.get(index) {
        Some(Arg::Str(text)) => Ok(text),
        _ => Err(ConditionError::Syntax(format!(
            "{name}() expects a quoted string as argument {}",
            index + 1
        ))),
    }
}

fn word_arg<'a>(name: &str, args: &'a [Arg], index: usize) -> Result<&'a str, ConditionError> {
    match args.get(index) {
        Some(Arg::Word(text)) => Ok(text),
        _ => Err(ConditionError::Syntax(format!(
            "{name}() expects a bare value as argument {}",
            index + 1
        ))),
    }
}

fn arity(name: &str, args: &[Arg], expected: usize) -> Result<(), ConditionError> {
    if args.len() == expected {
        Ok(())
    } else {
        Err(ConditionError::Syntax(format!(
            "{name}() takes {expected} argument(s), got {}",
            args.len()
        )))
    }
}

fn call(env: &dyn Env, name: &str, args: &[Arg]) -> Result<bool, ConditionError> {
    match name {
        "file" | "active" | "many" | "many_active" | "readable" | "is_master" => {
            arity(name, args, 1)?;
            let path = string_arg(name, args, 0)?;
            Ok(match name {
                "file" => env.count_files(path)? > 0,
                "many" => env.count_files(path)? > 1,
                "active" => env.count_active(path)? > 0,
                "many_active" => env.count_active(path)? > 1,
                "readable" => env.readable(path),
                _ => env.is_master(path),
            })
        }
        "file_size" => {
            arity(name, args, 2)?;
            let size: u64 = word_arg(name, args, 1)?
                .parse()
                .map_err(|_| ConditionError::Syntax("file_size() needs a decimal size".into()))?;
            Ok(env.file_size(string_arg(name, args, 0)?) == Some(size))
        }
        "checksum" => {
            arity(name, args, 2)?;
            let digits = word_arg(name, args, 1)?;
            let digits = digits.strip_prefix("0x").unwrap_or(digits);
            let expected = u32::from_str_radix(digits, 16).map_err(|_| {
                ConditionError::Syntax("checksum() needs hexadecimal digits".into())
            })?;
            Ok(env.checksum(string_arg(name, args, 0)?) == Some(expected))
        }
        _ if UNSUPPORTED_FUNCTIONS.contains(&name) => {
            Err(ConditionError::Unsupported(name.to_string()))
        }
        _ => Err(ConditionError::Syntax(format!("unknown function {name}()"))),
    }
}

/// Evaluate a parsed condition.
///
/// # Errors
/// Returns the first [`ConditionError`] that decides the result.
pub fn evaluate(expr: &Expr, env: &dyn Env) -> Result<bool, ConditionError> {
    match expr {
        Expr::Call(name, args) => call(env, name, args),
        Expr::Not(inner) => evaluate(inner, env).map(|value| !value),
        Expr::And(left, right) => {
            let (left, right) = (evaluate(left, env), evaluate(right, env));
            match (left, right) {
                (Ok(false), _) | (_, Ok(false)) => Ok(false),
                (Ok(true), Ok(true)) => Ok(true),
                (Err(error), _) | (_, Err(error)) => Err(error),
            }
        }
        Expr::Or(left, right) => {
            let (left, right) = (evaluate(left, env), evaluate(right, env));
            match (left, right) {
                (Ok(true), _) | (_, Ok(true)) => Ok(true),
                (Ok(false), Ok(false)) => Ok(false),
                (Err(error), _) | (_, Err(error)) => Err(error),
            }
        }
    }
}

/// Parse and evaluate in one step. An empty (or blank) condition is true.
///
/// # Errors
/// See [`parse`] and [`evaluate`].
pub fn eval_str(text: &str, env: &dyn Env) -> Result<bool, ConditionError> {
    if text.trim().is_empty() {
        return Ok(true);
    }
    evaluate(&parse(text)?, env)
}

#[cfg(test)]
mod tests {
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
}
