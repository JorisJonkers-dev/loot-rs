//! Reader for the subset of the LOOT metadata (masterlist) YAML that matters for New Vegas.
//!
//! Syntax reference: <https://loot-api.readthedocs.io/en/latest/metadata/file_structure.html>.
//! Read here: `globals`, `groups`, `bash_tags` and `plugins` with `name`, `group`, `after`,
//! `req`, `inc`, `msg`, `tag`, `dirty` and `clean`. Everything else (`url`, `prelude`
//! replacement, unknown keys) is ignored. YAML anchors and aliases are resolved by the parser;
//! YAML merge keys (`<<`) are not.
//!
//! Only the English text of a message is kept. `subs` are substituted into `{0}`, `{1}`, ...

use regex::{Regex, RegexBuilder};
use std::fmt;
use yaml_rust2::{Yaml, YamlLoader};

/// Name of the group every plugin is in unless the metadata says otherwise.
pub const DEFAULT_GROUP: &str = "default";

/// A problem that makes the metadata file unreadable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetadataError(pub String);

impl fmt::Display for MetadataError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for MetadataError {}

/// Message severity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageKind {
    /// A generic note.
    Say,
    /// A non-critical issue.
    Warn,
    /// A critical issue.
    Error,
}

impl MessageKind {
    /// The keyword used in the metadata.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Say => "say",
            Self::Warn => "warn",
            Self::Error => "error",
        }
    }
}

/// A message with its condition. `content` is the English text with `subs` applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    /// Severity.
    pub kind: MessageKind,
    /// English text, substitutions applied.
    pub content: String,
    /// Condition string; the message applies when it holds, or always when absent.
    pub condition: Option<String>,
}

/// A `file` data structure, used by `after`, `req` and `inc`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileRef {
    /// File name or path (never a regex).
    pub name: String,
    /// Text shown instead of the name in generated messages.
    pub display: Option<String>,
    /// Extra text appended to a generated error message.
    pub detail: Option<String>,
    /// The entry is used only when this holds.
    pub condition: Option<String>,
    /// The file's existence is recognised only when this also holds.
    pub constraint: Option<String>,
}

impl FileRef {
    /// Name for messages: `display` when set, otherwise the file name.
    #[must_use]
    pub fn label(&self) -> &str {
        self.display.as_deref().unwrap_or(&self.name)
    }
}

/// A suggested Bash Tag; a leading `-` in `name` means removal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tag {
    /// Tag name, with the `-` prefix when it is a removal.
    pub name: String,
    /// The tag is suggested only when this holds.
    pub condition: Option<String>,
}

/// Cleaning data (`dirty` / `clean`). Kept as data only; nothing here compares CRCs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cleaning {
    /// CRC-32 of the plugin the data refers to.
    pub crc: Option<u32>,
    /// Utility used to check the plugin.
    pub util: String,
    /// Identical-to-master records.
    pub itm: u32,
    /// Undeleted references.
    pub udr: u32,
    /// Deleted navmeshes.
    pub nav: u32,
}

/// A group definition after merging all definitions of the same name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    /// Case-sensitive group name.
    pub name: String,
    /// Description (last non-empty one wins).
    pub description: String,
    /// Groups this group loads after (union).
    pub after: Vec<String>,
}

/// How a plugin entry names its plugins.
#[derive(Debug, Clone)]
pub enum NamePattern {
    /// An exact file name, compared case-insensitively.
    Exact(String),
    /// A regular expression that must match the whole file name, case-insensitively.
    Regex(Regex),
}

impl NamePattern {
    /// True when `file_name` is matched by the pattern.
    #[must_use]
    pub fn matches(&self, file_name: &str) -> bool {
        match self {
            Self::Exact(name) => name.to_lowercase() == file_name.to_lowercase(),
            Self::Regex(regex) => regex.is_match(file_name),
        }
    }

    /// Source text of the pattern.
    #[must_use]
    pub fn text(&self) -> &str {
        match self {
            Self::Exact(name) => name,
            Self::Regex(regex) => regex.as_str(),
        }
    }
}

