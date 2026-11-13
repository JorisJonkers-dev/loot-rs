# Compatibility with LOOT

Status of every LOOT metadata and condition feature in this tool: **supported**, **partial** or **unsupported**.
Reference: https://loot-api.readthedocs.io (metadata syntax, condition strings). "Check" means the read-only
`loot-rs check`; nothing here sorts.

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
| Sorting | unsupported | Not a goal of this tool |

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
