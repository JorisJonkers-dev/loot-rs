# loot-fnv

Read-only evaluation of [LOOT](https://github.com/loot/loot) masterlist rules (requirements, incompatibilities,
load-after, groups, messages, Bash Tag suggestions) over a Fallout: New Vegas / Tale of Two Wastelands load order,
in Rust, for use on Linux.

**Licence: GPL-3.0**, the licence of LOOT and libloot (read from their `LICENSE` files, see
[docs/UPSTREAMS.md](docs/UPSTREAMS.md)). This is a port in the licensing sense: it follows LOOT's rule semantics,
which were read from LOOT's public metadata documentation (https://loot-api.readthedocs.io), and is kept under
LOOT's licence. No LOOT source code is copied in this first slice. LOOT is by Oliver Hamlet (Ortham) and
contributors: https://github.com/loot/loot. The masterlist is community data under its own licence (CC0-1.0 for
`loot/falloutnv`) and is read at run time from a path you give; it is never copied into this repository.

`src/plugin.rs` is not GPL: it is the plugin header reader from `fnvorder`
(MIT OR Apache-2.0, JorisJonkers-dev/NV-Linux-Modding), taken under MIT and kept as a separate module. See
[NOTICE.md](NOTICE.md).

## Use

```text
loot-fnv check --masterlist <masterlist.yaml> --plugins <plugins.txt> --data <Data dir> [--strict-groups]
```

* `--masterlist`: a LOOT masterlist (for example `masterlist.yaml` from https://github.com/loot/falloutnv).
* `--plugins`: the load order, a `plugins.txt` (the active-only style MO2 writes for New Vegas, or the `*` style).
  The order of the lines is the load order.
* `--data`: the game's `Data` directory (for MO2, a directory that already shows the merged files). Paths in
  conditions are relative to it; `..` reaches the game directory. File names are matched case-insensitively.

It prints, for every installed plugin of the load order that has something to report:

* the messages whose condition holds, and the Bash Tags suggested (`+Tag` add, `-Tag` remove);
* **VIOLATION** lines (hard): a `req` file that is missing, an `inc` file that is present, an `after` plugin
  (or a `req` plugin) that loads later than the plugin;
* `note` lines (soft): group-order violations, an undefined group, conditions that could not be evaluated, listed
  plugins that are not in `Data`.

Exit code `0`: no hard violation. `1`: at least one hard violation (with `--strict-groups`, group violations and
undefined groups count too). `2`: bad arguments or unreadable input. It does not sort and writes nothing.

## What it does NOT do

* It does not sort, and it does not write `plugins.txt`, `loadorder.txt` or anything else.
* It does not run the game, Wine, Proton or MO2, and it does not download the masterlist or update it.
* The condition functions `version`, `product_version`, `filename_version`, `description_contains` and
  `is_executable` are **not implemented**. An item guarded by one is not applied and is reported as unsupported
  (with a count at the end); it is never guessed. They are common: on a synthetic list of 400 plugin names taken
  from the current New Vegas masterlist, 53 items were skipped for `version()`.
* `dirty` / `clean` are read as data; CRCs are not compared and no cleaning messages are produced.
* No userlist, no prelude replacement, no `url`, no localised message selection (English is used), no Markdown
  rendering, no metadata merging across masterlist and userlist, no ghosted plugins (`.ghost`).
* Groups are checked pairwise and independently; LOOT's rule that a group edge is dropped when it would clash
  with a master, `after` or `req` edge is not applied, so a group note can appear that LOOT would not enforce.
* The checks look only at the plugin list you give. Masters are not checked (that is `fnvorder check`).

Every LOOT feature is listed as supported, partial or unsupported in [docs/COMPATIBILITY.md](docs/COMPATIBILITY.md).

## Design notes

* `plugin`: header reader (flags, masters, description). Used for `is_master()`.
* `metadata`: masterlist reader. YAML anchors, aliases and `<<` merge keys are resolved, because the real New
  Vegas masterlist builds its messages from `<<: *template` merges.
* `condition`: parser and evaluator for the condition language. `and` / `or` are plain boolean logic with errors as
  unknown: `false and <unsupported>` is `false`, `true and <unsupported>` is reported as unsupported.
* `env`: the load order and `Data` directory conditions run against. Regex file names and `file()` paths with
  directories resolve case-insensitively.
* `check`, `render`: the rules and the text output.

### Why `yaml-rust2`

The masterlist needs a real YAML parser with anchors and aliases. `serde_yaml` is archived and no longer
maintained. `yaml-rust2` is the maintained fork of `yaml-rust`, resolves anchors and aliases, has a small
dependency tree and no proc macros (the metadata is read by hand from its node tree, so there is no derive layer
to get wrong on LOOT's scalar-or-map forms). The version in `Cargo.lock` (0.10.4) is the newest one the registry
proxy offered for this toolchain; a newer 0.13.0 exists and was not taken. `regex` is used for LOOT's regex names
and paths (anchored, case-insensitive, Unicode-aware); it has no look-around, so such a masterlist regex is
skipped with a warning instead of matching wrongly.

## How it was tested

* 27 unit tests: the plugin reader, the YAML subset (aliases, merge keys, localised content, group merging,
  regex names, bad input), the condition grammar and precedence, unsupported functions, CRC-32.
* 15 integration tests that run the built binary on throwaway game directories whose plugin files are generated
  byte by byte inside the test: exit codes, each violation kind, conditional requirements, regex entries and paths,
  case-insensitive paths, groups (transitive, cyclic, undefined), the starred `plugins.txt` style, bad arguments,
  and a check that nothing under the fixture changed.
* `cargo fmt`, `cargo clippy --all-targets -- -D warnings` and `cargo test` pass.
* Smoke test, not in the repository: the real `loot/falloutnv` masterlist (8,213 lines) parses, and a check over
  400 empty files named after masterlist entries runs in about 0.4 s in a debug build. Nothing was compared with
  LOOT's own output, so **agreement with LOOT is not verified**. No game data was used.
* CI: see `.github/workflows/ci.yml` (format, clippy, tests with an 80% line coverage gate, release build,
  gitleaks, actionlint).

## Open questions

* Use `loot-condition-interpreter` (MIT, by the LOOT authors) instead of the evaluator here? It would give
  `version` and the rest of the language and LOOT's exact version comparison, at the cost of a dependency tree and
  a game-state adapter. This slice keeps its own small evaluator so the limits are visible.
* Implementing `version()` for plugins needs the description field and LOOT's version grammar (documented, long);
  for executables it needs PE version resources. Which of them matters first: plugins (e.g. TTW), or `.exe`/`.dll`
  (NVSE, the game)?
* Should group violations be hard by default? They are soft here because LOOT treats groups as a sorting hint that
  yields to other rules.
* A golden comparison against LOOT's real report for one profile (run under MO2 or Wine) is the way to find
  disagreements; it needs a decision on where the oracle files may live.
* The repository still contains the `templates/` tree and `scripts/` from the repository template.
