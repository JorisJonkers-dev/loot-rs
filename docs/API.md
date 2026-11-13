# Library API

`loot_rs::api` is the part of the crate meant to be consumed by another program, for example a mod manager. The
other modules (`check`, `condition`, `env`, `metadata`, `plugin`, `render`, `sort`, `cli`) are the implementation
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
| `plugins` | One `PluginResult` per plugin of `order`, in that order: `position`, `active`, `group`, `messages` (kind and text), `tags_added`, `tags_removed`, `findings` (kind, text, `hard`) |
| `general_messages` | Masterlist messages that belong to no plugin |
| `dirty` | `DirtyInfo` (plugin, CRC-32, dirty or clean, utility, ITM, UDR, NAV) where an installed file's CRC-32 is on the masterlist's `dirty` or `clean` list. Only files that have such data are read |
| `not_installed` | Plugins of the load order that are not in `Data`; they are not in `order`, the caller places them |
| `dropped_group_rules` | Group rules `(later, earlier)` that a requirement, a master or a stronger group rule contradicted |
| `warnings` | Skipped things: unreadable plugin headers, duplicate names, an ordering rule whose condition could not be evaluated |
| `changed(&load_order)` | Whether the order differs from the one given |

`SortError`: `DataDirectory`, `TooManyPlugins` (more than 1024 installed plugins), `Conflict { first, second }` (two
rules that cannot both hold), `MasterOrder { plugin, master }` (a plugin without the master flag that has to load
before one with it), `TooMuchWork`. A masterlist condition that cannot be evaluated is not an error: the item is left
out and reported.

## What the sort does

Rules, strongest first. Everything about the order is decided by these, nothing else:

1. Plugins with the master flag (ESM) load before all others. A rule that asks otherwise is an error when it comes from
   `after`, `req` or a header, and is dropped when it comes from a group.
2. The masters listed in a plugin's header load before it.
3. The masterlist's `after` and `req` plugins (when their `condition` and `constraint` hold) load before the plugin.
4. A plugin in a group that loads after another group loads after that group's plugins, unless that contradicts a rule
   above or a group rule that was already kept; then the group rule is dropped and reported. Groups that are each
   after the other are ignored, as in `check`.
5. Among the plugins that may come next, the one that was earlier in the given order comes first. A plugin therefore
   moves only for a reason, and sorting a sorted order changes nothing.

This is **not** LOOT's algorithm: LOOT also sorts by overlap of records, by how many records a plugin changes and by
other things, and it has a priority system. The result is a valid order under the metadata rules, not LOOT's order. See
[COMPATIBILITY.md](COMPATIBILITY.md) and [ORACLE.md](ORACLE.md).

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