/// One element of `plugins`.
#[derive(Debug, Clone)]
pub struct PluginEntry {
    /// Which plugins the entry applies to.
    pub pattern: NamePattern,
    /// Group name, when set.
    pub group: Option<String>,
    /// Plugins this one must load after.
    pub after: Vec<FileRef>,
    /// Files this plugin requires.
    pub req: Vec<FileRef>,
    /// Files this plugin is incompatible with.
    pub inc: Vec<FileRef>,
    /// Messages, in file order.
    pub msg: Vec<Message>,
    /// Bash Tag suggestions.
    pub tag: Vec<Tag>,
    /// Dirty plugin data.
    pub dirty: Vec<Cleaning>,
    /// Clean plugin data.
    pub clean: Vec<Cleaning>,
}

/// The parsed metadata file.
#[derive(Debug, Clone, Default)]
pub struct Metadata {
    /// `bash_tags`.
    pub bash_tags: Vec<String>,
    /// `globals`.
    pub globals: Vec<Message>,
    /// `groups`, merged by name, with `default` always present first.
    pub groups: Vec<Group>,
    /// `plugins`, in file order.
    pub plugins: Vec<PluginEntry>,
    /// Entries that could not be used (for example a regex this engine cannot compile).
    pub warnings: Vec<String>,
}

fn scalar(node: &Yaml) -> Option<String> {
    match node {
        Yaml::String(text) | Yaml::Real(text) => Some(text.clone()),
        Yaml::Integer(number) => Some(number.to_string()),
        Yaml::Boolean(value) => Some(value.to_string()),
        _ => None,
    }
}

fn field(node: &Yaml, key: &str) -> Option<String> {
    scalar(&node[key])
}

fn list(node: &Yaml) -> &[Yaml] {
    node.as_vec().map_or(&[], Vec::as_slice)
}

/// `content` may be a string or a list of `{lang, text}`; keep the English one (or the first).
fn content_text(node: &Yaml) -> Option<String> {
    if let Some(text) = scalar(node) {
        return Some(text);
    }
    let items = node.as_vec()?;
    let english = items.iter().find(|item| {
        field(item, "lang").is_some_and(|lang| lang == "en" || lang.starts_with("en_"))
    });
    field(english.or_else(|| items.first())?, "text")
}

fn substitute(mut text: String, subs: &[Yaml]) -> String {
    for (index, value) in subs.iter().enumerate() {
        if let Some(value) = scalar(value) {
            text = text.replace(&format!("{{{index}}}"), &value);
        }
    }
    text
}

fn parse_message(node: &Yaml, context: &str) -> Result<Message, MetadataError> {
    let kind = match field(node, "type").as_deref() {
        Some("say") => MessageKind::Say,
        Some("warn") => MessageKind::Warn,
        Some("error") => MessageKind::Error,
        other => {
            return Err(MetadataError(format!(
                "{context}: message type must be say, warn or error, found {other:?}"
            )));
        }
    };
    let text = content_text(&node["content"])
        .ok_or_else(|| MetadataError(format!("{context}: message has no content")))?;
    Ok(Message {
        kind,
        content: substitute(text, list(&node["subs"])),
        condition: field(node, "condition"),
    })
}

fn parse_messages(node: &Yaml, context: &str) -> Result<Vec<Message>, MetadataError> {
    list(node)
        .iter()
        .map(|item| parse_message(item, context))
        .collect()
}

fn parse_file(node: &Yaml, context: &str) -> Result<FileRef, MetadataError> {
    if let Some(name) = scalar(node) {
        return Ok(FileRef {
            name,
            display: None,
            detail: None,
            condition: None,
            constraint: None,
        });
    }
    let name = field(node, "name")
        .ok_or_else(|| MetadataError(format!("{context}: file entry has no name")))?;
    Ok(FileRef {
        name,
        display: field(node, "display"),
        detail: content_text(&node["detail"]),
        condition: field(node, "condition"),
        constraint: field(node, "constraint"),
    })
}

fn parse_files(node: &Yaml, context: &str) -> Result<Vec<FileRef>, MetadataError> {
    list(node)
        .iter()
        .map(|item| parse_file(item, context))
        .collect()
}

fn parse_tag(node: &Yaml, context: &str) -> Result<Tag, MetadataError> {
    if let Some(name) = scalar(node) {
        return Ok(Tag {
            name,
            condition: None,
        });
    }
    Ok(Tag {
        name: field(node, "name")
            .ok_or_else(|| MetadataError(format!("{context}: tag has no name")))?,
        condition: field(node, "condition"),
    })
}

