# Library API

`loot_rs::api` is the part of the crate meant to be consumed by another program, for example a mod manager. The
other modules (`archives`, `check`, `cli`, `condition`, `env`, `metadata`, `overlap`, `plugin`, `records`, `render`, `sort`, `validate`) are the implementation
behind it and the command line; they are public so that tests and the fuzz targets can reach them, and they carry no
stability promise.

```rust
use loot_rs::api::{Masterlist, PluginSlot, SortOptions, sort};
use std::path::Path;

let masterlist = Masterlist::from_file(Path::new("masterlist.yaml"))?;
let load_order = vec![
    PluginSlot::new("FalloutNV.esm", true),
    PluginSlot::new("MyMod.esp", true),
];
let result = sort(&load_order, &masterlist, Path::new("Data"), &SortOptions::default())?;
```

## Command line

`loot-rs sort --masterlist <file> --plugins <plugins.txt> --data <Data> [--strict-groups] [--json]` runs the same code: the
text is the order with what applies to each plugin, `--json` is one line with `sorted`, `plugins`, `general_messages`,
`dirty`, `rules`, `dropped_group_rules`, `not_installed`, `warnings` and `hard_violations`. Exit code 0, or 1 when a
finding is hard, or 2 when the rules contradict each other or an input cannot be read.

## `sort(load_order, masterlist, data_dir, options) -> Result<SortOutput, SortError>`

| Argument | Meaning |
| --- | --- |
| `load_order: &[PluginSlot]` | The current order, each plugin with its file name and whether it is active |
| `masterlist: &Masterlist` | A masterlist read once with `Masterlist::parse(text)` or `Masterlist::from_file(path)` (8 MiB limit, aliases bounded), reusable |
| `data_dir: &Path` | The game's `Data` directory; condition paths are relative to it, `..` reaches the game directory |
| `options: &SortOptions` | `strict_groups` (see [GROUPS.md](GROUPS.md)); default is soft |

`SortOutput`:

| Field | Content |
| --- | --- |
| `order` | The installed plugins in sorted order, named as given |
| `ordering_rules` | The rules the order honours: `before`, `after` and `source` (`header-master`, `after`, `req`, `group`); to explain a move, or to check the order against another tool's. Group rules are listed when the final order satisfies them |
| `plugins` | One `PluginResult` per plugin of `order`, in that order: `position`, `active`, `group`, `messages` (kind and text; the masterlist's), `generated_messages` (the ones LOOT generates from the install, as Markdown), `bash_tags` (from the description), `tags_added`, `tags_removed`, `findings` (kind, text, `hard`) |
| `general_messages` | Masterlist messages that belong to no plugin |
| `generated_general_messages` | Messages LOOT generates about the whole load order (more than 255 active plugins) |
| `dirty` | `DirtyInfo` (plugin, CRC-32, dirty or clean, utility, ITM, UDR, NAV) where an installed file's CRC-32 is on the masterlist's `dirty` or `clean` list. Only files that have such data are read |
| `not_installed` | Plugins of the load order that are not in `Data`; they are not in `order`, the caller places them |
| `dropped_group_rules` | Group rules `(later, earlier)` that the final order does not satisfy: the sorter left the edge out because it would have closed a cycle |
| `warnings` | Skipped things: unreadable plugin headers, duplicate names, an ordering rule whose condition could not be evaluated |
| `changed(&load_order)` | Whether the order differs from the one given |

`SortError`: `DataDirectory`, `TooManyPlugins` (more than 1024 installed plugins), `Conflict { first, second }` (two
rules that cannot both hold: the first two plugins of the cycle), `MasterOrder { plugin, master }` (a plugin without
the master flag that has to load before one with it), `GroupCycle(groups)` (groups that load after each other),
`TooMuchWork`, `Internal(text)` (a check inside the sorter failed: a bug). A masterlist condition that cannot be evaluated is not an error: the item is left
out and reported.

## What the sort does

The algorithm is libloot's (see [COMPATIBILITY.md](COMPATIBILITY.md) for what is ported and what is not):

1. Plugins are vertices in the byte order of their names; plugins with the master flag and the others are sorted in
   two separate graphs and joined, masters first. A rule that wants a plugin without the flag before one with it is a
   `MasterOrder` error.
2. Edges from the header masters, the masterlist's `after` and `req` plugins (when their `condition` and `constraint`
   hold) and the game's hard-coded first plugin (`FalloutNV.esm`). A cycle among them is a `Conflict` error.
