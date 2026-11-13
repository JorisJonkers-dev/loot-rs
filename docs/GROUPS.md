# Group violations: soft by default, hard with `--strict-groups`

LOOT sorts by groups. A plugin's group is a hint about where it belongs: group `Late` with `after: [Early]` means
plugins in `Late` should load after plugins in `Early`. `loot-rs check` does not sort; it reports where the load
order you give it disagrees with those hints.

## The two modes

| | Default (soft) | `--strict-groups` (hard) |
| --- | --- | --- |
| Group order violation | `note group order: ...` | `VIOLATION group order: ...` |
| Plugin in a group the masterlist does not define | `note undefined group: ...` | `VIOLATION undefined group: ...` |
| Counted in `Hard violations` and in the exit code | no | yes |
| Exit code with only group findings | `0` | `1` |
| JSON (`--json`) | `"hard":false` | `"hard":true` |
| Library | `check::Options { strict_groups: false }` (the default) | `check::Options { strict_groups: true }` |

The text of a finding is the same in both modes; only its severity changes. Requirements (`req`), incompatibilities
(`inc`) and load-after (`after`) violations are always hard.

## Why soft is the default

In LOOT a group edge is a preference that yields. LOOT drops a group edge when it would contradict a master
relationship, a `after` or a `req` edge, or when it would create a cycle. `loot-rs` does not implement that
dropping (see [COMPATIBILITY.md](COMPATIBILITY.md)), so it can report a group order that LOOT itself would not
enforce. Failing a build or a mod-manager profile on that would be a false alarm, so by default a group finding is a
note and the exit code stays `0`.

## When to use `--strict-groups`

When the question is "does this load order follow the masterlist's groups exactly", for example to gate a profile
in a script, and you accept that an edge LOOT would have dropped is reported as a violation. Read the finding: the
two plugins and their groups are named, so a clash with a master or `after` edge is easy to see.

```sh
loot-rs check --masterlist masterlist.yaml --plugins plugins.txt --data Data --strict-groups; echo $?
```

## Tests

Golden cases `group_soft` (note, exit 0), `group_strict` (the same input with `--strict-groups`: violation,
exit 1), `group_undefined`, `group_ok`, `group_cycle`; unit tests `strict_groups_make_group_violations_hard` and
`an_undefined_group_is_noted_and_hard_only_when_strict` in `src/check_tests.rs`.

## Not decided

Whether `--strict-groups` should one day apply LOOT's edge dropping first. That needs the master flags of the
plugins and the `after` and `req` edges in one graph, which is the sorter's job (see the library API work).
