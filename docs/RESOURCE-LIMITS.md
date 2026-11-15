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

| Executable version (cargo feature `executable-versions`) | 1024 data directories, 96 sections, a resource directory of at most 256 MiB of which only the few tables on the way to the version block are read (at most 64 tables of at most 256 entries), a version block of at most 64 KiB, every length inside a block checked against its parent and at least its own header, every offset computed in 64 bits and checked against the file | `exe_version::version_block` and `has_product_version`: the file is opened once and read with bounds, and only the validated block, inside a minimal image in a private temporary directory (mode 0700, removed afterwards), reaches `loot-condition-interpreter` | The interpreter's PE reader allocates from `u32` fields of the file (one as `capacity * 8` bytes), adds 32-bit offsets unchecked, reads the PE offset as 16 bits, maps the version data from the section address instead of the directory's, and loops forever on a string of length 0 (confirmed by `the_interpreter_alone_loops_forever_on_a_string_of_length_zero`, an ignored test). None of that sees the file. Because it opens only the copy, nothing can be swapped between the check and the use |

| Sorting (`api::sort`) | 4096 listed plugins, 1024 installed plugins (`sort::MAX_PLUGINS`), 1 billion steps of graph search (`sort::WORK_BUDGET`), 8 billion steps of record and archive comparison (`overlap::OVERLAP_WORK_BUDGET`), checksums read in 256 KiB blocks, at most 16 threads (`parallel::MAX_THREADS`) | `api::sort`, `sort::sort`, `overlap::PluginOverlaps::precompute` | The game loads 255 plugins. The graph keeps two `n * n` bit matrices (known paths and direct edges, 256 KiB together at the limit), its edges, and a table of one byte per pair of plugins for the overlap answers (1 MiB at the limit); every breadth-first search counts the vertices and edges it looks at, every comparison of two plugins' ids counts its steps, and the sort stops with `TooMuchWork` when either total passes its limit. The comparison steps of each pair are fixed by the pair, so the verdict does not depend on the threads. `tests/limits_sort.rs` sorts 1024 plugins with the heaviest shapes (every pair overlapping, a chain of 1023 rules listed backwards, 40 groups) in at most 40 MiB |
| Archive tables (`api::sort`) | 64 MiB of tables per archive (`archives::MAX_ARCHIVE_TABLES`) and 4,000,000 distinct files for all plugins together (`MAX_ASSETS_TOTAL`); the sizes in the header are multiplied in 64 bits and checked against the file's length before anything is allocated by them; only the header and tables are read, never the data; a folder offset before or past the tables is an error | `archives::read_archive_from` | A BSA with 300,000 files has about 10 MiB of tables, and the largest in the base game is far below that. A header that announces 4 million folders in a small file is refused without reading. An archive over a limit is `SortError::TooMuchWork`; one that cannot be read is noted and skipped, as libloot does. Read on several threads: a budget shared by the threads and taken in chunks bounds the memory while reading; whether the limit is passed is decided afterwards from the complete counts by the rule of a reader that goes one plugin after the other, and when the shared budget refuses, everything is read again that way, so the verdict never depends on the threads or on a `SortCache` |
| Plugin records (`api::sort`) | 4,000,000 records per plugin and 16,000,000 for all plugins together (`records::MAX_RECORDS_PER_PLUGIN`, `MAX_RECORDS_TOTAL`); every record size checked against the file's length before it is skipped, no recursion into groups, a 64 KiB read buffer | `records::read_records_from` | `FalloutNV.esm` has about 400,000 records; 255 plugins together stay far below the total. Eight bytes are kept per record, so the total is at most 128 MiB; the work of comparing two plugins is counted into the sorting work budget. A plugin that exceeds a limit is `SortError::TooMuchWork`; one that cannot be read is noted and counts as having no records. Read on several threads: a budget shared by the threads and taken in chunks bounds the memory while reading; whether the limit is passed is decided afterwards from the complete counts by the rule of a reader that goes one plugin after the other, and when the shared budget refuses, everything is read again that way, so the verdict never depends on the threads or on a `SortCache` |

All limits are errors, never panics and never silent truncation: the masterlist limits are `MetadataError`, the
condition limits are `ConditionError::Syntax`, the plugin limit is `PluginError::Truncated`, the file limits are exit
code 2 with a message naming the file.

