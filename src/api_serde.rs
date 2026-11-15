//! `Serialize` for the output of `api::sort` (cargo feature `serde`), with the keys and values of
//! `loot-rs sort --json` (`render::render_sort_json`) except the leading `schema_version` and `contract`. Written by
//! hand so that no derive macro is needed and the shape is exactly the CLI's; `tests/serde.rs` compares the two.

use serde::ser::{Serialize, SerializeSeq, SerializeStruct, Serializer};

use crate::api::{
    CleaningStatus, DirtyInfo, Finding, Message, MoveExplanation, OrderingRule, PluginMove,
    PluginResult, SortMode, SortOutput,
};

impl Serialize for Message {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut out = serializer.serialize_struct("Message", 2)?;
        out.serialize_field("type", self.kind.label())?;
        out.serialize_field("text", self.text.trim())?;
        out.end()
    }
}

impl Serialize for Finding {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut out = serializer.serialize_struct("Finding", 3)?;
        out.serialize_field("kind", self.kind.label())?;
        out.serialize_field("hard", &self.hard)?;
        out.serialize_field("text", &self.text)?;
        out.end()
    }
}

impl Serialize for PluginResult {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut out = serializer.serialize_struct("PluginResult", 10)?;
        out.serialize_field("name", &self.name)?;
        out.serialize_field("position", &self.position)?;
        out.serialize_field("active", &self.active)?;
        out.serialize_field("group", &self.group)?;
        out.serialize_field("messages", &self.messages)?;
        out.serialize_field("generated_messages", &self.generated_messages)?;
        out.serialize_field("bash_tags", &self.bash_tags)?;
        out.serialize_field("tags_add", &self.tags_added)?;
        out.serialize_field("tags_remove", &self.tags_removed)?;
        out.serialize_field("findings", &self.findings)?;
        out.end()
    }
}

impl Serialize for DirtyInfo {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut out = serializer.serialize_struct("DirtyInfo", 7)?;
        out.serialize_field("plugin", &self.plugin)?;
        out.serialize_field("crc", &self.crc)?;
        let status = match self.status {
            CleaningStatus::Dirty => "dirty",
            CleaningStatus::Clean => "clean",
        };
        out.serialize_field("status", status)?;
        out.serialize_field("utility", &self.utility)?;
        out.serialize_field("itm", &self.itm)?;
        out.serialize_field("udr", &self.udr)?;
        out.serialize_field("nav", &self.nav)?;
        out.end()
    }
}

impl Serialize for OrderingRule {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut out = serializer.serialize_struct("OrderingRule", 3)?;
        out.serialize_field("before", &self.before)?;
        out.serialize_field("after", &self.after)?;
        out.serialize_field("source", self.source.label())?;
        out.end()
    }
}

impl Serialize for PluginMove {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut out = serializer.serialize_struct("PluginMove", 6)?;
        out.serialize_field("plugin", &self.plugin)?;
        out.serialize_field("from", &self.from)?;
        out.serialize_field("to", &self.to)?;
        out.serialize_field("reason", self.reason.label())?;
        out.serialize_field("other", &self.other)?;
        out.serialize_field("indirect", &self.indirect)?;
        out.serialize_field("reasons", &self.reasons)?;
        out.end()
    }
}

impl Serialize for MoveExplanation {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        struct Via<'a>(&'a str, &'a str);
        impl Serialize for Via<'_> {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                let mut out = serializer.serialize_struct("Via", 2)?;
                out.serialize_field("before", self.0)?;
                out.serialize_field("after", self.1)?;
                out.end()
            }
        }
        let mut out = serializer.serialize_struct("MoveExplanation", 7)?;
        out.serialize_field("reason", self.reason.label())?;
        out.serialize_field("other", &self.other)?;
        out.serialize_field("later", &self.later)?;
        out.serialize_field("indirect", &self.indirect)?;
        if let Some((first, second)) = &self.via {
            out.serialize_field("via", &Via(first, second))?;
        }
        if let Some((first, second)) = &self.groups {
            out.serialize_field("groups", &[first, second])?;
        }
        if let Some((first, second)) = self.counts {
            out.serialize_field("counts", &[first, second])?;
        }
        out.end()
    }
}

/// `dropped_group_rules` as objects `{later, earlier}`.
struct Dropped<'a>(&'a [(String, String)]);

impl Serialize for Dropped<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        struct Rule<'a>(&'a str, &'a str);
        impl Serialize for Rule<'_> {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                let mut out = serializer.serialize_struct("DroppedGroupRule", 2)?;
                out.serialize_field("later", self.0)?;
                out.serialize_field("earlier", self.1)?;
                out.end()
            }
        }
        let mut out = serializer.serialize_seq(Some(self.0.len()))?;
        for (later, earlier) in self.0 {
            out.serialize_element(&Rule(later, earlier))?;
        }
        out.end()
    }
}

impl Serialize for SortOutput {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut out = serializer.serialize_struct("SortOutput", 14)?;
        out.serialize_field("sorted", &self.order)?;
        out.serialize_field("general_messages", &self.general_messages)?;
        out.serialize_field(
            "generated_general_messages",
            &self.generated_general_messages,
        )?;
        out.serialize_field("plugins", &self.plugins)?;
        out.serialize_field("dirty", &self.dirty)?;
        out.serialize_field("rules", &self.ordering_rules)?;
        out.serialize_field("dropped_group_rules", &Dropped(&self.dropped_group_rules))?;
        out.serialize_field("not_installed", &self.not_installed)?;
        out.serialize_field("warnings", &self.warnings)?;
        if self.mode == SortMode::MinimalChange {
            out.serialize_field("mode", self.mode.label())?;
            out.serialize_field("moves", &self.moves)?;
        }
        out.serialize_field("explanations", &self.explanations)?;
        out.serialize_field("hard_violations", &self.hard_violations())?;
        out.end()
    }
}
