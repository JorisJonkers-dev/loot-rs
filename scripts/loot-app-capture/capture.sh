#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-only
# Capture the text the LOOT application shows for a synthetic golden case: build the case's `Data` directory from
# `data.txt` (invented plugins only), run LOOT under Wine on a virtual display with the case's `masterlist.yaml`
# and `plugins.txt`, then use LOOT's own "Game > Copy Content" and write the clipboard to `loot-app.txt`.
#
# Nothing downloaded is kept in the repository. Everything lives in a scratch folder (default
# ~/Games/Fallout New Vegas RS/LOOT capture/<version>), see docs/LOOT-APP-CAPTURE.md.
#
#   scripts/loot-app-capture/capture.sh tests/golden_app/all_messages [more case directories]
#
# Needs: wine, Xvfb, xdotool, xclip, python3, cargo. On NixOS the display tools come from
#   nix shell nixpkgs#xorg.xvfb nixpkgs#xdotool nixpkgs#xclip nixpkgs#imagemagick -c scripts/loot-app-capture/capture.sh ...
set -euo pipefail

version=${LOOT_APP_VERSION:-0.29.2}
sha256=${LOOT_APP_SHA256:-10685b03a43a77a7644b5c417eb757f6b405425f3c4d9abeaab4faaa967fb6e0}
scratch=${LOOT_APP_SCRATCH:-$HOME/Games/Fallout New Vegas RS/LOOT capture/$version}
display=${LOOT_APP_DISPLAY:-:77}
repo=$(cd "$(dirname "$0")/../.." && pwd)
archive=loot_${version}-win64.7z

[ "$#" -ge 1 ] || { echo "usage: $0 <case directory>..." >&2; exit 2; }

# 1. The application: the official release, outside every repository, checked against the recorded hash.
if [ ! -f "$scratch/dl/$archive" ]; then
  mkdir -p "$scratch/dl"
  gh release download "$version" -R loot/loot -p "$archive" -D "$scratch/dl"
fi
echo "$sha256  $scratch/dl/$archive" | sha256sum -c -
if [ ! -d "$scratch/app" ]; then
  mkdir -p "$scratch/app"
  bsdtar -xf "$scratch/dl/$archive" -C "$scratch/app"
fi
app=$(echo "$scratch"/app/loot_*)

# 2. The synthetic Data directory writer (an example of this crate).
cargo build --quiet --manifest-path "$repo/Cargo.toml" --example synth_data
synth=${CARGO_TARGET_DIR:-$repo/target}/debug/examples/synth_data

export WINEPREFIX=$scratch/prefix WINEDEBUG=-all DISPLAY=$display WINEARCH=win64
win() { printf 'Z:%s' "$(printf '%s' "$1" | tr / '\\')"; }

if ! pgrep -u "$USER" -f "Xvfb $display " >/dev/null; then
  Xvfb "$display" -screen 0 1400x900x24 >/dev/null 2>&1 &
  sleep 2
fi
[ -d "$WINEPREFIX" ] || wineboot -i >/dev/null 2>&1

for case_dir in "$@"; do
  case_dir=$(cd "$case_dir" && pwd)
  name=$(basename "$case_dir")
  work=$scratch/runs/$name
  rm -rf "$work"
  mkdir -p "$work/lootdata/games/FalloutNV" "$work/game"
  : >"$work/game/FalloutNV.exe"          # an empty placeholder: LOOT only needs the file to exist
  "$synth" "$case_dir/data.txt" "$work/game/Data"
  # Load order of the game is the modification time of the files: masters first, then in plugins.txt order.
  i=0
  while read -r plugin; do
    [ -n "$plugin" ] || continue
    touch -d "2020-01-01 00:00:00 +0000 + $i seconds" "$work/game/Data/$plugin" 2>/dev/null || true
    i=$((i + 1))
  done < <(grep -E '^(plugin) ' "$case_dir/data.txt" | awk '{print $2}')
  local_dir=$work/localappdata
  mkdir -p "$local_dir"
  cp "$case_dir/plugins.txt" "$local_dir/plugins.txt"
  cp "$case_dir/masterlist.yaml" "$work/lootdata/games/FalloutNV/masterlist.yaml"
  cat >"$work/lootdata/settings.toml" <<TOML
enableDebugLogging = true
updateMasterlist = false
enableLootUpdateCheck = false
warnOnCaseSensitiveGamePaths = true
game = "FalloutNV"
lastGame = "FalloutNV"
language = "en"
lastVersion = "$version"

[[games]]
type = "FalloutNV"
name = "Fallout: New Vegas"
folder = "FalloutNV"
master = "FalloutNV.esm"
minimumHeaderVersion = 1.32
masterlistSource = ""
path = '$(win "$work/game")'
local_path = '$(win "$local_dir")'
TOML

  wineserver -k 2>/dev/null || true
  sleep 1
  (cd "$app" && wine LOOT.exe --loot-data-path="$(win "$work/lootdata")" >"$work/stdout.txt" 2>&1 &)
  # The debug log ends its start-up with the case-sensitivity check of the game's paths.
  for _ in $(seq 1 120); do
    grep -q "Failed to read metadata for\|case-sensitiv" "$work/lootdata/LOOTDebugLog.txt" 2>/dev/null && break
    sleep 1
  done
  sleep 5
  if [ -e "$case_dir/sort" ]; then
    # A case with a `sort` file also presses Game > Sort Plugins; a refused sort shows a dialog (Return) and puts the
    # reason in the General Information card.
    xdotool key alt+g s
    sleep 15
    xdotool key Return
    sleep 2
  fi
  xclip -selection clipboard -i </dev/null
  xdotool key alt+g c        # Game > Copy Content
  sleep 3
  xclip -selection clipboard -o >"$work/content.md"
  [ -s "$work/content.md" ] || { echo "$name: nothing was copied" >&2; exit 1; }
  { echo "LOOT $version (libloot $(grep -o 'libloot version: [0-9.]*' "$work/lootdata/LOOTDebugLog.txt" | head -1 | cut -d' ' -f3)), $archive sha256 $sha256"; echo "---"; cat "$work/content.md"; } >"$case_dir/loot-app.txt"
  cp "$work/lootdata/LOOTDebugLog.txt" "$work/debug.log"
  wineserver -k 2>/dev/null || true
  echo "$name: wrote $case_dir/loot-app.txt"
done
