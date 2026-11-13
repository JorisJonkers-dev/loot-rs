//! Read-only evaluation of LOOT masterlist rules over a Fallout: New Vegas load order.
//!
//! * [`plugin`]: plugin header reader (separate MIT module, see `NOTICE.md`).
//! * [`metadata`]: masterlist YAML reader for the subset that matters for New Vegas.
//! * [`condition`]: parser and evaluator for LOOT condition strings.
//! * [`env`]: the file system and load order a condition is evaluated against.
//! * `exe_version` (cargo feature `executable-versions`): `version()` and `product_version()` on executables.
//! * [`check`]: applies the metadata to a load order and produces a [`check::Report`].
//! * [`cli`]: argument parsing and the `check` command.
//! * [`render`]: plain-text output of a report.

pub mod check;
pub mod cli;
pub mod condition;
pub mod env;
#[cfg(feature = "executable-versions")]
pub mod exe_version;
pub mod metadata;
pub mod plugin;
pub mod render;

#[cfg(test)]
mod support_tests;
