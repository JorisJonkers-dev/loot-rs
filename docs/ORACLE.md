# Differential testing against LOOT (the oracle)

**Status: planned. No real LOOT run has been made, and nothing in this repository records LOOT output.** The
harness below compares two files; the LOOT side has to be produced on a machine that has libloot. Do not add
"expected" LOOT results to the repository unless they came from such a run (or can be derived from LOOT's public
documentation, which for message text and ordering they cannot).

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
| Ordering constraints | load-after findings | the order a LOOT sort produces |

Ordering is only checked one way: every constraint `loot-rs` reports as violated (a plugin that has to load after
another and does not) must hold in the order LOOT's sort produces. The other direction is not comparable: LOOT
also orders by master flags, overlap and group, which `loot-rs` does not do.

Known, expected differences (not bugs): `loot-rs` does not implement `version()`, `product_version()`,
`filename_version()`, `description_contains()` and `is_executable()`, so items that use them are reported as
unsupported and left out (about 29% of the conditions in the current falloutnv masterlist, see
[TESTING.md](TESTING.md)); LOOT localises and renders Markdown in messages, `loot-rs` keeps the English text with
substitutions applied; LOOT adds messages of its own (for example about missing masters or inactive requirements)
that `loot-rs` does not generate.

## How to run it

1. Materialise a case on disk, with the same plugin bytes the Rust tests use:

   ```sh
   scripts/oracle/materialise.py tests/golden/req_missing /tmp/oracle/req_missing
   ```

   This writes `Data/` (synthetic header-only plugins and files), `plugins.txt` and `masterlist.yaml`.

2. Run `loot-rs`:

   ```sh
   loot-rs check --json --masterlist /tmp/oracle/req_missing/masterlist.yaml \
     --plugins /tmp/oracle/req_missing/plugins.txt --data /tmp/oracle/req_missing/Data > ours.json
   ```

3. Run LOOT on the same inputs with a small driver written against libloot (not in this repository, because it
   needs libloot and a C++ or Python toolchain). The driver must write JSON in the shape documented at the top of
   `scripts/oracle/compare.py`. Its steps, in terms of the documented API: create a game handle for Fallout: New
   Vegas on the materialised `Data` directory; load the masterlist with an empty userlist; load the plugins listed in
   `plugins.txt` (headers only is enough for synthetic plugins); for the general messages and each plugin ask the
   database for the evaluated metadata; run the sort and write its result as `sorted`. LOOT itself has no command
   that prints a report, so the library is the way in.

4. Compare:

   ```sh
   scripts/oracle/compare.py --ours ours.json --loot loot.json
   ```

   Exit status 0 means no difference, 1 prints the differences, 2 is a usage error.

`python3 -I scripts/oracle/compare.py --selftest` and `python3 -I scripts/oracle/materialise.py --selftest` test the
two scripts on invented data (they run in `task scripts`); they say nothing about LOOT.

## What a first run should answer

* Do the golden cases agree on messages and tags? Each disagreement is either a bug here or a documented limit.
* Does the real masterlist (`LOOT_RS_MASTERLIST`, see [TESTING.md](TESTING.md)) over a real profile give the same
  messages for the 400 plugins of the benchmark?
* Is `constraint` (the `req` and `inc` condition that decides whether a file counts as present) read the way LOOT
  reads it? The golden case `req_conditional` encodes one reading.

When a run exists, store LOOT's output next to the case as `tests/golden/<case>/loot.json` together with the libloot
version and the masterlist revision it came from, and make `compare.py` part of the golden test.
