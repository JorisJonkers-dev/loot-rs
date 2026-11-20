# Compatibility with LOOT

Status of every LOOT metadata and condition feature in this tool: **supported**, **partial** or **unsupported**.
Reference: https://loot-api.readthedocs.io (metadata syntax, condition strings). "Check" means the read-only
`loot-rs check`. Sorting (`api::sort`, `loot-rs sort`) is a port of libloot's algorithm, see Sorting below.

## Metadata file

| Feature | Status | Notes |
| --- | --- | --- |
| `plugins` list | supported | Entries in file order; all matching entries are merged |
| `globals` | supported | Messages shown with their conditions evaluated |
| `groups` | supported | Merged by name (descriptions replaced, `after` unioned). `sort` uses libloot's group graph; `check` checks pairwise, see Groups |
| `bash_tags` | partial | Read; not used for anything |
| `prelude` | supported | A normal YAML node, so aliases into it work; `Masterlist::from_file_with_prelude` and `--prelude` replace it with a separate prelude file as libloot does (port of `replace_prelude`, libloot's tests translated) |
| YAML anchors, aliases | supported | |
| YAML merge keys `<<` | supported | Used heavily by the real masterlist |
| Userlist, metadata merging of several files | supported | `Userlist` and `api::sort_with`, `--userlist`: merged as libloot's `plugin_metadata` with user metadata (user entries first, user group wins, lists and messages concatenated, general messages and Bash Tags after the masterlist's, groups merged by name). Compared with libloot's `load_userlist` on random worlds. Writing a userlist is the caller's |
| Unknown top-level keys | supported | Ignored, as LOOT does |
| Masterlist identity (`blob_sha1`, `update_timestamp`) | supported | `Masterlist::digest()` (git blob id, as LOOT computes it) and `source()` from LOOT's `.metadata.toml`; checked on a real LOOT copy, where `digest()` equals the stored `blob_sha1` |

## Plugin entry

| Key | Status | Notes |
| --- | --- | --- |
| `name`, exact | supported | Case-insensitive |
| `name`, regex | supported, except look-around and back-references | Anchored as libloot anchors it (`^{pattern}$`, so a top-level `A\|B` is "starts with A or ends with B"), case-insensitive, with ECMAScript's meaning of `\d`, `\w`, `\s` and their complements (ASCII digits and word characters, ECMAScript white space), as libloot's regress engine with the `iu` flags. Checked on a fixed table of 96 cases whose answers come from libloot (`tests/regex/cases.tsv`, `tests/regex_semantics.rs`): case folding of `ß`, `ẞ`, the Kelvin sign, `ſ`, Greek and accented letters agrees. Look-around and back-references are not available in the `regex` crate: such an entry is skipped with a warning, and the weekly real-masterlist run fails on it (decision in #37: the gap is accepted; the New Vegas masterlist has no such entry) |
| `group` | partial | First entry that sets it wins (merging replaces it only when unset). Undefined group is a note |
| `after` | supported | Violated when the named installed plugin is later in the order. `condition` honoured |
| `req` | supported | Missing file is a hard violation; a required plugin that loads later is a hard violation. `condition`, `constraint`, `display`, `detail` honoured |
| `inc` | supported | Present file is a hard violation. Same fields as `req` |
| `msg` | supported | `type`, `content` (string or localised list, see Localised content), `condition`, `subs` |
| `tag` | supported | Scalar and map forms, `-` removal, `condition`; removal beats addition |
| `dirty`, `clean` | supported | Read (`crc`, `util`, `itm`, `udr`, `nav`, `detail`) and compared with the installed file's CRC-32 in `api::sort`; a `dirty` match gives LOOT's message (`generated_messages`). `check` does not compare |
| `url` | supported | `PluginResult::locations`: links with their names, as written; never fetched |
| `display`, `detail` on file entries | supported | |
| `constraint` on file entries | supported | |

## Groups

| Behaviour | Status | Notes |
| --- | --- | --- |
| Predefined `default` group | supported | |
| Transitive "loads after" | supported | Closure over the merged definitions |
| Cyclic group relations | supported | `sort` refuses them with `SortError::GroupCycle`, as LOOT does. `check` ignores both directions of a cycle |
| Edge dropped when it clashes with master, `after` or `req` edges | supported | In `sort`, as libloot does (`SortOutput::dropped_group_rules`). `check` checks pairwise and does not drop edges, so it can show a note that LOOT would not enforce |
| Group named but undefined | supported | Reported as a note (hard with `--strict-groups`) |
| Group order violation | supported | A note by default, a violation with `--strict-groups`; see [GROUPS.md](GROUPS.md) |
| Sorting | supported | See the Sorting section below |
| Minimal-change sort (`SortMode::MinimalChange`) | supported | Not in libloot: keeps the given order and moves the fewest plugins that fix hard-constraint violations. Algorithm, guarantees and per-move reasons in API.md |

## Sorting

`api::sort` and `loot-rs sort` run a port of libloot's plugin graph (`src/sort.rs`, `src/sort/`; libloot 0.29.7,
`src/sorting/`). The graph code is checked against libloot's own unit tests, translated; the rest against golden
cases, property tests and, on the owner's machine, the oracle ([ORACLE.md](ORACLE.md)).

