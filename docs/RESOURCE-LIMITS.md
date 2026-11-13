# Resource limits

`loot-rs` reads files it does not control (a masterlist from the internet, plugins from mods). Every size that
comes from the input is bounded in the library, so hostile or broken input is refused with an error that says what
the limit was, instead of being expanded in memory. The bounds are tested in `tests/limits.rs` (one test that
measures the rise in peak resident memory for each input) and in the unit tests.

| Input | Limit | Where | Why this number |
| --- | --- | --- | --- |
| Masterlist file | 8 MiB (`metadata::MAX_MASTERLIST_BYTES`, `cli::MAX_MASTERLIST_FILE`) | read with a cap, never more than the limit plus one byte | The real falloutnv masterlist is about 245 KB |
| YAML aliases | 200,000 nodes once every alias is counted as the node it stands for (`metadata::MAX_EXPANDED_NODES`) | counted from the YAML event stream before anything is cloned | The real masterlist expands to **19,816** nodes: ten times headroom. A "billion laughs" document is refused after counting, using about 1 MiB |
| Merge keys (`<<`) | twice that budget in work, and 256 levels of depth (`metadata::expand_merge_keys`) | each node visited or copied is charged first; the expansion stops at once when the budget runs out | The expansion clones maps, so it has its own budget instead of trusting the alias count |
| YAML nesting | 64 levels of `[` and `{`; 256 levels of collections | a scan before parsing; the alias counter | The real masterlist nests two levels |
| Condition text | 16 KiB (`condition::MAX_CONDITION_BYTES`) | checked before tokenising | The longest real condition is 256 bytes |
| Condition shape | 64 levels of parentheses and `not`, 2000 operators and calls (`MAX_NESTING`, `MAX_NODES`) | counted while parsing | Parsing, evaluating and dropping a tree recurse by its depth; this keeps the stack small |
| `plugins.txt` | 1 MiB (`cli::MAX_PLUGINS_FILE`) | read with a cap | A load order of 255 plugins is about 5 KB |
| Plugin header | 4 MiB of header data (`plugin::MAX_HEADER_DATA`) and never more than the file holds | the size field is checked against the file's real length before any buffer is allocated by it | Real headers are a few KiB; a file that claims 4 MiB and holds 30 bytes allocates nothing |

All limits are errors, never panics and never silent truncation: the masterlist limits are `MetadataError`, the
condition limits are `ConditionError::Syntax`, the plugin limit is `PluginError::Truncated`, the file limits are exit
code 2 with a message naming the file.

## How it is tested

* `tests/limits.rs` runs each hostile input (billion-laughs aliases, a merge-key bomb, a million nested brackets, an
  oversized masterlist, nested, chained and very long conditions, a header with a huge size field in bytes and in a
  small file, an oversized `plugins.txt`) and asserts an error, under 3 seconds and under 24 MiB of added peak
  resident memory. Measured locally, every case added at most 1.3 MiB.
* CI runs the same test with an address-space limit (`task limits`: `prlimit --as=536870912`, 512 MiB), so a
  regression fails the build instead of using the runner's memory. The test also passes under 128 MiB.
* The real-masterlist tests (`tests/real_masterlist.rs`) are ignored by default and serialised with a mutex.
* Property tests use fixed small case counts (`PROPTEST_CASES` lowers them) and bounded generators.
* Local runs of anything that compiles or runs code use `capped <limit>`, one test binary and one thread at a time.
  Peak memory measured that way (maximum resident size of the test process): unit tests 33 MiB; each integration
  test binary 13 to 19 MiB.
* `cargo-mutants` and `cargo-fuzz` run only in CI. A mutant can make a loop that never advances and allocates
  without end; `mutants.yml` gives each mutant a 60 second timeout and the run an address-space limit.
