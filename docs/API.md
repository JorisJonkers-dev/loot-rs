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

`loot-rs sort --masterlist <file> --plugins <plugins.txt> --data <Data> [--game-dir <dir>] [--extra-data <dir>]... [--strict-groups] [--mode full|minimal] [--json]` runs the same code: the
text is the order with what applies to each plugin, `--json` is one line with `sorted`, `plugins`, `general_messages`,
`dirty`, `rules`, `dropped_group_rules`, `not_installed`, `warnings` and `hard_violations`. Exit code 0, or 1 when a
finding is hard, or 2 when the rules contradict each other or an input cannot be read.

## `sort(load_order, masterlist, data_dir, options) -> Result<SortOutput, SortError>`

| Argument | Meaning |
| --- | --- |
| `load_order: &[PluginSlot]` | The current order, each plugin with its file name and whether it is active |
| `masterlist: &Masterlist` | A masterlist read once with `Masterlist::parse(text)` or `Masterlist::from_file(path)` (8 MiB limit, aliases bounded), reusable |
| `data_dir: &Path` | The game's `Data` directory (or a mod manager's staging `data` folder); condition paths are relative to it |
| `options: &SortOptions` | `strict_groups` (see [GROUPS.md](GROUPS.md)); default is soft. `game_dir` and `additional_data_dirs` (below). `mode`: `SortMode::Full` (default) or `SortMode::MinimalChange` (see Minimal-change sort). `threads` (`with_threads`): 0, the default, is one per core up to 16. `cache` (`with_cache`): a `SortCache` (below) |

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
| `mode`, `moves` | The mode that made the order; with `MinimalChange`, one `PluginMove` per plugin that moved (`plugin`, `from`, `to`, `reason`, `other`, `indirect`, `reasons`), else empty |
| `explanations` | Every installed plugin whose place changed, in sorted order, as a `PluginMove` with every reason (`MoveExplanation`: `reason`, `other`, `later`, `indirect`, and `via`, `groups`, `counts` where they apply). See Why plugins moved |
| `warnings` | Skipped things: unreadable plugin headers, duplicate names, an ordering rule whose condition could not be evaluated |
| `changed(&load_order)` | Whether the order differs from the one given |

`SortError`: `DataDirectory`, `TooManyPlugins` (more than 1024 installed plugins), `Conflict { first, second }` (two
rules that cannot both hold: the first two plugins of the cycle), `MasterOrder { plugin, master }` (a plugin without
the master flag that has to load before one with it), `GroupCycle(groups)` (groups that load after each other),
`TooMuchWork`, `Internal(text)` (a check inside the sorter failed: a bug). A masterlist condition that cannot be evaluated is not an error: the item is left
out and reported.

## `check(load_order, masterlist, userlist, data_dir, options) -> Result<CheckOutput, SortError>`

What `loot-rs check` reports, for the load order as given and without sorting it: one `PluginResult` per installed
plugin in the given order (messages, generated messages, Bash Tags, findings such as an unmet `req`, a present `inc`
or a violated `after` or group rule), `general_messages`, `generated_general_messages`, `general_findings` (a general
message whose condition could not be evaluated, so it was not applied), `dirty`, `not_installed` and `warnings`;
`hard_violations()` counts the hard findings, per plugin and general. `userlist` is merged as in `sort_with`. It takes
the same `SortOptions` (languages, `strict_groups`, directories, cancel, progress with the stages `ReadingPlugins`,
one tick per plugin whose header was looked at, and `Checking`, the skip switches) and returns `DataDirectory`,
`TooManyPlugins` or `Cancelled`.

`warnings` are the masterlist's and userlist's skipped entries (a userlist entry prefixed `userlist: `), duplicate
names, directories that are not ones and unreadable plugin headers, as in `loot-rs check`. A plugin is in `plugins`
or in `not_installed`, never both; `position` is its place in the order given. A group the userlist defines is defined
for the generated group messages too. The progress hook sees only `ReadingPlugins` and `Checking`: nothing is read
for overlap and nothing is sorted.

