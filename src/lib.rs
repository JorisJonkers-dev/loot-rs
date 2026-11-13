//! Read-only evaluation of LOOT masterlist rules over a Fallout: New Vegas load order.
//!
//! * [`plugin`]: plugin header reader (separate MIT module, see `NOTICE.md`).
//! * [`metadata`]: masterlist YAML reader for the subset that matters for New Vegas.
//! * [`condition`]: parser and evaluator for LOOT condition strings.
//! * [`env`]: the file system and load order a condition is evaluated against.
//! * [`check`]: applies the metadata to a load order and produces a [`check::Report`].

pub mod condition;
pub mod env;
pub mod metadata;
pub mod plugin;