fn count(node: &Yaml, key: &str) -> u32 {
    node[key]
        .as_i64()
        .and_then(|value| u32::try_from(value).ok())
        .unwrap_or(0)
}

fn parse_cleaning(node: &Yaml) -> Cleaning {
    Cleaning {
        crc: node["crc"]
            .as_i64()
            .and_then(|value| u32::try_from(value).ok()),
        util: field(node, "util").unwrap_or_default(),
        itm: count(node, "itm"),
        udr: count(node, "udr"),
        nav: count(node, "nav"),
    }
}

/// Characters that make a plugin name a regular expression.
const REGEX_MARKERS: [char; 5] = [':', '\\', '*', '?', '|'];

/// True when a plugin name or path argument is to be read as a regular expression.
#[must_use]
pub fn looks_like_regex(text: &str) -> bool {
    text.contains(REGEX_MARKERS)
}

/// Compile `pattern` anchored at both ends, case-insensitive and Unicode-aware.
///
/// # Errors
/// Returns the compile error text, for example for look-around, which this engine lacks.
pub fn compile_whole_name(pattern: &str) -> Result<Regex, String> {
    RegexBuilder::new(&format!("^(?:{pattern})$"))
        .case_insensitive(true)
        .unicode(true)
        .build()
        .map_err(|error| error.to_string())
}

fn merge_group(groups: &mut Vec<Group>, incoming: Group) {
    if let Some(existing) = groups.iter_mut().find(|group| group.name == incoming.name) {
        if !incoming.description.is_empty() {
            existing.description = incoming.description;
        }
        for name in incoming.after {
            if !existing.after.contains(&name) {
                existing.after.push(name);
            }
        }
    } else {
        groups.push(incoming);
    }
}

