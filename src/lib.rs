//! Read-only evaluation of LOOT masterlist rules over a Fallout: New Vegas load order.
//!
//! * [`api`]: the library API for a mod manager (sort or check a load order, messages, cleaning data, copyable
//!   errors). Start here.
//! * [`plugin`]: plugin header reader (separate MIT module, see `NOTICE.md`).
//! * [`metadata`]: masterlist YAML reader for the subset that matters for New Vegas.
//! * [`condition`]: parser and evaluator for LOOT condition strings.
//! * [`env`]: the file system and load order a condition is evaluated against.
//! * [`exe_version`]: the bounded PE reader; `is_executable()`, and with the cargo feature `executable-versions`
//!   `version()` and `product_version()` on executables.
//! * [`version`]: a plugin's version from its description, and LOOT's comparison of versions.
//! * [`check`]: applies the metadata to a load order and produces a [`check::Report`].
//! * [`cli`]: argument parsing and the `check` command.
//! * [`archives`]: the BSA archives a plugin loads and the files they hold, for the overlap between plugins.
//! * [`overlap`]: sorted lists of ids and which plugins share one.
//! * [`parallel`]: work spread over threads, with results in input order.
//! * [`records`]: the records of a plugin, for the overlap between plugins.
//! * [`render`]: plain-text output of a report.

pub mod api;
#[cfg(feature = "serde")]
mod api_serde;
pub mod archives;
pub mod check;
pub mod cli;
pub mod condition;
pub mod env;
pub mod exe_version;
pub mod explain;
mod load;
pub mod metadata;
pub mod overlap;
pub mod parallel;
pub mod plugin;
pub mod records;
pub mod render;
pub mod sha1;
pub mod sort;
pub mod validate;
pub mod version;

#[cfg(test)]
mod support_tests;

/// The ids of the feature catalogue (`catalogue/`), written by build.rs, for tests to tag what they prove.
#[cfg(test)]
mod tally {
    include!(concat!(env!("OUT_DIR"), "/tally_ids.rs"));
}
