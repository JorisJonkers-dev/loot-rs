# loot-rs

Read-only evaluation of [LOOT](https://github.com/loot/loot) masterlist rules (requirements, incompatibilities,
load-after, groups, messages, Bash Tag suggestions) over a Fallout: New Vegas / Tale of Two Wastelands load order,
in Rust, for use on Linux.

This is an independent port, not an official LOOT project.

**Licence: GPL-3.0**, the licence of LOOT and libloot (read from their `LICENSE` files, see
[docs/UPSTREAMS.md](docs/UPSTREAMS.md)). This is a port in the licensing sense: it follows LOOT's rule semantics,
which were read from LOOT's public metadata documentation (https://loot-api.readthedocs.io), and is kept under
LOOT's licence. The sorter, the record and archive overlap, the generated messages and the version grammar are ported
from libloot and LOOT's source; [NOTICE.md](NOTICE.md) lists each part and where it came from. LOOT is by Oliver Hamlet (Ortham) and
contributors: https://github.com/loot/loot. The masterlist is community data under its own licence (CC0-1.0 for
`loot/falloutnv`) and is read at run time from a path you give; it is never copied into this repository.

`src/plugin.rs` is not GPL: it is the plugin header reader from `fnvorder`
(MIT OR Apache-2.0, JorisJonkers-dev/NV-Linux-Modding), taken under MIT and kept as a separate module. See
[NOTICE.md](NOTICE.md).

## Use as a library

`loot_rs::api::sort(load_order, masterlist, data_dir, options)` returns the sorted order, the messages and Bash Tags
that apply, the findings, and the cleaning data that matches the installed files, for a program such as a mod
manager. It reads files and writes nothing. See [docs/API.md](docs/API.md) for the contract, the sorting algorithm (a
port of libloot's plugin graph, see [docs/COMPATIBILITY.md](docs/COMPATIBILITY.md)) and the semver policy.

## Packages

The `loot-rs` program is published on each release as an OCI package on ghcr.io:

`ghcr.io/jorisjonkers-dev/loot-rs:<version>-x86_64-unknown-linux-gnu`

where `<version>` is the release tag without the leading `v`. The package is a `tar.gz` archive with the binary and
`LICENSE`. Verify it with:

```sh
gh attestation verify oci://ghcr.io/jorisjonkers-dev/loot-rs:<version>-x86_64-unknown-linux-gnu --repo JorisJonkers-dev/loot-rs
```

## Use (command line)

`loot-rs sort` takes the same arguments as `check` and prints the load order sorted with LOOT's algorithm (see
[docs/API.md](docs/API.md)), with what applies to it; `--json` prints it for tools. It writes nothing. The rest of this
section describes `check`.

```text
loot-rs check --masterlist <masterlist.yaml> --plugins <plugins.txt> --data <Data dir> [--strict-groups]
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

Group findings are soft unless `--strict-groups` is given, see [docs/GROUPS.md](docs/GROUPS.md).

Exit code `0`: no hard violation. `1`: at least one hard violation (with `--strict-groups`, group violations and
undefined groups count too). `2`: bad arguments or unreadable input. `check` does not sort and writes nothing.

## Executable versions (opt-in)

```text
cargo build --release --features executable-versions
```

With the feature, `version(path, ...)` and `product_version(path, ...)` on a `.exe` or `.dll` under `Data` (or `..`,
the game directory) read the file's PE version resource and compare it with LOOT's version grammar, through
[`loot-condition-interpreter`](https://github.com/loot/loot-condition-interpreter) (MIT, by the LOOT authors). The
default build does not contain it. The interpreter depends on `esplugin` (GPL-3.0), which is why the feature is
off by default; this project is GPL-3.0 as well. A file that does not exist makes the condition false, as in
LOOT; a file that is not a PE image, or whose headers are out of bounds, is a condition error (the item is not
applied). The file is read once with every size checked and the interpreter is given only the validated version block, in a minimal image in a private temporary directory. `version()` on a plugin (`.esp`, `.esm`) needs no feature: it is read from the description field by this crate's own
port of libloot's rules (`src/version.rs`). Every size read from the file is checked before the interpreter sees it, see
[docs/RESOURCE-LIMITS.md](docs/RESOURCE-LIMITS.md).

## What it does NOT do

* Nothing here writes `plugins.txt`, `loadorder.txt` or anything else. `sort` (command line and library) sorts: it is a
  port of libloot's plugin graph (masters, `req`, `after`, groups, record overlap, archive (BSA) overlap, tie-breaks),
  and `dirty` / `clean` CRCs are compared with the installed files. What is ported and what is not is listed in
  [docs/COMPATIBILITY.md](docs/COMPATIBILITY.md).
* It does not run the game, Wine, Proton or MO2, and it does not download the masterlist or update it.
* `version` and `product_version` on executables and DLLs need the cargo feature `executable-versions` (below).
  An item guarded by a condition this crate cannot evaluate is not applied and is reported as unsupported (with a
  count at the end); it is never guessed.
* No Markdown rendering: message text is returned as the masterlist writes it.
* `check` looks only at the plugin list you give and does not sort. Its group findings are pairwise: it does not drop
  the group rules that `sort` drops because they clash with a master, `after` or `req` rule, so `check` can show a
  group note that `sort` (and LOOT) would not enforce. `sort` reports only the group rules it really dropped
  (`dropped_group_rules`).

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

* Unit tests next to each module (`src/*_tests.rs`): the plugin and record readers, the archive tables, the YAML subset
  (aliases, merge keys, localised content, group merging, regex names, bad input), the condition grammar and
  precedence, the version grammar, the sorter (libloot's own sorting tests, translated) and the generated messages.
* Integration tests (`tests/`) that run the library and the built binary on throwaway game directories whose plugin
  and archive files are generated byte by byte inside the test: golden check and sort cases, property tests over
  generated worlds, resource-limit tests, and a check that nothing under the fixture changed.
* The sorted order, messages, Bash Tags, groups and cleaning data are compared with the real libloot on random
  synthetic worlds, and the plugin readers with esplugin; see [docs/ORACLE.md](docs/ORACLE.md) for the runs and their
  results. The owner's real load order is compared locally and privately by `scripts/oracle/oracle_run.py`.
* Fuzzing, mutation testing and a weekly run against the real masterlist: see [docs/TESTING.md](docs/TESTING.md) and
  [docs/PRACTICES.md](docs/PRACTICES.md). `task check` runs what CI runs.

## Open questions

* Group violations are soft by default and hard with `--strict-groups`; see [docs/GROUPS.md](docs/GROUPS.md).
* The messages the LOOT application (not libloot) generates are ported from its source and documentation; they are not
  yet compared with the application's own output (issue #38).