## Errors to show and copy

`SortError`, `MasterlistError` and `EvaluateError` each have `code()`, a stable code (`MasterlistError`:
`masterlist-io`, `masterlist-invalid`), a `Display` message in plain words, and `copy_text()`: one block of three lines,
`loot-rs <version>`, `error: <code>` and the message, with the home directory (`$HOME`, without a trailing `/`)
written as `~` where it starts a path. It never contains a command line. `api::copy_text(code, error)` builds the same
block for any message.

No library function prints or ends the process: the command line writes to the writers it is given and only
`main.rs` turns the result into an exit code (`tests/no_print.rs` checks the sources).

## Threads and the cache

`sort` reads plugin headers, records, archive tables and checksums, and compares the plugins' records and archive
files, on `SortOptions::threads` threads (0: one per available core, at most 16). The result is the same for every
thread count; the limits are decided from complete counts, never by which thread ran first.

`SortCache` (`SortCache::new()`, `SortOptions::with_cache(cache.clone())`) keeps the record ids and archive files that a
sort read, by path, size, modification time and inode, and the next sort with the same cache reads only what changed.
Clones share the data, and several sorts can use one cache at once from several threads. `clear()` forgets everything;
`len()` returns the plugins and archives held. The result is the same with or without a cache. A file rewritten in place
within the same second to the same size is not noticed: use a new cache after such a change.

`Masterlist`, `SortOptions` and `SortCache` are `Send` and `Sync`: one masterlist can be shared by sorts on several
threads. Speed measurements and the decisions behind them are in [RESOURCE-LIMITS.md](RESOURCE-LIMITS.md).

## Message text: language and Markdown

Message text is CommonMark, as the masterlist writes it and as LOOT's own generated messages are; loot-rs does not
render it (the caller does). The New Vegas masterlist uses emphasis (`**bold**`, `*italic*`), inline code, links
(`[text](url)`), and occasionally bulleted lists and line breaks; substitutions (`{0}`, `subs`) are applied before
the text is returned, and LOOT's generated messages escape Markdown in plugin names.

`SortOptions::languages` (`with_languages(["de", "pt_BR"])`, `--language <code>` on the command line, repeatable) chooses
the text of a localised message: a message with one text gives that text; otherwise each wanted language in turn by
libloot's `select_message_content` rule (the text in exactly that language, else the first whose language code matches:
`pt` for `pt_BR`, `pt_BR` for `pt`); then English; then the first text. `Message::lang` (JSON `lang`) says which was
chosen; generated messages are `en`.

`PluginResult::locations` (JSON `locations`) are the plugin's `url` entries (a link, or `{link, name}`), each once, the
userlist's first. They are returned as written and never checked or fetched.

## The userlist

`api::sort_with(load_order, &masterlist, Some(&userlist), data_dir, &options)` merges a `Userlist`
(`Userlist::parse(text)` or `Userlist::from_file(path)`, the same limits as a masterlist) over the masterlist, as
libloot does with a loaded userlist; `sort` is `sort_with` without one. For each plugin the userlist's entries come
first: a group it sets wins, and its `after`, `req`, `inc`, messages, tags and cleaning data come before the
masterlist's. Its groups are merged by name for the checks and added to the group graph after the masterlist's (a
userlist group may load after a masterlist group; a masterlist group cannot load after a group only the userlist
defines). Its general messages and known Bash Tags follow the masterlist's.

Rules from the userlist are reported with the sources `user-after`, `user-req` and `user-group` (`RuleSource`,
`MoveReason`); a group rule is a user one when either plugin's group came from the userlist. A cycle through a user
rule is `SortError::RuleCycle(links)` (code `rule-cycle`): each plugin with the rule that puts it before the next.

loot-rs does not write userlists. A caller writes the YAML in the masterlist's shape:

```yaml
groups:
  - name: my late group
    after: [default]
plugins:
  - name: MyPatch.esp
    group: my late group
    after: [SomeMod.esp]
    req: [SomeMod.esm]
    msg:
      - type: say
        content: 'A note of my own.'
globals:
  - type: warn
    content: 'Something about the whole load order.'
```

On the command line: `--userlist <file>` for `check` and `sort`. The JSON's `rules[].source` and the move reasons
gain the values `user-after`, `user-req` and `user-group`, which appear only when a userlist is given.

## The masterlist: identity, prelude and updates

* `Masterlist::digest()`: the git blob id of the masterlist text as read (`sha1("blob <length>\0" + bytes)`, what
  `git hash-object` prints), before any prelude replacement. It is the value LOOT stores as `blob_sha1`.
* `Masterlist::from_file` also reads LOOT's `<masterlist>.metadata.toml` next to the file when there is one:
  `source()` gives its `blob_sha1` and `update_timestamp`, and `matches_source()` says whether the text read is the one
  it names. A caller that shows "messages are LOOT's own" labels them with `digest()` (and the timestamp).
* `Masterlist::from_file_with_prelude(masterlist, prelude)` and `parse_with_prelude(text, prelude)`: the masterlist's
  block-style `prelude:` section is replaced by the prelude file's text, indented by two spaces, as libloot's
  `replace_prelude` does (the section ends at the first line that starts with neither a space, `#` nor a line break;
  a masterlist without such a key is read as it is). `prelude_digest()` is the prelude's blob id. On the command
  line: `--prelude <file>` for `check` and `sort`.

Update policy: loot-rs never touches the network. The caller downloads the masterlist from
`https://raw.githubusercontent.com/loot/falloutnv/v0.29/masterlist.yaml` (the branch whose metadata syntax libloot 0.29
reads) and the prelude from `https://raw.githubusercontent.com/loot/prelude/v0.29/prelude.yaml`, verifies them (for
example against the blob ids of the files in that branch, which `digest()` and `prelude_digest()` compute), stores them
and gives loot-rs the paths.

## Why plugins moved

`SortOutput::explanations` lists every installed plugin whose place in the result differs from its place in the given
order (`from` and `to` count installed plugins only); a plugin that kept its place is not listed. In a full sort the
reasons are computed from the edges the sorter added, never guessed from the final order:

* every edge between the plugin and a plugin that was on its other side in the given order: `header-master`, `req`,
  `after`, `group` (with `groups`: the earlier plugin's group, then the later's), `record-overlap` and
  `asset-overlap` (with `counts`: override records or archive files of the earlier plugin, then the later's; the
  one with more loads first), `hardcoded` (`FalloutNV.esm` first);
* when there is none, `master-flag` when a plugin of the other partition crossed it (masters load first);
* otherwise one indirect reason: a plugin that crossed it, joined to it by a chain of edges, and `via`, the link of the
  chain that goes against the given order (that link's class is the `reason`; `tie-break` only appears here).

`later` says whether the reason puts the plugin after `other` (it moved later) or before. `reason`, `other` and
`indirect` of the `PluginMove` itself repeat the first reason. In a minimal-change sort `explanations` equals `moves`.
Reasons hold names and counts only.

## Cancelling, progress, codes and conditions

* `SortOptions::with_cancel(token)`: a `CancelToken` (`CancelToken::new()`, or `CancelToken::from(Arc<AtomicBool>)`);
  `token.cancel()` from any thread makes the sort return `SortError::Cancelled` soon after. It is checked before each
  file is read, between the stages and inside the graph search; nothing is written and no thread is left running.
* `SortOptions::with_progress(ProgressHook::new(|progress| ...))`: called with `Progress { stage, done, total }` from the
  sort's threads. Stages, in order: `ReadingPlugins` (headers and records, per plugin), `ReadingArchives` (per archive,
  left out when `skip_archives`), `ComparingPlugins` (per plugin), `Sorting` (0 then 1), `Checking` (0 then 1). Each
  stage starts with `done` 0 and ends with `done == total`. Counts only, never names.
