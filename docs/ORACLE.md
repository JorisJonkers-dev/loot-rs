# Differential testing against LOOT (the oracle)

**Status: the sorted order has been compared with the real libloot (0.29.7, the Rust crate, built from source) on 3,300
random synthetic load orders with no difference (below). Messages, Bash Tags and cleaning data have not been compared
with libloot's output, and no real load order has been run.** Nothing here needs the game, and CI never runs libloot: CI
runs the self-tests of the scripts (`task scripts`) and the golden tests of the Rust side. The LOOT side runs on the
owner's machine, where libloot and (for a real load order) the game files are. Do not add "expected" LOOT results to the
repository unless they came from such a run (or can be derived from LOOT's public documentation, which for message text
and ordering they cannot).

## The differential test on random load orders

`scripts/oracle/random_diff.py` makes random worlds (2 to 12 or more invented plugins, some masters, header masters,
records that overlap, BSA archives that overlap, groups with `after` lists, `group`, `after` and `req` entries, a random
current load order), sorts each with `loot-rs sort` and with libloot, and requires the same order (or both refusing).
The libloot side is `scripts/oracle/libloot-driver`, a small program on the libloot crate with its own workspace, built
only by whoever runs the test:

```sh
cargo build --release --manifest-path scripts/oracle/libloot-driver/Cargo.toml
cargo build --release
scripts/oracle/random_diff.py --loot-rs target/release/loot-rs \
  --driver scripts/oracle/libloot-driver/target/release/libloot-driver --count 500 --seed 1 --max-plugins 12
```

(`task oracle:random` does the same.) A run on 2026-11-14 against libloot 0.29.7: 3,300 worlds (seeds 101 to 103, 400
worlds each of up to 12 plugins; seed 7, 300 worlds of up to 30; earlier unrecorded runs of 600, 800 and 400 worlds of
up to 9, 12 and 25 plugins), **0 differences**. About nine worlds in ten are re-ordered by the sort, and about a third
sort differently when their records and archives are removed, so the comparison does exercise the overlap edges, the
group walk and the tie-break pinning. A world in which libloot refuses (a cycle) is one where `loot-rs` refuses too.
This is the evidence for `docs/COMPATIBILITY.md`'s claim that the sort is libloot's.

## What is compared

`loot-rs` reads a masterlist and reports what applies to a load order. LOOT's library, libloot
(<https://github.com/loot/libloot>, GPL-3.0, documented at <https://loot-api.readthedocs.io>), does the same and
more, and is the reference. For one load order (the synthetic ones under `tests/golden/`, or a real profile) the
two should agree on:

| What | loot-rs | libloot (names as in the public API documentation; check them against the release used) |
| --- | --- | --- |
| General messages | `general_messages` | the database's general messages, with conditions evaluated |
| Messages per plugin | `plugins[].messages` | the plugin's metadata from the database, conditions evaluated |
| Bash Tags | `tags_add`, `tags_remove` | the plugin's metadata tags, conditions evaluated |
| Bash Tags in the description | `bash_tags` | the plugin's `bash_tags()` |
| Group | `plugins[].group` | the plugin's group as resolved by the database |
| Ordering constraints | `rules` of `loot-rs sort`, load-after findings of `check` | the order a LOOT sort produces |
| Cleaning data | `dirty` | the plugin metadata's dirty and clean info whose CRC is the installed file's |

Ordering is checked two ways: every `after`, `req` and header-master rule of `loot-rs sort`, and every constraint
`check` reports as violated, must hold in the order LOOT's sort produces (group rules are listed as notes); and
`loot-rs sort` is a port of LOOT's algorithm, so on the synthetic cases, which have no records in common, the two
orders should be **equal**. The record and archive overlap data are read as libloot reads them (see [COMPATIBILITY.md](COMPATIBILITY.md)); a real load order that differs from LOOT's order is a case to examine, not an expected difference.

Known, expected differences (not bugs): `loot-rs` does not implement `filename_version()`,
`description_contains()` and `is_executable()`, and implements `version()` and `product_version()` only on
executables and DLLs with the cargo feature `executable-versions` (build with `--features executable-versions` for the
comparison), so other items that use them are reported as unsupported and left out (see
[TESTING.md](TESTING.md)); LOOT localises and renders Markdown in messages, `loot-rs` keeps the English text with
substitutions applied; LOOT's application adds messages of its own (missing masters, inactive requirements, dirty plugins); libloot does
not, so they are in `generated_messages` and are not compared with libloot's output.

## How to run it

The pieces, all under `scripts/oracle/` (Python 3, standard library only):

| Script | Does |
| --- | --- |
| `materialise.py` | builds the `Data` directory, `plugins.txt` and `masterlist.yaml` of a golden case on disk, with the same synthetic plugin bytes the Rust tests write; `ensure_master` adds an empty `FalloutNV.esm` because libloot refuses a game directory without it |
| `loot_driver.py` | the libloot side: opens the game directory with libloot's Python bindings (`loot_api`), loads the masterlist, loads the plugins (in full, because the sort uses their records), asks the database for the evaluated general messages and per-plugin metadata, runs LOOT's sort, and writes `loot.json` |
| `compare.py` | compares `ours.json` (`loot-rs sort --json`) with `loot.json`; its header documents both shapes |
| `run.py` | does all of it for one case, all cases, or a real load order, and keeps the JSON files under `--work` |
| `random_diff.py`, `libloot-driver/` | the differential test on random load orders against the libloot crate (above) |

### On the synthetic cases (no game)

You need libloot's Python bindings (`loot_api`, from <https://github.com/loot/libloot>, GPL-3.0; the version is whatever
you install, write it down with the results). Then:

