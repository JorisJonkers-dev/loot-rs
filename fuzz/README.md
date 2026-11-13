# Fuzz targets

Own workspace (`cargo-fuzz`, nightly toolchain). One target per parser:

| Target | Parser | Asserts |
| --- | --- | --- |
| `masterlist_yaml` | `metadata::parse` and the conditions it yields | no panic, no stack overflow |
| `condition` | `condition::eval_str` over a fixed environment | no panic, no stack overflow |
| `plugin_header` | `plugin::header_data_size`, `plugin::parse_header` | no panic, no out-of-bounds read |
| `plugins_txt` | `env::parse_plugins_txt` | no panic |

```sh
cd fuzz
cargo +nightly fuzz run condition -- -max_total_time=60
```

`.github/workflows/fuzz.yml` runs each target for ten minutes every week. A crash is uploaded as an artifact; turn
its input into a unit test next to the parser before fixing it.
