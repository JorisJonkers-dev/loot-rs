# Testing

Everything is deterministic: no network, no clock, no randomness that is not seeded, no game, Wine, Proton or
MO2. The one exception by design is the ignored real-masterlist test, which reads a file you give it.

```sh
task test        # cargo test: unit, property, golden and CLI tests
task coverage    # the same under cargo-llvm-cov; fails under 95 percent of lines
task check       # everything CI runs
```

On NixOS run cargo inside `nix shell nixpkgs#gcc nixpkgs#lld nixpkgs#cargo-llvm-cov` with
`RUSTFLAGS="-Clink-self-contained=-linker"` (see `.cargo/config.toml`).

## Inventory

| Kind | Where | Count | What it checks |
| --- | --- | --- | --- |
| Unit | `src/*_tests.rs` | 135 | Every module: the condition parser and evaluator, the masterlist reader, the plugin header reader, the file-system environment, the checker, the text and JSON renderers, argument parsing and the `check` command |
| Property | `tests/prop_condition.rs`, `prop_metadata.rs`, `prop_plugin.rs` | 44 properties, 128 to 256 cases each | See below |
| Golden | `tests/golden.rs`, `tests/golden/<case>/` | 22 cases (5 also in JSON) | The binary's output and exit code on synthetic game directories, one case per rule kind |
| Sorter (libloot port) | `src/sort/plugins_tests.rs`, `src/sort_tests.rs`, `tests/prop_sort.rs`, `tests/limits_sort.rs` | 50 + 20 + 5 properties + 1 | libloot's own unit tests of its plugin graph, translated (group edges across branches, merges and skipped edges, early loaders, overlap edges, tie-break pinning), then the whole sort (masters, rules, errors, cycles, overlap, limits). Properties: the result is a permutation that holds every edge the sorter added and every rule, errors exist only when an independent model finds a real conflict, sorting is deterministic and a sorted order is a fixed point, a plugin that changes more records loads first, large random inputs finish |
| Library API | `src/api_tests.rs`, `tests/api.rs`, `tests/prop_sort.rs` | 17 + 5 + 1 property | `api::sort` over synthetic plugins (headers, `after`, `req`, groups, strict groups, case, duplicates, unreadable headers, cleaning data by CRC, errors) and a consumer of the public API from outside the crate; random load orders sort to an order that holds every rule and is stable |
| Binary, end to end | `tests/cli.rs` | 15 | Whole runs on generated plugins, including that nothing is written |
| Golden, sort | `tests/golden_sort.rs`, `tests/golden_sort/<case>/` | 6 cases (text and JSON) | `loot-rs sort`: masters first, header masters (LOOT keeps the load order of the plugins that need not move), `after` and `req`, groups, a dropped group rule, messages with cleaning data |
| Binary, errors | `tests/cli_errors.rs` | 25 | Exit code 2 and its messages: arguments, bad or unreadable masterlist, plugin list, Data directory, unreadable plugins |
| Executable versions (feature) | `src/exe_version_tests.rs`, `tests/limits_exe.rs` | 25 + 1 (+1 ignored) | `version()` and `product_version()` on synthetic PE images built in the test (no game files): both argument orders, LOOT's comparison, case-insensitive paths, missing and non-PE files, plugins still unsupported, hostile sizes, lengths that do not fit, a directory that does not start its section, a PE offset beyond 16 bits, file names with quotes and control characters, the resource walk's bounds, every truncation and byte flip. The ignored test demonstrates the interpreter's own hang. `task test:features` runs them; CI does too |
| Resource limits | `tests/limits.rs` | 1 (11 hostile inputs) | Every size that comes from the input is bounded; see [RESOURCE-LIMITS.md](RESOURCE-LIMITS.md). CI runs it under a 512 MiB address-space limit |
| Real data (ignored) | `tests/real_masterlist.rs` | 2 | The real falloutnv masterlist; run weekly, see below |
| Fuzz | `fuzz/` | 6 targets | Every parser and two round trips, with libFuzzer: 15 seconds each on every pull request (`Fuzz smoke` job of the pipeline), ten minutes each weekly |
| Mutation | `.github/workflows/mutants.yml` | the condition evaluator | cargo-mutants (weekly) |

Property tests use `proptest` 1.12.0 (published 2026-10-10, MIT OR Apache-2.0, more than the registry proxy's seven
days old; the `fork` and `timeout` features are off so `rusty-fork` and `tempfile` stay out of the tree). The seed is
fixed, so a run generates the same cases every time and a failure is reproducible; set `LOOT_RS_PROPTEST_SEED=<n>` to
explore other cases locally.

