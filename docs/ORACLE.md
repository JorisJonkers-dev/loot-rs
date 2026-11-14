# Differential testing against LOOT (the oracle)

**Status: the harness is complete and tested on invented data; no real LOOT run has been made, and nothing in this
repository records LOOT output.** Nothing here needs the game, and CI never runs libloot: CI runs the self-tests of the
scripts (`task scripts`) and the golden tests of the Rust side. The LOOT side runs on the owner's machine, where libloot
and (for a real load order) the game files are. Do not add "expected" LOOT results to the repository unless they came
from such a run (or can be derived from LOOT's public documentation, which for message text and ordering they cannot).

## What is compared

`loot-rs` reads a masterlist and reports what applies to a load order. LOOT's library, libloot
(<https://github.com/loot/libloot>, GPL-3.0, documented at <https://loot-api.readthedocs.io>), does the same and
more, and is the reference. For one load order (the synthetic ones under `tests/golden/`, or a real profile) the
two should agree on:

| What | loot-rs | libloot (names as in the public API documentation; check them against the release used) |
| --- | --- | --- |
| General messages | `general_messages` | the database's general messages, with conditions evaluated |
| Messages per plugin | `plugins[].messages` | the plugin's metadata from the database, conditions evaluated, including the messages LOOT generates itself from `req`, `inc` and the like |
| Bash Tags | `tags_add`, `tags_remove` | the plugin's metadata tags, conditions evaluated |
| Group | `plugins[].group` | the plugin's group as resolved by the database |
| Ordering constraints | `rules` of `loot-rs sort`, load-after findings of `check` | the order a LOOT sort produces |
| Cleaning data | `dirty` | the plugin metadata's dirty and clean info whose CRC is the installed file's |

Ordering is checked two ways: every `after`, `req` and header-master rule of `loot-rs sort`, and every constraint
`check` reports as violated, must hold in the order LOOT's sort produces (group rules are listed as notes); and
`loot-rs sort` is a port of LOOT's algorithm, so on the synthetic cases, which have no records in common, the two
orders should be **equal**. Until the record and archive overlap data is read (see
[COMPATIBILITY.md](COMPATIBILITY.md)), a real load order in which plugins change the same records can still differ.

Known, expected differences (not bugs): `loot-rs` does not implement `filename_version()`,
`description_contains()` and `is_executable()`, and implements `version()` and `product_version()` only on
executables and DLLs with the cargo feature `executable-versions` (build with `--features executable-versions` for the
comparison), so other items that use them are reported as unsupported and left out (see
[TESTING.md](TESTING.md)); LOOT localises and renders Markdown in messages, `loot-rs` keeps the English text with
substitutions applied; LOOT adds messages of its own (for example about missing masters or inactive requirements)
that `loot-rs` does not generate.

## How to run it

The pieces, all under `scripts/oracle/` (Python 3, standard library only):

| Script | Does |
| --- | --- |
| `materialise.py` | builds the `Data` directory, `plugins.txt` and `masterlist.yaml` of a golden case on disk, with the same synthetic plugin bytes the Rust tests write; `ensure_master` adds an empty `FalloutNV.esm` because libloot refuses a game directory without it |
| `loot_driver.py` | the libloot side: opens the game directory with libloot's Python bindings (`loot_api`), loads the masterlist, loads the plugins (headers only), asks the database for the evaluated general messages and per-plugin metadata, runs LOOT's sort, and writes `loot.json` |
| `compare.py` | compares `ours.json` (`loot-rs sort --json`) with `loot.json`; its header documents both shapes |
| `run.py` | does all of it for one case, all cases, or a real load order, and keeps the JSON files under `--work` |

### On the synthetic cases (no game)

You need libloot's Python bindings (`loot_api`, from <https://github.com/loot/libloot>, GPL-3.0; the version is whatever
you install, write it down with the results). Then:

```sh
cargo build --release
scripts/oracle/run.py --loot-rs target/release/loot-rs \
  --driver "python3 scripts/oracle/loot_driver.py" --all --work ~/oracle-work
```

`--all` runs the 22 cases of `tests/golden` and the 6 of `tests/golden_sort`; `--case tests/golden_sort/groups` runs one.
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