```sh
cargo build --release
scripts/oracle/run.py --loot-rs target/release/loot-rs \
  --driver "python3 scripts/oracle/loot_driver.py" --all --work ~/oracle-work
```

`--all` runs the 22 cases of `tests/golden` and the 10 of `tests/golden_sort`; `--case tests/golden_sort/groups` runs one.
Without `--driver`, or when `loot_api` is not installed, it still runs `loot-rs` on every case, prints `skipped` and
exits 3: that means nothing was compared, never "no difference". Exit codes: 0 compared and no difference, 1
differences, 2 an error, 3 skipped.

### On a real load order (the owner, locally)

Read only: nothing is written to the game directory, `plugins.txt` or the masterlist.

```sh
scripts/oracle/run.py --loot-rs target/release/loot-rs \
  --driver "python3 scripts/oracle/loot_driver.py" --real \
  --game-path ~/Games/FNV            `# the directory that holds Data/` \
  --data ~/Games/FNV/Data \
  --masterlist ~/Downloads/masterlist.yaml \
  --plugins ~/.config/mo2/profiles/Default/plugins.txt \
  --work ~/oracle-work
```

With Mod Organizer 2 give the profile's `plugins.txt` and a `Data` directory that already shows the merged files (the
virtual file system's output, or a `Data` folder that you linked together). Use `--plugins` with the load order
you want compared; the order of its lines is the load order given to both tools. Keep `--work` out of any repository.
Mod names, paths and load orders belong to you: do not paste them into issues or commits.

Reading the result: `ours.json` and `loot.json` are the two sides; `differences.txt` lists the differences. A line
starting with `info:` says how many pairs of plugins are in a different order in the two sorts, which is expected
(it should be 0 on the synthetic cases; real load orders can differ until the overlap data is read); an `order differs` line is a difference. `note:` lines are group rules that LOOT's order does not follow, which is
worth a look but not a difference; everything else is a difference to explain: a bug here, or an entry in the list of
known differences below.

`python3 -I scripts/oracle/<script>.py --selftest` tests each script on invented data (they run in `task scripts`);
they say nothing about LOOT.

## What a first run should answer

* Do the golden cases agree on messages and tags? Each disagreement is either a bug here or a documented limit.
* Does the real masterlist (`LOOT_RS_MASTERLIST`, see [TESTING.md](TESTING.md)) over a real profile give the same
  messages, and does LOOT's order satisfy every `after`, `req` and header-master rule of `loot-rs sort`?
* Is `constraint` (the `req` and `inc` condition that decides whether a file counts as present) read the way LOOT
  reads it? The golden case `req_conditional` encodes one reading.

When a run on the synthetic cases exists, store LOOT's output next to the case as `tests/golden*/<case>/loot.json`
together with the libloot version it came from (synthetic inputs only; never the output of a real load order), and make
`compare.py` part of the golden test.

## Real load orders, privately: `oracle_run.py`

`scripts/oracle/oracle_run.py` (`task oracle:real`) runs `loot-rs sort` and the Rust libloot driver on the owner's real
load order and compares the two orders. It is local only: the paths live in `~/.config/nvmod/oracle.toml`, outside any
repository (the script refuses a config inside a git work tree, and refuses to write its output inside one; the config
name and the log folders are also in `.gitignore` as a second line of defence). The config format is in the script's
docstring. It reads the TES4 header of each plugin for the dataset digest and lists the BSA files; it never reads other
game files and never writes to the game or to an MO2 instance. Plugins are looked up through the enabled mods of an MO2
`modlist.txt` (highest priority first) and then the game's `Data`, and are symlinked into a staging directory, so libloot
never sees the real game directory.

Three levels of output, under `~/.local/share/nvmod-oracle/<run-id>/` (mode 0700):

| Level | File | Content |
| --- | --- | --- |
| L2 | `l2/` | everything: names, both sorted orders, the differences, the errors. Local only |
| L1 | `l1.json` | redacted: hashed ids, counts, digests, a hash per difference. No names, paths or text |
| L0 | `l0.json`, stdout | `{suite, status, n_datasets, n_plugins, n_missing, n_differences, digest, tool}` |

Status is `pass`, `fail`, `skipped` (no driver: nothing compared) or `partial` (compared and equal, but some plugins of
the load order were not found, so fewer were compared than the order has; never a green gate). Exit codes 0, 1, 3, 4
respectively; 2 is a usage or config error. libloot loads plugins in full, in memory, to sort them; only the order it
produces is printed or kept. Run by hand this is phase 0 of ADR 0002's private oracle testing: it opens no issue and posts no status.

### After a merge: the private trigger

The script itself is unchanged; a private runner calls it after every push to `main` and on owner dispatch, and the
result becomes the commit status `digest/real-data` that the release gate reads. Two workflows, because a self-hosted
runner is never attached to a public repository:

* `.github/workflows/oracle-trigger.yml` (public, GitHub-hosted, no data, no `pull_request` trigger) marks the commit
  pending and dispatches the private repository `oracle-ci`.
* `scripts/oracle/oracle-ci/real-data.yml` is the template of that private workflow. It builds the commit, runs
  `oracle_run.py` with the config on the runner, and lets `oracle_ci_report.py` (allow-list, selftested) turn the L0 line
  into the status and an issue. A failure opens or comments on the issue "Real-data oracle is red" with counts and the
  digest only; a later green run closes it. Only `pass` is green: `skipped`, `partial`, `fail` and an error are red.
  Nothing is reverted.

The owner-only setup (runner, tokens, config, caps) is in [runbooks/private-oracle-runner.md](runbooks/private-oracle-runner.md).