## Speed, threads and caching (issue #33)

Measured with `task bench` (`examples/bench.rs`): a generated `Data` directory with one base master of 400,000 records,
three masters of 40,000, mods of 50 to 20,000 records (a third overriding base records), BSAs for one mod in four
(100 to 20,000 files) and a masterlist with groups, `after`, `req`, messages, tags, cleaning data and conditions for a
third of the plugins. Release build, 16 threads, files in the page cache, median of 5 to 7 runs. libloot 0.29.7 (the
Rust crate at `opt-level = 3`, `load_plugins` then `sort_plugins`) was timed on the same directories and gave the same
order for all three sizes.

| Plugins | Before (serial) | Now | Now, second sort with a `SortCache` | Now, 1 thread | libloot 0.29.7 |
| --- | --- | --- | --- | --- | --- |
| 133 | 113 ms | 29 ms | 14 ms | | 69 ms |
| 255 | 247 ms | 52 ms | 33 ms | 171 ms | 142 ms |
| 1024 | `TooMuchWork` (2.3 s without the limit) | 0.68 s | 0.63 s | | 3.0 s |

Peak resident memory of the 255-plugin sort is about 60 MiB (libloot: 55 MiB), of the 1024-plugin sort about 200 MiB
(libloot: 154 MiB). The time is the whole `api::sort` call, which also evaluates the masterlist, computes checksums and
the generated messages; libloot's numbers are the plugin load and the sort only.

What was decided, from those numbers:

* (a) Parallel reads, yes: headers, records, archive tables and checksums are read with `std::thread::scope`
  (`src/parallel.rs`, no new dependency), results in input order. Archive folder and file names are interned in a table
  split into 64 locked shards; each archive directory is listed once, not once per plugin. Every pair of plugins the
  overlap stage can ask about is compared ahead of the sort, on the threads, into a table.
* (b) A cache across calls, yes, opt-in: `SortCache` keeps record ids and archive files by path, size, modification time
  and inode. It is worth it for a caller that sorts the same files several times (profiles, a check after a sort).
* (c) Condition results are kept per sort (a condition's value does not depend on the order), compiled regular
  expressions too, and each file's checksum is read once. Masterlist entries are found through a name index instead of
  comparing every entry with every plugin.
* Not done: the graph search itself is serial, as libloot's is; it is most of the 1024-plugin time (about 350 ms) and
  changing it would change which edges are found. Splitting one large plugin's records over threads is left out: the base
  game's master is read in about 11 ms.

## How it is tested

* `tests/limits.rs` runs each hostile input (billion-laughs aliases, a merge-key bomb, a million nested brackets, an
  oversized masterlist, nested, chained and very long conditions, a header with a huge size field in bytes and in a
  small file, an oversized `plugins.txt`) and asserts an error, under 3 seconds and under 24 MiB of added peak
  resident memory. Measured locally, every case added at most 1.3 MiB.
* `tests/limits_sort.rs` sorts the largest load order in its heaviest shapes and asserts an order or `TooMuchWork`, in under
  64 MiB of added peak resident memory, reads a plugin whose records never end (refused at 4 million records, 33 MiB) and an archive that announces millions of folders.
* `tests/limits_exe.rs` (with the feature) does the same for hostile executables built in the test: huge numbers of
  data directories, a huge resource directory, a huge version block and a string table that would not advance.
  Each is refused (or false) in microseconds and a few hundred KiB.
* CI runs the same tests with an address-space limit (`task limits`: `prlimit --as=536870912`, 512 MiB), so a
  regression fails the build instead of using the runner's memory. The test also passes under 128 MiB.
* The real-masterlist tests (`tests/real_masterlist.rs`) are ignored by default and serialised with a mutex.
* Property tests use fixed small case counts (`PROPTEST_CASES` lowers them) and bounded generators.
* Local runs of anything that compiles or runs code use `capped <limit>`, one test binary and one thread at a time.
  Peak memory measured that way (maximum resident size of the test process): unit tests 33 MiB; each integration
  test binary 13 to 19 MiB.
* `cargo-mutants` and `cargo-fuzz` run only in CI. A mutant can make a loop that never advances and allocates
  without end; `mutants.yml` gives each mutant a 60 second timeout and the run an address-space limit.