* `SortError::code()`: `data-directory`, `too-many-plugins`, `conflict`, `master-order`, `group-cycle`,
  `too-much-work`, `internal`, `cancelled`, `rule-cycle`. A code keeps its meaning; a new variant gets a new code.
* `api::evaluate(condition, load_order, data_dir, options) -> Result<bool, EvaluateError>`: one condition string,
  evaluated as the sort evaluates the masterlist's (a tester for a doctor command). `EvaluateError::code()`:
  `data-directory`, `invalid`, `unsupported`.
* Switches for a caller that wants only the order: `skip_checksums` (no whole-file CRC-32 for cleaning data, so
  `dirty` is empty; the order is unchanged), `skip_archives` (no BSA tables, so no asset overlap: the order can differ
  from LOOT's), `skip_executable_versions` (`version()`/`product_version()` on executables are unsupported; they are evaluated by default).
  `SortOptions::order_only()` sets all three.
* Cargo feature `serde`: `Serialize` for `SortOutput` and its parts, with exactly the keys and values of
  `loot-rs sort --json` without `schema_version` and `contract` (`tests/serde.rs` compares the two). Written by hand on
  `serde` without derive macros; the default build does not depend on `serde`. `SortOutput::hard_violations()` is the
  JSON's `hard_violations`.

## Game directory and additional data directories

`SortOptions` has `game_dir: Option<PathBuf>` and `additional_data_dirs: Vec<PathBuf>`, set with `with_game_dir`,
`with_additional_data_dir` and `with_additional_data_dirs`; `loot-rs check` and `sort` take them as `--game-dir` and a
repeatable `--extra-data`. `SortOptions` is no longer `Copy` (clone it where a copy is needed).

The rule, the one `loot-condition-interpreter` and libloot use:

* A path (in a condition, a plugin name, a `BashTags` file) is looked for in the additional directories in the order
  given, then in `data_dir`. Every component is matched without regard to case.
* When a file exists in several directories the first directory that has it wins, for its contents as well as its
  existence: that file's header, records, checksum, size and version are the ones used. `file()`, `active()`,
  `many()`, `checksum()`, `readable()`, `is_master()` and `file_size()` follow this rule. A name pattern counts each
  name once across all directories.
* A leading `..` names `game_dir` when it is set. Without it, `..` is the parent of the directory searched (so a
  staging tree's `data/..` is the staging folder, not the game).
* A plugin that is only in an additional directory is installed, read and sorted. Archives (`.bsa`) are taken from
  every directory, additional ones first, as libloot does.
* `Name.esp.ghost` (and `.esm.ghost`) in a directory counts as the plugin `Name.esp`, as libloot trims the suffix.
  `ghost` files that are not plugins are not renamed.
* Two names in one searched directory that differ only in case are reported in `warnings` (the first in sorted
  order is used); a `game_dir` or additional directory that is not a directory is a warning too.

The JSON gains no key: the new texts are entries of `warnings`, so `schema_version` stays 1.

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

## Minimal-change sort

`SortOptions::with_mode(SortMode::MinimalChange)` (`loot-rs sort --mode minimal`) is for an order that is already valid
for the game but not LOOT-sorted, for example a mod manager's own order. The full sort reorders plugins that no rule
relates, by the order it was given and by record overlap; this mode keeps the given order wherever no hard constraint is
violated and moves the fewest plugins that fix the violations. The default mode, the order it returns and the JSON of a
default sort are unchanged.

Hard constraints: plugins with the master flag before the others; the header masters; the masterlist's `req` and
`after` (conditions applied); the game's first plugin (`FalloutNV.esm`) before every other plugin of its kind; and, only
with `strict_groups`, the group rules that libloot's sort satisfies (the ones it did not drop for a cycle). Soft group
rules, record overlap and asset overlap are not enforced. The full sort runs first, so the same errors are returned
(`Conflict`, `MasterOrder`, `GroupCycle`, ...) and the constraints are known to be consistent.

The algorithm (`src/sort/minimal.rs`, deterministic, no randomness):

1. A violation is a pair `(u, v)`, `u` required before `v` (directly or through a chain of constraints, including the
   master flag), that the given order has the other way round. The violations are the edges of a conflict graph.
2. The plugins that stay must contain no violation among themselves, so they form an independent set of the conflict
   graph; the moves are `n` minus the size of that set. A largest independent set is found per connected component:
   plugins without a violation stay, a plugin with one violation left stays and its partner moves (an optimal
   reduction), and the rest is a branch and bound over the plugin with most violations. Largest independent set is
   NP-hard in general, so each component has a work cap (`COMPONENT_BUDGET`); when it is reached the best set found is
   used, the first answer being the greedy "move the plugin with most violations" one, and the result is not proven
   smallest. Among equally small answers the branching order decides, so the result is the same every time.
3. The kept plugins keep their relative order. Each moved plugin, in the order given, is inserted at the place nearest
   its given place that respects every constraint towards the plugins already in the sequence.

Why every constraint holds: the kept plugins have no violation, so they respect the transitive constraints. When a
moved plugin `m` is inserted, every placed plugin `p` that must load before `m` and every placed plugin `s` that must
load after it satisfy `p` before `s` (`p < m < s` and the sequence respects the relation), so the interval between the
last `p` and the first `s` is not empty and the invariant survives. An order without violations has an empty conflict
graph: nothing moves and the order is returned as given. The tests check these claims: `tests/prop_minimal.rs` on random
worlds compares the satisfied constraints with the full sort (the libloot port), the move count with a brute-force
optimum, idempotence and that the full sort's own order is returned unchanged; `scripts/oracle/random_diff.py` does the
same against libloot itself on synthetic worlds (see [ORACLE.md](ORACLE.md)).

Each move has a reason: the class of the first violated constraint that forced it, in the order of the classes
(hardcoded, master-flag, header-master, req, after, group), a directly stated constraint before one that follows from a
chain (`indirect`), and `other`, the plugin on the other side. `from` and `to` are positions among the installed plugins.
The text output lists the moves after the plugins; the JSON gains two optional keys, `mode` (`minimal`) and `moves`, only
for this mode. New optional keys do not raise `schema_version`, which stays 1, and a default sort writes neither.
`SortMode` and `MoveReason` are `#[non_exhaustive]`.

## Guarantees

* Read only: nothing under `Data` or anywhere else is written.
* Deterministic: the same arguments and files give the same result; which group rules are dropped does not depend on
  the order given.
* Bounded: see [RESOURCE-LIMITS.md](RESOURCE-LIMITS.md) (at most 1024 installed plugins, graph search limited to 1
  billion steps and record comparison to 8 billion, no whole plugin file held in memory for a checksum). Limits are
  errors, never panics.
* Independent of threads and cache: `SortOptions::threads` and `SortOptions::cache` change only how fast the result
  comes.
* Honest about gaps: an unimplemented condition function leaves its item out and is reported, never guessed.

## Versioning

* `api::API_VERSION` is the version of this API: the crate version. Each release's tag (`vX.Y.Z`), its GitHub release
  and its `CHANGELOG.md` section carry it, and a breaking change raises it as described below.
* The crate follows semantic versioning; while it is 0.x a minor release may break the API and a patch release never
  does. The release pull requests are made by release-please from the Conventional Commit titles, so a breaking
  change is written as `feat!:` or has a `BREAKING CHANGE:` footer.
* Structs in `loot_rs::api` are `#[non_exhaustive]`: build them with `new` or `Default` and read fields freely; a
  field can be added in a minor release.
* Enums in `loot_rs::api` are `#[non_exhaustive]`: a variant can be added in a minor release, so keep a `_` arm.
* Cargo features are additive. `executable-versions` (see the README) changes only which conditions can be evaluated.
* The tests in `tests/api.rs` use only `loot_rs::api` and `loot_rs::plugin::synthetic_plugin` from outside the crate; a
  change that breaks them is a breaking change.

### Added in this version

`SortOptions::threads`, `cache`, `cancel`, `progress`, `skip_checksums`, `skip_archives`, `skip_executable_versions`
and their builders; `SortCache`, `CancelToken`, `Progress`, `Stage`, `ProgressHook`; `SortError::Cancelled` and
`SortError::code()`; `api::evaluate` and `EvaluateError`; `SortOutput::hard_violations()`; the `serde` feature;
`SortOutput::explanations`, `PluginMove::reasons`, `MoveExplanation` and the `MoveReason` variants `RecordOverlap`,
`AssetOverlap` and `TieBreak`. The JSON gains the key `explanations` (and `reasons` in each move): new keys, so
`schema_version` stays 1. All
additions: no signature changed, and `SortError` is `#[non_exhaustive]`.

Also: `api::check` with `CheckOutput` (and its `general_findings`), `copy_text()` on the three error types,
`MasterlistError::code()`, `api::copy_text` and `api::API_VERSION`.

## Not in the API yet

* Writing `plugins.txt` or `loadorder.txt` (the caller writes them).

Everything else that is missing is listed in [COMPATIBILITY.md](COMPATIBILITY.md).

## Semver

The crate is 0.x. Moving from the metadata-only sorter to libloot's algorithm changes the order `sort` returns for the
same input (a plugin that is later in the given order stays later wherever the rules allow, where the old sorter pulled
earlier ones forward), adds the `SortError` variants `GroupCycle` and `Internal` (the enum is `#[non_exhaustive]`, so
callers already have a wildcard arm), and replaces the `sort` module's functions (`sort::order` is now `sort::sort`
over `PluginInput`s; that module is public for tests and carries no promise). The `api` module's signatures are
unchanged, but `PluginResult` and `SortOutput` (both `#[non_exhaustive]`) gain fields (`generated_messages`, `bash_tags`, `generated_general_messages`), and the JSON of `loot-rs sort --json` gains the same keys. Release as a minor version.

## JSON contracts

`loot-rs check --json` and `loot-rs sort --json` write an integer `schema_version` as the first key (`api::SCHEMA_VERSION`, now 1) and a `contract` name as the second: `loot-check` for `check`, `loot-sort` for `sort` (`api::CHECK_CONTRACT`, `api::SORT_CONTRACT`). A reader refuses a document whose name is not the one it asked for. `schema_version` rises when a key is removed, renamed or retyped, or when a value is added to an enum that is not open; a new optional key does not raise it, so a consumer ignores keys it does not know.

`loot-rs contracts --json` prints `{"contracts":[{"name":"loot-check","versions":[1]},{"name":"loot-sort","versions":[1]}]}` (`api::CONTRACTS`, `api::contracts_json`) and exits 0. A consumer picks the highest version both sides list that is not above the version it was tested with, and refuses a tool whose list lacks it. This build emits only the newest version of each contract.

An enum in a published schema that may grow carries `"x-open-enum": true`, and a reader treats an unknown value as "other"; an enum without the mark is closed, and adding a value to it is a version change.

## Helpers for a mod manager

* `SortOutput::order_with_not_installed(&load_order)`: the whole load order with the not-installed plugins put back, each right after the installed plugin that preceded it in the given order (first when none did).
* `MessageKind::label()` and `FindingKind::label()`: the words the text and JSON output use.
* `api::read_plugin_header(path)`, with `api::PluginHeader` and `api::PluginError`: the header of one plugin (masters, flags, author, description, record count). `PluginHeader` is `#[non_exhaustive]`.
