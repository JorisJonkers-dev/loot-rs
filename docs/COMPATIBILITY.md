# Compatibility with LOOT

Status of every LOOT metadata and condition feature in this tool: **supported**, **partial** or **unsupported**.
Reference: https://loot-api.readthedocs.io (metadata syntax, condition strings). "Check" means the read-only
`loot-rs check`. Sorting (`api::sort`, `loot-rs sort`) is a port of libloot's algorithm, see Sorting below.

## Metadata file

| Feature | Status | Notes |
| --- | --- | --- |
| `plugins` list | supported | Entries in file order; all matching entries are merged |
| `globals` | supported | Messages shown with their conditions evaluated |
| `groups` | partial | Merged by name (descriptions replaced, `after` unioned); checked pairwise, see Groups |
| `bash_tags` | partial | Read; not used for anything |
| `prelude` | partial | It is a normal YAML node, so aliases into it work. Replacement from a separate prelude file is unsupported |
| YAML anchors, aliases | supported | |
| YAML merge keys `<<` | supported | Used heavily by the real masterlist |
| Userlist, metadata merging of several files | unsupported | One file is read |
| Unknown top-level keys | supported | Ignored, as LOOT does |

## Plugin entry

| Key | Status | Notes |
| --- | --- | --- |
| `name`, exact | supported | Case-insensitive |
| `name`, regex | partial | Whole name, case-insensitive, Unicode. Detected by `:\*?\|` as in LOOT. Rust `regex` syntax, so look-around and back-references are not available: such an entry is skipped with a warning |
| `group` | partial | First entry that sets it wins (merging replaces it only when unset). Undefined group is a note |
| `after` | supported | Violated when the named installed plugin is later in the order. `condition` honoured |
| `req` | supported | Missing file is a hard violation; a required plugin that loads later is a hard violation. `condition`, `constraint`, `display`, `detail` honoured |
| `inc` | supported | Present file is a hard violation. Same fields as `req` |
| `msg` | supported | `type`, `content` (string or localised list: English), `condition`, `subs` |
| `tag` | supported | Scalar and map forms, `-` removal, `condition`; removal beats addition |
| `dirty`, `clean` | partial | Read as data (`crc`, `util`, `itm`, `udr`, `nav`); not compared with the installed plugin, no messages derived |
| `url` | unsupported | Ignored |
| `display`, `detail` on file entries | supported | |
| `constraint` on file entries | supported | |

## Groups

| Behaviour | Status | Notes |
| --- | --- | --- |
| Predefined `default` group | supported | |
| Transitive "loads after" | supported | Closure over the merged definitions |
| Cyclic group relations | partial | Both directions of a cycle are ignored; LOOT ignores the edge that would create the cycle |
| Edge dropped when it clashes with master, `after` or `req` edges | unsupported | A note can appear that LOOT would not enforce |
| Group named but undefined | supported | Reported as a note (hard with `--strict-groups`) |
| Group order violation | supported | A note by default, a violation with `--strict-groups`; see [GROUPS.md](GROUPS.md) |
| Sorting | partial | See the Sorting section below |

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
| Edges from the userlist (`req`, `after`, `group`) | unsupported | One masterlist is read; there is no userlist |
| Hard-coded early loader | supported | `FalloutNV.esm`, the one plugin libloadorder lists for this game |
| Cycle among these edges is an error naming the cycle | supported | `SortError::Conflict` names the first two plugins of the cycle |
| Group edges: depth-first searches of the group graph, roots first by longest path, skipped when they would close a cycle, the `default` group last | supported | Same code path as libloot, including which edges are skipped. A group `after` a group nobody defined has that edge ignored, a plugin in an undefined group gets no group edges (libloot refuses both with an error; the finding is in `check`) |
| Cycle among groups | supported | `SortError::GroupCycle` (LOOT refuses it too) |
| Record overlap edges | partial | The graph code is ported and tested; the record data is not read yet, so no plugin overlaps another and these edges are absent. A load order where two plugins change the same records can come out in another order than LOOT's |
| Asset (archive) overlap edges | partial | Same: ported, not fed yet |
| Tie-break edges and pinning | supported | |
| Topological sort | supported | petgraph's depth-first topological sort, reproduced |
| Messages LOOT's application generates while sorting (missing masters, inactive requirements, dirty plugins) | unsupported | |

Everything here is Fallout: New Vegas only. Differences that follow from what is not supported are listed above;
there are no others known.

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
| `version(path, op, version)` and the older argument order | partial | Executables and DLLs with the cargo feature `executable-versions` (fixed file version, LOOT's comparison). Plugins, and every file without the feature: parsed, evaluates to "unsupported", the item is not applied |
| `product_version(path, op, version)` | partial | Executables and DLLs with the feature (the `ProductVersion` string); otherwise unsupported |
| `filename_version(regex, op, version)` | unsupported | Same |
| `description_contains(path, regex)` | unsupported | Same |
| `is_executable(path)` | unsupported | Same |
| Version comparison grammar | partial | With the feature, LOOT's own (the interpreter's). Without it unsupported, and needed by `filename_version` |
| Evaluation caching | unsupported | Directory listings are cached; results are not |

## Messages and output

| Item | Status | Notes |
| --- | --- | --- |
| Message types `say`, `warn`, `error` | supported | |
| Localised content | partial | English (or the first language) only |
| `subs` substitution | supported | `{0}`, `{1}`, ... |
| CommonMark rendering | unsupported | Text is printed as written |

## Load order input

| Item | Status | Notes |
| --- | --- | --- |
| `plugins.txt`, active-only style | supported | |
| `plugins.txt`, `*` style | supported | |
| `loadorder.txt` | unsupported | Not read; give a `plugins.txt` in load order |
| Ghosted plugins (`.ghost`) | unsupported | |
| Master flag / master list sort rules | unsupported | See `fnvorder check` in NV-Linux-Modding |