| libloot stage | Status | Notes |
| --- | --- | --- |
| Vertices in lexicographical (byte) order of the names | supported | The result does not depend on the order given except through the tie-break |
| Separate graphs for masters (master flag) and the others, joined masters first | supported | A rule that puts a plugin without the flag before one with it is an error (`MasterOrder`), as in libloot |
| Blueprint masters (Starfield) | not applicable | Fallout: New Vegas has none |
| Edges from header masters, `req` and `after` (masterlist) | supported | Conditions and constraints are evaluated first, as `filter_by_constraints` does. A name that is not installed adds nothing |
| Edges from the userlist (`req`, `after`, `group`) | supported | `UserRequirement`, `UserLoadAfter` and `UserGroup` edges in libloot's order; userlist groups appended to the group graph after the masterlist's; a group edge is a user one when the group path uses a userlist group edge or either plugin's group came from the userlist (libloot's tests translated). A cycle through a user rule is `SortError::RuleCycle`, naming every rule and its source |
| Hard-coded early loader | supported | `FalloutNV.esm`, the one plugin libloadorder lists for this game |
| Cycle among these edges is an error naming the cycle | supported | `SortError::Conflict` names the first two plugins of the cycle |
| Group edges: depth-first searches of the group graph, roots first by longest path, skipped when they would close a cycle, the `default` group last | supported | Same code path as libloot, including which edges are skipped. A group `after` a group nobody defined has that edge ignored, a plugin in an undefined group gets no group edges (libloot refuses both with an error; the finding is in `check`) |
| Cycle among groups | supported | `SortError::GroupCycle` (LOOT refuses it too) |
| Record overlap edges | supported | The record ids of every installed plugin are read (`src/records.rs`, FormIDs resolved against the plugin's masters as esplugin does): the plugin that overrides more records loads first when two plugins have a record in common. A plugin whose records cannot be read is noted and counts as having none |
| Asset (archive) overlap edges | supported | A plugin loads the BSA files of `Data` whose names begin with its name (case ignored, as libloot does for New Vegas); their folder and file names are read from the tables (version 103, 104 and 105; never the data). Of two plugins that load a file with the same path, the one that loads more files goes first, unless records decide. An archive that cannot be read is noted and skipped. Archives are taken from every searched directory (the additional data directories first), as libloot does. `.ba2` archives are not looked at |
| Tie-break edges and pinning | supported | |
| Topological sort | supported | petgraph's depth-first topological sort, reproduced |
| Messages LOOT's application generates when it checks the install (`generated_messages`, `generated_general_messages`) | partial | The ones that apply to Fallout: New Vegas, with LOOT's English texts as Markdown: a missing or inactive master of an active plugin (not with the `Filter` tag), a plugin that is its own master, a header version below 1.32, a group named by the masterlist that it does not define, Bash Tags overridden by `Data/BashTags/<plugin>.txt`, a `dirty` entry that matches the file, a requirement that is missing, an incompatibility that is present, and more than 255 active plugins. Every text above is compared with what LOOT 0.29.2 itself showed on synthetic cases ([LOOT-APP-CAPTURE.md](LOOT-APP-CAPTURE.md), `tests/golden_app/`); no difference was found. Not ported, with the reasons in [Not-ported messages](#not-ported-messages) below |
| The Bash Tags of the plugin's description (`bash_tags`) | supported | `{{BASH:Tag,Tag}}`, as libloot reads them |

Everything here is Fallout: New Vegas only. Differences that follow from what is not supported are listed above;
there are no others known. The sorted order, and the messages, Bash Tags, groups and cleaning data of a plugin and the
general messages (conditions evaluated, user metadata left out), are compared with libloot 0.29.7 on random synthetic
worlds, with the known differences listed in `scripts/oracle/compare.py` ([ORACLE.md](ORACLE.md)); the port is also
checked against libloot's own unit tests and its documentation.

## Generated messages LOOT's application shows and loot-rs does not port

libloot does not produce these messages; LOOT's application does. Each was decided for Fallout: New Vegas under nvmod
on Linux (issue #38):

| Message | Decision | Why |
| --- | --- | --- |
| Case-sensitive path warnings (a file named in metadata or the load order differs in case from the file on disk) | Ported as data, not as LOOT's text | It applies on Linux. loot-rs resolves every path case-insensitively, so no rule is lost; names in a data directory that differ only in case are reported in `SortOutput::warnings` (`FsEnv::case_warnings`), and a caller that wants LOOT's wording writes it from that |
| "Not sorted this session" | Not ported | It is the GUI's state of whether the user pressed Sort; nvmod decides when to sort and knows it |
| The text LOOT shows for a sorting cycle | Not ported as text | The cycle is returned as data: `SortError::Conflict` (two plugins) or `SortError::RuleCycle` (each plugin with the rule and its source) and `SortError::code()`; the caller words it |
| Checks of other games (light, medium, update and blueprint plugins) | Not applicable | Fallout: New Vegas has none of these plugin kinds |

The texts of the messages loot-rs does generate are LOOT's English texts, taken from its source (`src/validate.rs`,
NOTICE.md). Comparing them with the text the application shows, on synthetic Data directories, needs a run of LOOT
under Wine by the owner (issue #38); until then they are checked against LOOT's source only.

## Condition language

| Item | Status | Notes |
| --- | --- | --- |
| `and`, `or`, `not`, parentheses, C-style precedence | supported | |
| Whitespace and line breaks around operators and arguments | supported | |
| Regex detection (`:\*?\|` in the argument) | supported | Only the file name part is a regex; the directory is literal |
| `file(path)`, `file(regex)` | supported | Relative to `Data`, `..` allowed, case-insensitive |
| `active(path)`, `active(regex)` | supported | In the plugin list as active and installed |
| `many(regex)`, `many_active(regex)` | supported | |
| `file_size(path, size)` | supported | |
| `readable(path)` | supported | |
| `is_master(path)` | supported | Master flag of the plugin header |
| `checksum(path, crc)` | supported | CRC-32 of the whole file; `0x` prefix accepted |
| `version(path, op, version)` and the older argument order | supported on plugins, partial otherwise | Plugins, in every build: the version is read from the description field (`src/version.rs`, libloot's `extract_version` and the interpreter's comparison, ported; a timestamp, `version:` and a pseudo-semver, a bare pseudo-semver, then `v` or `version:` and a number). A plugin that is not installed or not in the load order, or whose description has no version, makes the condition false, as in libloot; a plugin that cannot be read is a condition error. Executables and DLLs need the cargo feature `executable-versions` (fixed file version); without it they are parsed, evaluate to "unsupported" and the item is not applied |
| `product_version(path, op, version)` | partial | Executables and DLLs with the feature (the `ProductVersion` string); otherwise unsupported |
| `filename_version(regex, op, version)` and the other argument order | supported | As `loot-condition-interpreter`: a file of the path's directory whose whole name (ghost extension removed, case ignored) matches and whose single capture group, as a version, compares true. Exactly one capture group, else a syntax error. Compared with libloot on random worlds |
| `description_contains(path, regex)` | supported | The plugin's description (decoded as Windows-1252, as esplugin does), case-insensitive, unanchored; false for a missing file, a non-plugin or no description. Compared with libloot on random worlds |
| `is_executable(path)` | supported | True when the file reads as a PE image by this crate's bounded reader (the interpreter's rule: headers, sections and the version resource, if any, read without an error); a file the interpreter would read with absurd sizes is false here instead of being read. Every build, no feature needed |
| Version comparison grammar | supported | LOOT's own (Pseudosem as the interpreter implements it), ported in `src/version.rs`; no dependency. `filename_version` uses it too |
| Evaluation caching | supported | Per sort: condition results by their text (as libloot caches them), compiled regular expressions, directory listings and checksums |

## Messages and output

`api::sort` returns three kinds of message per plugin: `messages` (the masterlist's, conditions evaluated), `generated_messages` (what LOOT's application adds, see Sorting) and `findings` (this crate's own notes, for example a rule the order could not satisfy).

| Item | Status | Notes |
| --- | --- | --- |
| Message types `say`, `warn`, `error` | supported | |
| Localised content | supported | `SortOptions::languages`: libloot's `select_message_content` per language, then English, then the first text; compared with libloot on random worlds with regional codes |
| `subs` substitution | supported | `{0}`, `{1}`, ... |
| CommonMark rendering | not applicable | Text is returned as CommonMark; the caller renders it (see API.md) |

## Load order input

| Item | Status | Notes |
| --- | --- | --- |
| `plugins.txt`, active-only style | supported | |
| `plugins.txt`, `*` style | supported | |
| `loadorder.txt` | unsupported | Not read; give a `plugins.txt` in load order |
| Ghosted plugins (`.ghost`) | supported | `X.esp.ghost` in a data directory counts as `X.esp`; names are not rewritten in `plugins.txt` |
| Game directory, additional data directories | supported | `SortOptions::game_dir` and `additional_data_dirs`, or `--game-dir` and `--extra-data`; additional directories first, then `Data`; `..` is the game directory. See API.md |
| Names differing only in case | warning | Reported in `warnings`; matching stays case-insensitive |
| Master flag / master list sort rules | unsupported | See `fnvorder check` in NV-Linux-Modding |

## Not-ported messages

Decisions for the messages the LOOT application adds that `generated_messages` and `generated_general_messages` do not
reproduce. The first two were shown by the application in the capture ([LOOT-APP-CAPTURE.md](LOOT-APP-CAPTURE.md)).

| LOOT message | Decision | Reason |
| --- | --- | --- |
| "You have not sorted your load order this session." (General Information) | Not ported | It describes the application's session (no sort has been applied since it started). `loot-rs sort` is the sort, and `check` has no session; a caller that wants the note knows its own session. The golden test drops this one line from the application's list |
| Text for a sorting cycle: `Cyclic interaction detected between "A.esp" and "B.esp": A.esp [Masterlist Load After] B.esp [Masterlist Load After] A.esp` | Not ported, behaviour matches | Both refuse the sort and name the same two plugins; `SortError::Conflict` / `GroupCycle` carry the plugin names, not LOOT's Markdown path with the rule kinds. Difference listed, not a bug: `tests/golden_app.rs` checks that both refuse and name the same plugins |
| A plugin that is its own master | Reproduced, but only visible through `validate::plugin_messages` | `loot-rs sort` refuses such a load order (the plugin would have to load before itself, as in the application, whose sort also fails), so the message never reaches `sort` output. The text matches the application's (`tests/golden_app/self_master`) |
| Case-sensitive path warnings ("... is installed in a case-sensitive location ...", and the same for local application data) | Not ported | The warning is about the game's folder and the mods, not about the load order: it fires when the game folder or `%LOCALAPPDATA%` folder is on a case-sensitive file system, which is the normal state on Linux. `loot-rs` resolves every plugin and file name case-insensitively itself (`FsEnv::resolve`) and already reports names that differ only in case (`case_warnings`), so its result does not depend on the file system. Under nvmod the case handling of the game folder belongs to nvmod, which owns the folder. The application could not be made to show it under Wine (Wine looks files up case-insensitively, so the check in LOOT 0.29.2 found nothing with the setting on), so there is no captured text to compare; the template is in LOOT.exe's string table. A native-Linux capture would need a LOOT build for Linux, which the release does not have |
| Checks of other games (light, medium, update and blueprint plugins) | Not applicable | Fallout: New Vegas has none of these plugin kinds |

## Condition functions used by the real masterlist

The README lists `filename_version`, `description_contains` and `is_executable` as not implemented. The Fallout: New
Vegas masterlist (`loot/falloutnv` branches `v0.26`, commit 9f4974f of 2026-04-07, and `v0.29`, 2026-08-31, the one LOOT
0.29 uses) contains none of them (`grep -c` of each name is 0 in both). The functions it does use are `active`, `file`,
`checksum`, `many`, `version`, `is_master`, `readable` and `product_version`, so no ticket is needed for the three.
