#!/usr/bin/env bash
# Run fuzz targets for a fixed time each, seeded from fuzz/seeds/<target> and, for the masterlist target, from the
# synthetic masterlists of the golden tests. Needs a nightly toolchain and cargo-fuzz.
#
#   scripts/fuzz-run.sh                      every target, FUZZ_SECONDS (default 15) each
#   scripts/fuzz-run.sh condition 600        one target, 600 seconds
set -euo pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
seconds="${2:-${FUZZ_SECONDS:-15}}"
cd "$root/fuzz"
if [ $# -ge 1 ]; then targets=("$1"); else mapfile -t targets < <(cargo +nightly fuzz list); fi
scratch="$(mktemp -d)"
trap 'rm -rf "$scratch"' EXIT
for target in "${targets[@]}"; do
  dirs=("corpus/$target")
  mkdir -p "corpus/$target"
  [ -d "seeds/$target" ] && dirs+=("seeds/$target")
  if [ "$target" = masterlist_yaml ]; then
    mkdir -p "$scratch/$target"
    for file in ../tests/golden/*/masterlist.yaml; do
      cp "$file" "$scratch/$target/$(basename "$(dirname "$file")")"
    done
    dirs+=("$scratch/$target")
  fi
  echo "== $target ($seconds s)"
  cargo +nightly fuzz run "$target" "${dirs[@]}" -- -max_total_time="$seconds" -rss_limit_mb=2048 -timeout=10
done