### Properties

* **Condition language.** Printing an expression and parsing it gives the expression back, and printing is a fixed
  point; padding between tokens changes nothing. Arbitrary text and token soup never panic, and every parse error is
  a syntax error. Nesting beyond 64 levels is refused, not crashed on. Over a model of the file system, `and`, `or`
  and `not` are conjunction, disjunction and negation, and satisfy double negation, commutativity, associativity,
  distributivity, De Morgan, idempotence, absorption and complement; `not` binds tighter than `and`, which binds
  tighter than `or`. Each function agrees with the model. An unsupported function only matters when it decides
  the result (`false and unsupported` is false, `true or unsupported` is true, anything else is the error).
* **Masterlist reader.** Arbitrary text, arbitrary bytes read lossily and YAML-shaped token soup never panic. A
  generated masterlist is read back field by field. An alias reads the same as the node it points to; a merge key
  supplies what the node does not say and explicit keys win; in a list of merge sources the earlier one wins. A name
  with a regex marker matches whole names, case-insensitively.
* **Plugin header reader.** Arbitrary bytes, bytes that start like a plugin, subrecord soup inside a valid header and
  any single flipped byte never panic or read out of bounds. A synthetic plugin reads back what was written; every
  truncation is an error, never a partial header; a master list cut inside the data keeps only whole masters; a file
  reads the same as its bytes.
* **plugins.txt and CRC-32.** Arbitrary text never panics; comments and blank lines are skipped; the star style marks
  exactly the starred lines active; CRC-32 matches a bitwise reference and detects every single-bit flip.

### Golden cases

`tests/golden/<case>/` holds `masterlist.yaml`, `plugins.txt`, `data.txt` (the `Data` directory to build: synthetic
plugins and small files), optional `args`, and `expected.stdout`, `expected.code` (and `expected.json` for five
cases). `UPDATE_GOLDEN=1 cargo test --test golden` rewrites the expected files; read the diff, because a golden file
is a claim about LOOT's rules.

| Rule kind | Cases |
| --- | --- |
| `req` | `req_missing`, `req_satisfied`, `req_loads_later`, `req_conditional` (a condition and a constraint) |
| `inc` | `inc_present`, `inc_absent` |
| `after` | `after_violated`, `after_satisfied` |
| `group` | `group_soft`, `group_strict` (`--strict-groups`), `group_ok`, `group_undefined`, `group_cycle` |
| `tag` | `tag_suggestions` |
| `msg` with conditions | `msg_conditions` (`file`, `active`, `many`, `is_master`, `checksum`, `file_size`, `readable`, regex paths, substitutions, translations, multi-line text), `merge_keys_and_anchors` |
| dirty and clean data | `dirty_clean` (read, deliberately not applied: the output is quiet) |
| unsupported | `unsupported_conditions` |
| other | `regex_entries`, `not_installed_and_inactive`, `empty_load_order`, `everything` |
| exit 0 | most cases; exit 1: `req_missing`, `req_loads_later`, `req_conditional`, `inc_present`, `after_violated`, `group_strict`, `everything`; exit 2 is in `cli_errors.rs` |

### Coverage per module

Line coverage by `cargo llvm-cov` (measured before the resource limits were added; the job summary in CI has the
current numbers), unit-test files not counted (`task coverage`; CI prints the same table in the job
summary). The gate is 95 percent of lines; the organisation requires 80.

| Module | Lines | Missed | Line coverage |
| --- | ---: | ---: | ---: |
| `check.rs` | 302 | 1 | 99.67% |
| `cli.rs` | 66 | 0 | 100.00% |
| `condition.rs` | 241 | 1 | 99.59% |
| `env.rs` | 183 | 15 | 91.80% |
| `main.rs` | 7 | 0 | 100.00% |
| `metadata.rs` | 299 | 2 | 99.33% |
| `plugin.rs` | 152 | 0 | 100.00% |
| `render.rs` | 168 | 0 | 100.00% |
| **Total** | **1418** | **19** | **98.66%** |

What is not covered in `env.rs`: the branches that only run when the file system misbehaves (a directory that vanishes
between listing and reading).

## The real masterlist (ignored by default)