/// Parse a masterlist (or userlist) from its YAML text.
///
/// # Errors
/// Returns [`MetadataError`] for invalid YAML or an entry missing a required key. A regular
/// expression that cannot be compiled does not fail the file: the entry is skipped and a
/// warning is recorded in [`Metadata::warnings`].
pub fn parse(text: &str) -> Result<Metadata, MetadataError> {
    let documents =
        YamlLoader::load_from_str(text).map_err(|error| MetadataError(format!("YAML: {error}")))?;
    let mut metadata = Metadata::default();
    merge_group(
        &mut metadata.groups,
        Group {
            name: DEFAULT_GROUP.to_string(),
            description: String::new(),
            after: Vec::new(),
        },
    );
    let Some(root) = documents.first() else {
        return Ok(metadata);
    };
    if root.as_hash().is_none() {
        return Err(MetadataError("the root of the file is not a map".into()));
    }

    metadata.bash_tags = list(&root["bash_tags"]).iter().filter_map(scalar).collect();
    metadata.globals = parse_messages(&root["globals"], "globals")?;
    for node in list(&root["groups"]) {
        let name = field(node, "name")
            .ok_or_else(|| MetadataError("groups: a group has no name".into()))?;
        merge_group(
            &mut metadata.groups,
            Group {
                name,
                description: field(node, "description").unwrap_or_default(),
                after: list(&node["after"]).iter().filter_map(scalar).collect(),
            },
        );
    }
    for node in list(&root["plugins"]) {
        let name = field(node, "name")
            .ok_or_else(|| MetadataError("plugins: an entry has no name".into()))?;
        let context = format!("plugin {name}");
        let pattern = if looks_like_regex(&name) {
            match compile_whole_name(&name) {
                Ok(regex) => NamePattern::Regex(regex),
                Err(error) => {
                    metadata.warnings.push(format!(
                        "{context}: regular expression skipped, not supported by this engine: {error}"
                    ));
                    continue;
                }
            }
        } else {
            NamePattern::Exact(name)
        };
        metadata.plugins.push(PluginEntry {
            pattern,
            group: field(node, "group"),
            after: parse_files(&node["after"], &context)?,
            req: parse_files(&node["req"], &context)?,
            inc: parse_files(&node["inc"], &context)?,
            msg: parse_messages(&node["msg"], &context)?,
            tag: list(&node["tag"])
                .iter()
                .map(|item| parse_tag(item, &context))
                .collect::<Result<_, _>>()?,
            dirty: list(&node["dirty"]).iter().map(parse_cleaning).collect(),
            clean: list(&node["clean"]).iter().map(parse_cleaning).collect(),
        });
    }
    Ok(metadata)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
prelude:
  - &thanks
    type: say
    content: 'Thanks'
bash_tags:
  - Relev
globals:
  - *thanks
  - type: warn
    content:
      - lang: de
        text: 'Hallo'
      - lang: en
        text: 'Use {0} {1}'
    subs: ['this', 'mod']
    condition: 'file("a.esp")'
groups:
  - name: 'Late'
    after: ['default']
  - name: 'Late'
    description: 'later'
    after: ['default', 'Mid']
plugins:
  - name: 'A.esp'
    group: 'Late'
    after: ['B.esp', {name: 'C.esp', condition: 'file("c")'}]
    req:
      - name: 'D.esp'
        display: '[D](http://x)'
        detail: 'get it'
    inc: ['E.esp']
    tag: ['Relev', {name: '-Delev', condition: 'file("f")'}]
    dirty:
      - crc: 0x1234ABCD
        util: 'FNVEdit'
        itm: 3
    clean:
      - crc: 0x1
        util: 'FNVEdit'
  - name: 'Bad(?=x)\.esp'
  - name: 'Cat.*\.esp'
    msg:
      - type: say
        content: 'regex entry'
"#;

    fn sample() -> Metadata {
        parse(SAMPLE).expect("parses")
    }

    #[test]
    fn aliases_in_globals_are_resolved() {
        let metadata = sample();
        assert_eq!(metadata.globals.len(), 2);
        assert_eq!(metadata.globals[0].content, "Thanks");
        assert_eq!(metadata.globals[0].kind, MessageKind::Say);
    }

    #[test]
    fn english_content_is_chosen_and_subs_are_applied() {
        let message = &sample().globals[1];
        assert_eq!(message.content, "Use this mod");
        assert_eq!(message.condition.as_deref(), Some("file(\"a.esp\")"));
    }

    #[test]
    fn groups_merge_by_name_and_default_exists() {
        let metadata = sample();
        assert_eq!(metadata.groups[0].name, DEFAULT_GROUP);
        let late = metadata.groups.iter().find(|group| group.name == "Late");
        let late = late.expect("Late group");
        assert_eq!(late.description, "later");
        assert_eq!(late.after, ["default", "Mid"]);
    }

    #[test]
    fn plugin_entry_fields_are_read() {
        let metadata = sample();
        let entry = &metadata.plugins[0];
        assert!(entry.pattern.matches("a.ESP"));
        assert_eq!(entry.group.as_deref(), Some("Late"));
        assert_eq!(entry.after.len(), 2);
        assert_eq!(entry.after[1].condition.as_deref(), Some("file(\"c\")"));
        assert_eq!(entry.req[0].label(), "[D](http://x)");
        assert_eq!(entry.req[0].detail.as_deref(), Some("get it"));
        assert_eq!(entry.inc[0].name, "E.esp");
        assert_eq!(entry.tag[1].name, "-Delev");
        assert_eq!(entry.dirty[0].crc, Some(0x1234_ABCD));
        assert_eq!(entry.dirty[0].itm, 3);
        assert_eq!(entry.clean[0].crc, Some(1));
    }

    #[test]
    fn regex_names_match_whole_names_case_insensitively() {
        let metadata = sample();
        let entry = metadata.plugins.last().expect("regex entry");
        assert!(entry.pattern.matches("CATS.esp"));
        assert!(!entry.pattern.matches("xCats.esp"));
        assert!(!entry.pattern.matches("Cats.esp.bak"));
    }

    #[test]
    fn a_regex_this_engine_cannot_compile_is_skipped_with_a_warning() {
        let metadata = sample();
        assert_eq!(metadata.plugins.len(), 2);
        assert_eq!(metadata.warnings.len(), 1);
        assert!(metadata.warnings[0].contains("Bad(?=x)"));
    }

    #[test]
    fn bad_message_type_is_an_error() {
        let error = parse("globals:\n  - type: shout\n    content: x\n").expect_err("fails");
        assert!(error.0.contains("say, warn or error"));
    }

    #[test]
    fn entry_without_name_is_an_error() {
        assert!(parse("plugins:\n  - group: x\n").is_err());
    }

    #[test]
    fn empty_and_non_map_files() {
        assert!(parse("").expect("empty is fine").plugins.is_empty());
        assert!(parse("- a\n- b\n").is_err());
        assert!(parse("a: [unclosed").is_err());
    }
}
