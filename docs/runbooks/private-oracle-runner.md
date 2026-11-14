# Runbook: the private runner for the real-data oracle

Owner only. Nothing here can be done by a contributor, a pull request or a bot, and none of it is automated by the
repositories. Background: ADR-0002 (private data never runs beside untrusted code), `docs/ORACLE.md` (the script) and
section 9 and 11 of the integration design in the umbrella repository. Until every step below is done, the
`digest/real-data` status of a commit is missing and the release gate (#33) stays red; that is the safe state.

## What runs where

| Piece | Where | Holds data | Reachable from a pull request |
| --- | --- | --- | --- |
| `oracle-trigger.yml` | this repository, GitHub-hosted runner | no | no (push to `main` and owner dispatch only) |
| `real-data.yml` | private repository `JorisJonkers-dev/oracle-ci`, self-hosted runner | the owner's machine | no (owner dispatch only) |
| `oracle.toml`, the datasets, L2 output | the runner's disk, outside every repository | yes | no |

The runner is registered to the private repository, not to the organisation and not to this repository, so no public
workflow can name its label. Do not register it anywhere else.

## Steps

1. **Create the private repository** `JorisJonkers-dev/oracle-ci`. Only the owner is a member; no outside collaborators,
   no forks (Settings, Actions, "Fork pull request workflows": off; it has no pull request workflows anyway).
2. **Add the workflow.** Copy `scripts/oracle/oracle-ci/real-data.yml` from this repository to
   `.github/workflows/real-data.yml` of `oracle-ci` on its default branch. Review it as you would a change here; it is
   the only code that sees the data. Add a `CODEOWNERS` line for yourself.
3. **Register the runner** to `oracle-ci` only, with the label `oracle`, as an ephemeral runner (it accepts one job,
   then re-registers). Run it as a dedicated systemd unit under a user that can read the datasets and nothing else.
   Install on that host: a Rust toolchain, `python3` 3.11 or newer (for `tomllib`), `git`, `gh`, `jq`, `curl`. The
   unit's environment file (mode 0600, outside every repository) sets `ORACLE_CONFIG=/path/to/oracle.toml`.
4. **Cap the runner.** Put the unit in its own slice, for example `MemoryMax=12G`, `MemoryHigh=10G`, `CPUQuota=600%`,
   `CPUWeight=20`, `IOWeight=20`, `Nice=19`, `RuntimeMaxSec=3600`, and never on the node the
   estate keeps free of load. libloot loads whole plugins to sort them, so the memory cap is the one that matters. Set
   `CARGO_BUILD_JOBS` to a small number in the environment file.
5. **Write the oracle config** at `ORACLE_CONFIG`, mode 0600, outside any git work tree (the script refuses
   otherwise). The format is the docstring of `scripts/oracle/oracle_run.py`. Point `[tools] loot_rs` and
   `libloot_driver` at the binaries the job builds (`tool/target/release/loot-rs` and
   `tool/scripts/oracle/libloot-driver/target/release/libloot-driver` under the runner's work directory), `masterlist`
   at a masterlist you keep there, and each `[dataset.<name>]` at a pinned copy of the load order and files. Use a
   copy or a read-only mount of the data, not the live MO2 instance: the script never writes to it, but a runner has
   no reason to see it. Add `[oracle] perturbations = 20` and a `seed`, and variants (`variant_of`, `disable`) for the
   subsets worth testing; the workflow's optional `perturbations` and `seed` inputs override them for one dispatch. For the plugin comparison (`oracle_run.py --esplugin`) add `esplugin_diff` under `[tools]`, pointing at
   `tool/scripts/oracle/esplugin-diff/target/release/esplugin-diff` after building it with `cargo build --release
   --manifest-path scripts/oracle/esplugin-diff/Cargo.toml`. Keep `[output] root` on the runner's disk; the L2 detail stays there and is never uploaded.
6. **Create two fine-grained tokens** and store them as secrets:
   * In `oracle-ci`: `ORACLE_STATUS_TOKEN`, repository access limited to the public repositories under test (today
     `JorisJonkers-dev/loot-rs`), permissions Commit statuses: write, Issues: write, Contents: read, Metadata: read.
   * In each public repository under test: `ORACLE_DISPATCH_TOKEN`, repository access limited to `oracle-ci`,
     permission Actions: write, Metadata: read.
   Set an expiry on both and put the renewal date in your calendar. A dispatch token that expires makes the trigger
   do nothing (the status then stays missing and the gate red), not fail silently green.
7. **Make the status a requirement.** In the release pull request rules of each repository, the `release-gate` check
   (umbrella issue #33) reads the `digest/real-data` status of the release commit. Do not add the status as a required
   check on ordinary pull requests: it only exists after a merge.
8. **Try it** with a dispatch from the Actions tab of this repository ("Oracle trigger", Run workflow). Expected: the
   commit shows `digest/real-data` pending, then `pass`, `fail` with counts or `error`. With a config that has no
   libloot driver the status is `skipped`, which is red by design.

## Checks before the first real run

* The run log of `oracle-ci` shows build output and the repository, commit, state and counts, and nothing else. Open
  it once and read it as a stranger would.
* A dataset with a missing plugin gives `partial` (red), a deliberately changed order gives `fail`, and the issue
  "Real-data oracle is red" appears in this repository with counts and a digest only.
* `ORACLE_TOOL_SHA` equals the commit in the status description. The runner checks that the commit is on the default
  branch before it does anything, so a dispatch with another commit is refused.

## What is deliberately not there

No step summary, annotation, artifact or cache in the private workflow; no `pull_request`, `pull_request_target`,
`issue_comment` or label trigger in either workflow; no automatic revert; no private text in an issue or status
(`oracle_ci_report.py` copies nothing but counts and hex digests, and its selftest checks that free text is refused).
If one of these ever has to change, change the allow-list and its selftest first.

## nvmod-rs

nvmod-rs has no real-data oracle to run yet: the checks of integration design section 9 (the imported Instance has
the MO2 profile's enabled Mod list and Load order, the Load order equals real LOOT's, the Staging tree has the
expected path set) have no script on `main`. When one exists it takes the same shape: an `oracle_run`-style script
with a config outside the repository that prints an L0 line, a second dispatch option in `real-data.yml`, and a
trigger workflow like `oracle-trigger.yml`. It must run `nvmod import-mo2` on a copy of the MO2 data, never on the
live instance.
