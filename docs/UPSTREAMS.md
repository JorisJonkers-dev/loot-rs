# Upstreams and licences

Read from the `LICENSE` file (and, for the Rust crates, the `license` field of `Cargo.toml`) of each
repository on 2026-11-13, through the GitHub API. The licence claims in the research notes this
project started from were from memory; the table below replaces them.

| Project | Repository | Licence found | Evidence |
| --- | --- | --- | --- |
| LOOT (application) | https://github.com/loot/loot | GPL-3.0 | `LICENSE` is the GNU GPL, version 3 |
| libloot (the library LOOT is built on) | https://github.com/loot/libloot | GPL-3.0 | `LICENSE` is the GNU GPL, version 3. Not MIT, as the research notes guessed |
| esplugin (plugin parsing) | https://github.com/Ortham/esplugin | GPL-3.0 | `LICENSE`; `Cargo.toml` `license = "GPL-3.0"` (crate `parsing` 6.1.4). Lives under the author's account, not `loot/` |
| libloadorder (load order handling) | https://github.com/Ortham/libloadorder | GPL-3.0 | `LICENSE`; `Cargo.toml` `license = "GPL-3.0"` (crate `load_order` 18.8.2). Lives under the author's account, not `loot/` |
| loot-condition-interpreter | https://github.com/loot/loot-condition-interpreter | MIT | `LICENSE` is "The MIT License (MIT), Copyright (c) 2018 Oliver Hamlet"; `Cargo.toml` `license = "MIT"` (crate `eval` 6.0.0). Not GPL |
| Masterlist for Fallout: New Vegas | https://github.com/loot/falloutnv | CC0-1.0 | `LICENSE` is "Creative Commons Legal Code, CC0 1.0 Universal". The README says nothing about a licence |
| Masterlist prelude, other games | https://github.com/loot/prelude, https://github.com/loot/skyrimse | CC0-1.0 | `LICENSE` files, same text |

Which version each licence was read from is the default branch at the time of reading, not a pinned tag.

## What this repository takes from them

* **LOOT, libloot**: the rule semantics, read from the public metadata and condition-language
  documentation (`docs/metadata/` in libloot, published at https://loot-api.readthedocs.io). No source
  code is copied in this slice. This repository is GPL-3.0 to follow them, so adapting their code later is
  allowed with attribution and the licence kept.
* **esplugin, libloadorder**: nothing. The plugin header reader is `src/plugin.rs`, which comes from
  fnvorder (MIT OR Apache-2.0), see `NOTICE.md`.
* **loot-condition-interpreter** (6.0.0): a dependency, but only behind the cargo feature
  `executable-versions`, for `version()` and `product_version()` on executables and DLLs (PE version resources
  and LOOT's version comparison). The default build does not contain it. The crate is MIT; its dependency
  `esplugin` is GPL-3.0 (see above), which `deny.toml` allows because this project is GPL-3.0. The other
  functions are still evaluated by `src/condition.rs`.
* **Masterlist data**: nothing is copied into this repository. The tool reads a masterlist from a path
  the user gives. Tests use small YAML written inside the tests. CC0 would permit copying, but the
  masterlist is large, changes daily and may carry third-party text, so it stays out of the tree.