3. Edges from groups: a plugin in a group loads before the plugins of groups that load after it, each edge only when it
   closes no cycle (found with libloot's depth-first search order).
4. Overlap edges: of two plugins that have a record in common, the one that overrides more records loads first (the
   record ids are read from the plugin files); failing that, of two that load a file with the same path from their
   BSA archives, the one that loads more files. The plugin files are read for this, and the archives' tables.
5. Tie-break edges that keep the current load order wherever nothing above forbids it, and a topological sort. A plugin
   therefore moves only for a reason, and sorting a sorted order changes nothing.

Not supported: the userlist. The messages LOOT's application generates when it checks an install are returned apart from the masterlist's (`generated_messages`); `docs/COMPATIBILITY.md` lists which.

## Guarantees

* Read only: nothing under `Data` or anywhere else is written.
* Deterministic: the same arguments and files give the same result; which group rules are dropped does not depend on
  the order given.
* Bounded: see [RESOURCE-LIMITS.md](RESOURCE-LIMITS.md) (at most 1024 installed plugins, bit-matrix work limited to
  400 million steps, no whole plugin file held in memory for a checksum). Limits are errors, never panics.
* Honest about gaps: an unimplemented condition function leaves its item out and is reported, never guessed.

## Versioning

* The crate follows semantic versioning; while it is 0.x a minor release may break the API and a patch release never
  does. The release pull requests are made by release-please from the Conventional Commit titles, so a breaking
  change is written as `feat!:` or has a `BREAKING CHANGE:` footer.
* Structs in `loot_rs::api` are `#[non_exhaustive]`: build them with `new` or `Default` and read fields freely; a
  field can be added in a minor release.
* Enums in `loot_rs::api` are `#[non_exhaustive]`: a variant can be added in a minor release, so keep a `_` arm.
* Cargo features are additive. `executable-versions` (see the README) changes only which conditions can be evaluated.
* The tests in `tests/api.rs` use only `loot_rs::api` and `loot_rs::plugin::synthetic_plugin` from outside the crate; a
  change that breaks them is a breaking change.

## Not in the API yet

Writing `plugins.txt` or `loadorder.txt`, userlists, metadata for the user's own plugins, localised messages other than
English, `version()` on plugins, and the other things in [COMPATIBILITY.md](COMPATIBILITY.md).

## Semver

The crate is 0.x. Moving from the metadata-only sorter to libloot's algorithm changes the order `sort` returns for the
same input (a plugin that is later in the given order stays later wherever the rules allow, where the old sorter pulled
earlier ones forward), adds the `SortError` variants `GroupCycle` and `Internal` (the enum is `#[non_exhaustive]`, so
callers already have a wildcard arm), and replaces the `sort` module's functions (`sort::order` is now `sort::sort`
over `PluginInput`s; that module is public for tests and carries no promise). The `api` module's signatures are
unchanged, but `PluginResult` and `SortOutput` (both `#[non_exhaustive]`) gain fields (`generated_messages`, `bash_tags`, `generated_general_messages`), and the JSON of `loot-rs sort --json` gains the same keys. Release as a minor version.

## JSON schema version

`loot-rs check --json` and `loot-rs sort --json` write an integer `schema_version` as the first key (`api::SCHEMA_VERSION`, now 1). It rises when a key is removed, renamed or changes meaning; a new key does not raise it, so a consumer ignores keys it does not know.

## Helpers for a mod manager

* `SortOutput::order_with_not_installed(&load_order)`: the whole load order with the not-installed plugins put back, each right after the installed plugin that preceded it in the given order (first when none did).
* `MessageKind::label()` and `FindingKind::label()`: the words the text and JSON output use.
* `api::read_plugin_header(path)`, with `api::PluginHeader` and `api::PluginError`: the header of one plugin (masters, flags, author, description, record count). `PluginHeader` is `#[non_exhaustive]`.
