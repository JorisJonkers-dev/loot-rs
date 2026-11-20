# Text captured from the LOOT application

`generated_messages` and `generated_general_messages` reproduce what the LOOT application (not libloot) adds. Their
expected text in `tests/golden_app/<case>/loot-app.txt` is what LOOT 0.29.2 (libloot 0.29.6) itself showed when it was
run on the case, so it does not come from documentation or from reading the source.

## What was run

* The official release `loot_0.29.2-win64.7z` from https://github.com/loot/loot/releases/tag/0.29.2, sha256
  `10685b03a43a77a7644b5c417eb757f6b405425f3c4d9abeaab4faaa967fb6e0` (the release has no Linux build).
* Wine 11 (Staging) with a throw-away prefix, on an Xvfb virtual display. Nothing is shown on the desktop.
* A synthetic Fallout: New Vegas install: an empty `FalloutNV.exe`, and a `Data` directory written from the case's
  `data.txt` by `cargo run --example synth_data` (invented plugins, header only). The masterlist is the case's
  `masterlist.yaml`; LOOT's own masterlist is never downloaded (`updateMasterlist = false`). `plugins.txt` is the case's.
* LOOT's settings (`settings.toml`): game path and local path set, `minimumHeaderVersion = 1.32` (LOOT's default for
  this game), debug logging on, update checks off, "warn on case-sensitive paths" on.
* The text is LOOT's own menu command **Game > Copy Content**, sent with `xdotool` (`alt+g c`) and read from the
  clipboard with `xclip`. It is the Markdown of the General Information card and of every plugin card. A case with a
  `sort` file also runs **Game > Sort Plugins** first (used by `sort_cycle`, whose sort is refused).

There is no flag that prints the messages: `--auto-sort` only sorts and quits, and the debug log (kept next to
the capture, in `runs/<case>/debug.log`) has the message text but without the Markdown escapes the card shows.

## Reproduce

```sh
# Wine, Xvfb, xdotool, xclip, bsdtar, gh, cargo are needed; on NixOS:
nix shell nixpkgs#xorg.xvfb nixpkgs#xdotool nixpkgs#xclip nixpkgs#imagemagick -c \
  scripts/loot-app-capture/capture.sh tests/golden_app/all_messages tests/golden_app/self_master \
  tests/golden_app/active_limit tests/golden_app/sort_cycle
```

The script downloads the release, checks the hash, and keeps everything under
`~/Games/Fallout New Vegas RS/LOOT capture/<version>/` (override with `LOOT_APP_SCRATCH`), outside every repository. Only the
resulting `loot-app.txt` (first line: the LOOT version, libloot version and archive hash) is committed. CI does not run
it; `tests/golden_app.rs` compares the committed text with `loot-rs sort --json`.

## Cases

| Case | Messages it shows |
| --- | --- |
| `all_messages` | missing master, inactive master, header version below 1.32, group that is not defined, `Data/BashTags` overriding the masterlist's tags, `Filter` tag silencing the master check, a `dirty` entry that matches (all three counts), a missing requirement with a display name and with a detail, a present incompatibility, a masterlist `globals` note |
| `self_master` | a plugin that is its own master (sorting is refused, see below) |
| `active_limit` | 256 active plugins |
| `sort_cycle` | the text LOOT shows when the sort finds a cycle |

## Licence

LOOT is GPL-3.0; loot-rs is GPL-3.0-only (`LICENSE`), so the message strings (and the captured card text of synthetic
plugins) may be carried here. The captured files contain only LOOT's English message templates filled with invented
plugin names. No LOOT binary, no masterlist, no game data is in this repository.

## Differences and decisions

See the table of generated messages in [COMPATIBILITY.md](COMPATIBILITY.md).
