//! Parser and evaluator for LOOT condition strings.
//!
//! Syntax reference: <https://loot-api.readthedocs.io/en/latest/metadata/conditions.html>.
//!
//! Supported functions: `file`, `active`, `many`, `many_active`, `file_size`, `readable`,
//! `is_master`, `checksum` and (on plugins) `version`, combined with `and`, `or`, `not` and parentheses (`not` binds
//! tightest, then `and`, then `or`). Functions that need executable resources, file name versions
//! or plugin descriptions (`version` on a file that is not a plugin without the `executable-versions` feature,
//! `product_version`, `filename_version`, `description_contains`, `is_executable`) parse, but evaluate to
//! [`ConditionError::Unsupported`]: they are never guessed.
//!
//! A condition may be at most [`MAX_CONDITION_BYTES`] long, nest at most [`MAX_NESTING`] parentheses or `not`s and hold at most [`MAX_NODES`]
//! operators and calls, so that parsing, evaluating and dropping it cannot exhaust the stack on
//! hostile input.
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

/// Deepest nesting of parentheses and `not` that is accepted.
pub const MAX_NESTING: usize = 64;

/// Longest condition text that is parsed, in bytes (the longest in the real masterlist is 256).
pub const MAX_CONDITION_BYTES: usize = 16 * 1024;

/// Most operators and function calls that one condition may hold.
pub const MAX_NODES: usize = 2_000;

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

impl fmt::Display for Arg {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Str(text) => write!(f, "\"{text}\""),
            Self::Word(text) => f.write_str(text),
        }
    }
}

/// The canonical text of a condition: operands are parenthesised only where the grammar needs it (`not` binds
/// tighter than `and`, which binds tighter than `or`, and both are left-associative), so
/// `parse(&expr.to_string())` gives back `expr` and a long chain of `or`s stays one level deep instead of
/// growing a parenthesis per operator, which would pass the nesting limit on the way back in.
impl fmt::Display for Expr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Call(name, args) => {
                write!(f, "{name}(")?;
                for (index, arg) in args.iter().enumerate() {
                    if index > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{arg}")?;
                }
                f.write_str(")")
            }
            Self::Not(inner) => {
                f.write_str("not ")?;
                Operand::new(inner, self.rank(), false).fmt(f)
            }
            Self::And(left, right) => {
                Operand::new(left, self.rank(), false).fmt(f)?;
                f.write_str(" and ")?;
                Operand::new(right, self.rank(), true).fmt(f)
            }
            Self::Or(left, right) => {
                Operand::new(left, self.rank(), false).fmt(f)?;
                f.write_str(" or ")?;
                Operand::new(right, self.rank(), true).fmt(f)
            }
        }
    }
}

impl Expr {
    /// How tightly the expression binds: calls and `not` 3, `and` 2, `or` 1.
    const fn rank(&self) -> u8 {
        match self {
            Self::Call(..) | Self::Not(_) => 3,
            Self::And(..) => 2,
            Self::Or(..) => 1,
        }
    }
}

/// An operand of an operator of rank `parent`: bare when it binds at least as tightly, and, on the right of a
/// left-associative operator, only when strictly tighter.
struct Operand<'a> {
    expr: &'a Expr,
    parenthesised: bool,
}

impl<'a> Operand<'a> {
    fn new(expr: &'a Expr, parent: u8, right: bool) -> Self {
        let binary = matches!(expr, Expr::And(..) | Expr::Or(..));
        let parenthesised = expr.rank() < parent || (right && binary && expr.rank() == parent);
        Self {
            expr,
            parenthesised,
        }
    }
}

impl fmt::Display for Operand<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.parenthesised {
            write!(f, "({})", self.expr)
        } else {
            write!(f, "{}", self.expr)
        }
    }
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
    /// Evaluate `version(...)` or `product_version(...)` (`name`) on a file. The default answers
    /// [`ConditionError::Unsupported`]: an environment says otherwise only when it can read the file's version
    /// (see the `executable-versions` cargo feature).
    ///
    /// # Errors
    /// [`ConditionError::Unsupported`] when the environment cannot, other errors when the file cannot be read.
    fn version_call(&self, name: &str, args: &[Arg]) -> Result<bool, ConditionError> {
        let _ = args;
        Err(ConditionError::Unsupported(name.to_string()))
    }
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
    nesting: usize,
    nodes: usize,
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

    fn node(&mut self) -> Result<(), ConditionError> {
        self.nodes += 1;
        if self.nodes > MAX_NODES {
            return Err(ConditionError::Syntax(format!(
                "condition has more than {MAX_NODES} operators and calls"
            )));
        }
        Ok(())
    }

    fn expr(&mut self) -> Result<Expr, ConditionError> {
        let mut left = self.and_expr()?;
        while self.peek_keyword("or") {
            self.at += 1;
            self.node()?;
            left = Expr::Or(Box::new(left), Box::new(self.and_expr()?));
        }
        Ok(left)
    }

    fn and_expr(&mut self) -> Result<Expr, ConditionError> {
        let mut left = self.unary()?;
        while self.peek_keyword("and") {
            self.at += 1;
            self.node()?;
            left = Expr::And(Box::new(left), Box::new(self.unary()?));
        }
        Ok(left)
    }

    fn unary(&mut self) -> Result<Expr, ConditionError> {
        self.nesting += 1;
        if self.nesting > MAX_NESTING {
            return Err(ConditionError::Syntax(format!(
                "condition is nested more than {MAX_NESTING} levels deep"
            )));
        }
        let result = self.unary_inner();
        self.nesting -= 1;
        result
    }

    fn unary_inner(&mut self) -> Result<Expr, ConditionError> {
        self.node()?;
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
    if text.len() > MAX_CONDITION_BYTES {
        return Err(ConditionError::Syntax(format!(
            "condition is longer than {MAX_CONDITION_BYTES} bytes"
        )));
    }
    let mut parser = Parser {
        tokens: tokenize(text)?,
        at: 0,
        nesting: 0,
        nodes: 0,
    };
    let expr = parser.expr()?;
    if parser.at < parser.tokens.len() {
        return Err(ConditionError::Syntax(
            "trailing text after the condition".into(),
        ));
    }
    Ok(expr)
}

/// Functions that exist in LOOT but are not implemented here. `version` and `product_version` are the
/// exception that the `executable-versions` feature makes for executables and DLLs. `version` on a plugin is
/// evaluated from its description in every build ([`crate::version`]); `product_version` on a plugin stays
/// unsupported.
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
        "version" | "product_version" => env.version_call(name, args),
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
#[path = "condition_tests.rs"]
mod tests;
