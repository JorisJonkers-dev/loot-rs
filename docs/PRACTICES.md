# Engineering practices

What the repository does to stay correct and safe, and where each practice lives. Modelled on the `nv-rs` fork
baseline, adapted to a Linux-first Rust library and CLI (no Windows build, no upstream to follow).

| Practice | Where | Notes |
| --- | --- | --- |
| One required check, `Pipeline Complete` | `.github/workflows/ci.yml`, `.github/rulesets/main.json` | The Rust checks are steps of one job (the repository is private: billing is per job); hygiene and PR title are reusable workflows the last job waits for. Documentation-only changes skip the Rust job |
| Superseded runs are cancelled | `ci.yml` `concurrency` | Group per pull request number; a title edit has its own group; `main` is never cancelled; no label events trigger the pipeline |
| Every action pinned by SHA | all workflows | Version in a comment. The organisation's reusable workflows at v0.50.3 (SHA-pinned themselves). `setup-repo.sh` can require pinning |
| Format, clippy (`-D warnings`), tests | `task fmt`, `task lint`, `task coverage` | `task check` runs everything CI runs |
| Line-coverage gate | `Taskfile.yml` `COVERAGE_MIN`, `cargo-llvm-cov` | The organisation requires 80; this repository gates at 90. Per-module numbers: [TESTING.md](TESTING.md) |
| Licences, bans, sources, advisories | `deny.toml`, `task deny`, `advisories.yml` | Daily advisory run against the unchanged lockfile |
| Secret scanning | `.gitleaks.toml`, `task secrets` | Every commit, in CI and locally |
| Workflow lint and security | `actionlint`, `zizmor` (`task lint:actions`) | |
| Resource limits | `docs/RESOURCE-LIMITS.md`, `tests/limits.rs`, `task limits` | Bounds in the library on every input size; CI runs the bounded tests under `prlimit --as` |
| Library API | `src/api.rs`, `docs/API.md` | `#[non_exhaustive]` structs and enums, a documented contract and semver policy, `tests/api.rs` as an outside consumer |
| Optional feature | `Cargo.toml` `executable-versions`, `src/exe_version.rs` | `loot-condition-interpreter` only behind the feature; default build pure; clippy and tests run both ways (`task lint`, `task test:features`) |
| Fuzzing | `fuzz/`, `scripts/fuzz-run.sh`, `.github/workflows/fuzz.yml`, the `Fuzz smoke` job in `ci.yml` | Six targets, seeded; 15 seconds each on every pull request, ten minutes each weekly; needs `cargo +nightly` |
| Mutation testing | `.github/workflows/mutants.yml` | Weekly, condition evaluator; not required |
| Real masterlist | `.github/workflows/masterlist.yml`, `tests/real_masterlist.rs` | Weekly; the data is downloaded into the runner's temporary directory, never committed |
| Conventional Commits | `pr-title` job, `scripts/hooks/commit-msg` | The pull request title is the squash commit, so it is what release-please reads |
| Releases | `release-please-config.json`, `.release-please-manifest.json`, `release.yml` | `release-type: rust`, see below. No crates.io publication |
| Dependency cooldown | `.cargo/config.toml`, `renovate.json` | Cargo ignores versions younger than 7 days from 1.100 on (1.99 warns that it ignores the setting); Renovate has the estate preset's `minimumReleaseAge`. The registry proxy refuses younger versions |
| Dependency updates | `renovate.json` | Extends `github>JorisJonkers-dev/renovate-config` |
| Repository settings | `scripts/setup-repo.sh`, `scripts/apply-ruleset.sh` | `--audit` is read-only, `DRY_RUN=1` prints the calls; the owner applies |
| Oracle | `scripts/oracle/`, `docs/ORACLE.md`, `task oracle` | Runs `loot-rs sort` and libloot on the same synthetic cases or on the owner's real load order, locally; CI runs only the self-tests and never needs libloot or the game. Pending real runs |
| Local setup | `mise.toml`, `Taskfile.yml`, `scripts/install-git-hooks.sh` | `mise install`, then `task check` |

## Why release-please uses `rust`

The package has `publish = false` and nothing is published, so the only artefact of a release is the tag and the
GitHub Release. `release-type: rust` bumps `version` in `Cargo.toml` and the matching entry in `Cargo.lock` with the
tag, so `loot-rs --version` and the tag always agree; `simple` would leave them at one version forever. The cost is that
the release pull request touches `Cargo.lock`, which is harmless here (there is no checksum over the lockfile, as
there is in `nv-rs`). The manifest starts at 0.0.0 and `release-as` pins the first release to 0.1.0; remove `release-as` from `release-please-config.json` once 0.1.0 is tagged, or every later release stays 0.1.0. `bump-minor-pre-major` keeps breaking changes at 0.x.

## Dependencies and licences

The project is GPL-3.0-only. `cargo deny` allows only what the dependency tree needs today: MIT and Apache-2.0
(`regex`, `memchr`, `aho-corasick`, ...), Zlib (`foldhash`) and BSD-3-Clause (`encoding_rs`, which is also MIT or
Apache-2.0). All are compatible with GPL-3.0. A new licence is added only after reading it.
