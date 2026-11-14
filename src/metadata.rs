//! Reader for the subset of the LOOT metadata (masterlist) YAML that matters for New Vegas.
//!
//! Syntax reference: <https://loot-api.readthedocs.io/en/latest/metadata/file_structure.html>.
//! Read here: `globals`, `groups`, `bash_tags` and `plugins` with `name`, `group`, `after`,
//! `req`, `inc`, `msg`, `tag`, `dirty` and `clean`. Everything else (`url`, `prelude`
//! replacement, unknown keys) is ignored. YAML anchors and aliases are resolved by the parser and YAML merge keys
//! (`<<`, which the real New Vegas masterlist uses for its message templates) are expanded
//! before reading: explicit keys win over merged ones, and earlier maps of a merge list win
//! over later ones.
//!
//! Only the English text of a message is kept. `subs` are substituted into `{0}`, `{1}`, ...

use regex::{Regex, RegexBuilder};
use std::collections::HashMap;
use std::fmt;
use yaml_rust2::parser::{Event, MarkedEventReceiver, Parser};
use yaml_rust2::scanner::Marker;
use yaml_rust2::yaml::Hash;
use yaml_rust2::{Yaml, YamlLoader};

/// Most nodes a masterlist may expand to once aliases are resolved. A file of a few hundred KiB, like the
/// real masterlist, expands to well under a million; this stops "billion laughs" documents, whose aliases
/// multiply on every level, before the loader clones them.
pub const MAX_EXPANDED_NODES: usize = 200_000;

/// Largest masterlist text that is read, in bytes (the real one is about 250 KB).
pub const MAX_MASTERLIST_BYTES: usize = 8 * 1024 * 1024;

/// Deepest nesting of `[` and `{` that is accepted (the real masterlist nests two levels).
pub const MAX_FLOW_DEPTH: usize = 64;

/// Deepest nesting of collections that the alias counter follows.
const MAX_COUNTED_DEPTH: usize = 256;

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
    /// Text appended to the message about the data (English).
    pub detail: Option<String>,
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

/// Counts the nodes of the document with every alias counted as the node it stands for.
#[derive(Default)]
struct ExpansionCounter {
    /// Nodes counted so far in each open collection.
    open: Vec<(usize, usize)>,
    /// Expanded size of each anchored node.
    anchors: HashMap<usize, usize>,
    total: usize,
    exceeded: bool,
}

impl ExpansionCounter {
    fn add(&mut self, size: usize) {
        self.total = self.total.saturating_add(size);
        if let Some(top) = self.open.last_mut() {
            top.1 = top.1.saturating_add(size);
        }
        self.exceeded |= self.total > MAX_EXPANDED_NODES;
    }

    fn close(&mut self) {
        if let Some((anchor, size)) = self.open.pop() {
            if anchor != 0 {
                self.anchors.insert(anchor, size);
            }
            if let Some(parent) = self.open.last_mut() {
                parent.1 = parent.1.saturating_add(size);
            }
        }
    }
}

impl MarkedEventReceiver for ExpansionCounter {
    fn on_event(&mut self, event: Event, _mark: Marker) {
        match event {
            Event::Scalar(_, _, anchor, _) => {
                self.add(1);
                if anchor != 0 {
                    self.anchors.insert(anchor, 1);
                }
            }
            Event::Alias(anchor) => {
                // The aliased node was counted where it was defined; every use counts it again.
                let size = self.anchors.get(&anchor).copied().unwrap_or(1);
                self.total = self.total.saturating_add(size);
                if let Some(top) = self.open.last_mut() {
                    top.1 = top.1.saturating_add(size);
                }
                self.exceeded |= self.total > MAX_EXPANDED_NODES;
            }
            Event::SequenceStart(anchor, _) | Event::MappingStart(anchor, _) => {
                self.total = self.total.saturating_add(1);
                if self.open.len() >= MAX_COUNTED_DEPTH {
                    self.exceeded = true;
                    return;
                }
                self.open.push((anchor, 1));
            }
            Event::SequenceEnd | Event::MappingEnd => self.close(),
            _ => {}
        }
    }
}