`tests/real_masterlist.rs` runs against the real masterlist for Fallout: New Vegas, community data under CC0-1.0
(<https://github.com/loot/falloutnv>). It is never committed. Give the tests a copy:

```sh
curl -sL -o /tmp/masterlist.yaml https://raw.githubusercontent.com/loot/falloutnv/v0.29/masterlist.yaml
LOOT_RS_MASTERLIST=/tmp/masterlist.yaml task real
```

They assert that the file parses, that every condition in it is valid syntax, and that 400 plugin names named by the
masterlist are checked within `LOOT_RS_BUDGET_SECS` (default 10), and print the counts. `.github/workflows/masterlist.yml`
runs them every week against the current file. Result on `v0.29` (release build, a loaded machine):

| | |
| --- | --- |
| Alias-expanded YAML nodes | 19,816 (limit 200,000) |
| Plugin entries | 880 (129 are regular expressions, none skipped by this engine) |
| Groups, Bash Tags, global messages | 25, 78, 14 |
| Condition strings (aliases expanded) | 400, all valid syntax except one: `notfile(` in an `and` (a typo in the masterlist; it is reported as a condition error and the item is not applied) |
| Conditions that use an unsupported function | 117 (29%): `version()` in 115, `product_version()` in 3 |
| Parse time | about 25 ms |
| 400 plugins checked | about 40 ms (budget 10 s) |

Textually the masterlist calls `version()` 17 times (9 on executables or DLLs, 8 on plugins) and
`product_version()` 3 times; aliases multiply those into the 117 expanded conditions.

## Differential testing against LOOT

The harness is done, the LOOT runs are not: see [ORACLE.md](ORACLE.md). `scripts/oracle/` holds the fixture builder, the
libloot driver, the comparison and the runner, with self-tests on invented data (`task scripts`, in CI), and
`task oracle` runs `loot-rs sort` on the 28 cases (and compares with LOOT when `ORACLE_DRIVER` names a driver). No LOOT
output is recorded anywhere, and CI never needs libloot or the game.

## Fuzzing

`fuzz/` is a separate cargo-fuzz workspace. Targets, and what each asserts beyond "no panic, no hang, no runaway
allocation":

| Target | Input | Also asserted |
| --- | --- | --- |
| `masterlist_yaml` | masterlist text | the alias counter and the reader agree on what is too big; every condition the masterlist carries is parsed |
| `condition` | condition text | what parses prints to text that parses back to the same expression (and prints the same); both evaluate alike |
| `plugin_header` | plugin bytes | a parsed header's announced size fits the bytes |
| `plugin_roundtrip` | a generated plugin (masters, flags, description) | it reads back what was written; no cut of it reads as a different header |
| `plugins_txt` | `plugins.txt` bytes | no empty inactive names |
| `exe_version` | a valid synthetic PE image with bytes overwritten | the bounded walk and the interpreter (feature `executable-versions`) answer and return |
| `sort_graph` | generated plugins (master flags, masters, `req`, `after`, groups), groups and record and asset overlaps | an error names plugins that exist; an order is a permutation that holds every edge the sorter added, leaves one possible order, puts the masters first |

`scripts/fuzz-run.sh` runs them, seeded from `fuzz/seeds/<target>/` (and, for the masterlist, the synthetic masterlists
of the golden tests): `task fuzz:smoke` for `FUZZ_SECONDS` (default 15) each, or `scripts/fuzz-run.sh <target> <seconds>`.
The `Fuzz smoke` job of `.github/workflows/ci.yml` runs it on every pull request that changes code and is part of
`Pipeline Complete`; `.github/workflows/fuzz.yml` runs each target for ten minutes every Monday. The targets need a
nightly toolchain and `cargo-fuzz`; they type-check on stable (checked here). A crash input becomes a unit test next to
the parser before it is fixed.

Two limits the tests found while being written, both fixed: a condition with a very long or deeply nested chain
overflowed the stack when parsed, evaluated or dropped (now limited to 64 levels of nesting and 2000 operators and
calls), and a YAML document whose aliases multiply on every level expanded to billions of nodes before it was read
(now refused above 200,000 nodes; the real masterlist has 19,816).

## Optional features

`executable-versions` is off by default; `task test:features` runs everything with it (clippy runs both ways, `task lint`).
Tests that assert the default behaviour (an unsupported `version()` on a missing executable) are compiled out with the
feature, the executable tests only exist with it.

## Mutation testing

`.github/workflows/mutants.yml` runs cargo-mutants on `src/condition.rs` every Tuesday (not required). Not run locally: a mutant can turn a loop into one that
never advances and allocates without end, which is what froze a machine once, so mutation testing runs only in CI
(see [RESOURCE-LIMITS.md](RESOURCE-LIMITS.md)). The first CI run reports the surviving mutants. Ten mutants of the
tokeniser and parser were run before the run was stopped; all ten were caught.
