# Fuzz targets

Own workspace (`cargo-fuzz`, nightly toolchain). See [docs/TESTING.md](../docs/TESTING.md#fuzzing) for what each target
asserts.

| Target | Parser or property |
| --- | --- |
| `masterlist_yaml` | `metadata::parse`, the alias counter, and the conditions it yields |
| `condition` | `condition::eval_str`, and the print and parse round trip |
| `plugin_header` | `plugin::header_data_size`, `plugin::parse_header` |
| `plugin_roundtrip` | `plugin::synthetic_plugin` then `parse_header` |
| `plugins_txt` | `env::parse_plugins_txt` |
| `exe_version` | `exe_version::preflight` and `evaluate` (feature `executable-versions`) |
| `plugin_records` | `records::read_records_from` |
| `sort_graph` | `sort::sort`: a generated set of plugins, groups, rules and overlaps |

```sh
scripts/fuzz-run.sh                    # every target, 15 seconds each, seeded
scripts/fuzz-run.sh condition 600      # one target, ten minutes
```

`seeds/<target>/` holds small hand-made inputs (conditions, synthetic plugin headers). The pull request pipeline runs
every target briefly; the weekly workflow runs each for ten minutes. A crash is uploaded as an artifact; turn its
input into a unit test next to the parser before fixing it.