/// The number of nodes the document has once every alias is counted as the node it stands for. Refuses a
/// document that is too large, nests brackets too deeply or expands to more than [`MAX_EXPANDED_NODES`].
///
/// # Errors
/// [`MetadataError`] for invalid YAML or a document over one of the limits.
pub fn expanded_node_count(text: &str) -> Result<usize, MetadataError> {
    if text.len() > MAX_MASTERLIST_BYTES {
        return Err(MetadataError(format!(
            "the masterlist is {} bytes, more than the limit of {MAX_MASTERLIST_BYTES}",
            text.len()
        )));
    }
    let (mut depth, mut deepest) = (0usize, 0usize);
    for byte in text.bytes() {
        match byte {
            b'[' | b'{' => {
                depth += 1;
                deepest = deepest.max(depth);
            }
            b']' | b'}' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    if deepest > MAX_FLOW_DEPTH {
        return Err(MetadataError(format!(
            "YAML: brackets are nested more than {MAX_FLOW_DEPTH} levels deep"
        )));
    }
    let mut counter = ExpansionCounter::default();
    let mut parser = Parser::new_from_str(text);
    parser
        .load(&mut counter, true)
        .map_err(|error| MetadataError(format!("YAML: {error}")))?;
    if counter.exceeded {
        return Err(MetadataError(format!(
            "YAML: the aliases expand to more than {MAX_EXPANDED_NODES} nodes"
        )));
    }
    Ok(counter.total)
}

/// Expand YAML merge keys (`<<: *alias` or `<<: [*a, *b]`) in `node` and everything below it.
/// Work allowed for merge-key expansion: every node visited or copied counts, and the expansion stops the
/// moment the budget or the depth runs out, before anything more is cloned.
struct Budget {
    left: usize,
}

impl Budget {
    fn spend(&mut self, amount: usize) -> Result<(), MetadataError> {
        self.left = self.left.checked_sub(amount).ok_or_else(|| {
            MetadataError(format!(
                "YAML: merge keys expand to more than {} nodes",
                MAX_EXPANDED_NODES * 2
            ))
        })?;
        Ok(())
    }
}

fn expand_merge_keys(
    node: &Yaml,
    budget: &mut Budget,
    depth: usize,
) -> Result<Yaml, MetadataError> {
    if depth > MAX_COUNTED_DEPTH {
        return Err(MetadataError("YAML: nested too deeply".into()));
    }
    budget.spend(1)?;
    Ok(match node {
        Yaml::Array(items) => Yaml::Array(
            items
                .iter()
                .map(|item| expand_merge_keys(item, budget, depth + 1))
                .collect::<Result<_, _>>()?,
        ),
        Yaml::Hash(map) => {
            let mut out = Hash::new();
            let mut merged: Vec<Hash> = Vec::new();
            for (key, value) in map {
                if key.as_str() == Some("<<") {
                    let sources = match value {
                        Yaml::Array(items) => items.iter().collect(),
                        other => vec![other],
                    };
                    for source in sources {
                        if let Yaml::Hash(inner) = expand_merge_keys(source, budget, depth + 1)? {
                            merged.push(inner);
                        }
                    }
                } else {
                    out.insert(key.clone(), expand_merge_keys(value, budget, depth + 1)?);
                }
            }
            for inner in merged {
                budget.spend(inner.len())?;
                for (key, value) in inner {
                    if !out.contains_key(&key) {
                        out.insert(key, value);
                    }
                }
            }
            Yaml::Hash(out)
        }
        other => other.clone(),
    })
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
        detail: content_text(&node["detail"]),
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
    expanded_node_count(text)?;
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
    let mut budget = Budget {
        left: MAX_EXPANDED_NODES * 2,
    };
    let Some(root) = documents
        .first()
        .map(|document| expand_merge_keys(document, &mut budget, 0))
        .transpose()?
    else {
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
#[path = "metadata_tests.rs"]
mod tests;
