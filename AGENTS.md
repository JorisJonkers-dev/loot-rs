# Agents

Guidance for any coding agent working in this repository.

- Agent contract: [CLAUDE.md](CLAUDE.md)
- Estate conventions: https://github.com/JorisJonkers-dev/workspace/blob/main/CLAUDE.md

This repository is `loot-fnv`: a read-only evaluator of LOOT masterlist rules over a Fallout: New Vegas load
order (see [README.md](README.md)). The `templates/` tree and `scripts/` come from the repository template.

## Build and test

- Everything CI runs: `task check` (or `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`)
- Install the push-protection git hooks: `scripts/install-git-hooks.sh`
