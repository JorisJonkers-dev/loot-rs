#!/usr/bin/env python3
"""Differential test: loot-rs against the real libloot on random synthetic load orders.

Each world is a handful of invented plugins (masters, records, BSA archives), some in an additional data
directory (`Extra`, searched before `Data`), groups with `after` lists and
`after` and `req` rules; both tools sort the same directory, given the same current order, and the sorted orders
must be equal. An error is a result too: both refusing counts as agreement. Nothing here uses game files.

    # the libloot side: build it once (needs network for the crates), then
    cargo build --release --manifest-path scripts/oracle/libloot-driver/Cargo.toml
    scripts/oracle/random_diff.py --loot-rs target/release/loot-rs \\
        --driver scripts/oracle/libloot-driver/target/release/libloot-driver --count 500 --seed 1

    scripts/oracle/random_diff.py --selftest

Exit status: 0 no difference, 1 differences (the first few are printed with their world), 2 error, 3 skipped (no
driver given or built: nothing was compared).
"""
import json
import os
import random
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import materialise as m  # noqa: E402


def world(rng: random.Random, most: int = 12) -> dict:
    """Plugins P0.. (the first few masters), what each lists as masters, its records and archive files, a group
    graph and masterlist entries. Masters and rules only point at earlier plugins, so cycles are rare (and agreed
    on when they happen)."""
    count = rng.randint(2, most)
    masters_count = rng.randint(0, 3)
    names = [f"P{i}{'.esm' if i < masters_count else '.esp'}" for i in range(count)]
    plugins = []
    for i, name in enumerate(names):
        listed = ["FalloutNV.esm"] + [
            other
            for other in names[:i]
            if (other.endswith(".esm") or not name.endswith(".esm")) and rng.random() < 0.15
        ]
        records = []
        if rng.random() < 0.8:
            for _ in range(rng.randint(0, 6)):
                if rng.random() < 0.7:
                    records.append((rng.randrange(len(listed)) << 24) | rng.randint(1, 8))
                else:
                    records.append((len(listed) << 24) | (0x800 + rng.randint(0, 6)))
        archive = []
        if rng.random() < 0.4:
            folder = rng.choice(["meshes", "textures"])
            archive = [(folder, [f"f{rng.randint(0, 5)}.nif" for _ in range(rng.randint(1, 5))])]
        plugins.append({"name": name, "esm": name.endswith(".esm"), "masters": listed, "records": records, "archive": archive,
                        "extra": rng.random() < 0.3})
    group_after = {"g1": [], "g2": ["g1"], "g3": ["g1"] if rng.random() < 0.5 else ["g2"]}
    if rng.random() < 0.5:
        group_after["default"] = [rng.choice(["g1", "g2"])]
    entries = []
    for i, plugin in enumerate(plugins):
        entry: dict = {"name": plugin["name"]}
        if rng.random() < 0.5:
            entry["group"] = rng.choice(["g1", "g2", "g3"])
        for key in ("after", "req"):
            earlier = [other for other in names[:i] if other.endswith(".esm") or not plugin["esm"]]
            if earlier and rng.random() < 0.15:
                entry[key] = [rng.choice(earlier)]
        entries.append(entry)
    order = names[:]
    rng.shuffle(order)
    return {"plugins": plugins, "group_after": group_after, "entries": entries, "order": order}


def write(w: dict, root: Path) -> list[str]:
    """The game directory of a world: `Data`, `Extra` (an additional data directory that holds the plugins the
    world marks with `extra`, with their archives), `masterlist.yaml`, `plugins.txt`, `local`. Returns the load order."""
    data = root / "Data"
    data.mkdir(parents=True)
    extra = root / "Extra"
    extra.mkdir()
    (data / "FalloutNV.esm").write_bytes(m.synthetic_plugin(m.FLAG_ESM, []))
    for plugin in w["plugins"]:
        flags = m.FLAG_ESM if plugin["esm"] else 0
        if plugin["records"]:
            blob = m.synthetic_records(flags, plugin["masters"], plugin["records"])
        else:
            blob = m.synthetic_plugin(flags, plugin["masters"])
        home = extra if plugin.get("extra") else data
        (home / plugin["name"]).write_bytes(blob)
        if plugin["archive"]:
            (home / (Path(plugin["name"]).stem + ".bsa")).write_bytes(m.synthetic_bsa(plugin["archive"]))
    lines = ["groups:"]
    for group, after in w["group_after"].items():
        lines.append(f"  - name: {group}")
        if after:
            lines.append(f"    after: [{', '.join(after)}]")
    lines.append("plugins:")
    for entry in w["entries"]:
        lines.append(f"  - name: '{entry['name']}'")
        for key in ("group", "after", "req"):
            if key in entry:
                value = entry[key] if key == "group" else f"[{', '.join(entry[key])}]"
                lines.append(f"    {key}: {value}")
    (root / "masterlist.yaml").write_text("\n".join(lines) + "\n")
    order = ["FalloutNV.esm"] + w["order"]
    (root / "plugins.txt").write_text("\n".join(order) + "\n")
    (root / "local").mkdir()
    return order


def sort_ours(binary: str, root: Path) -> list[str] | str:
    done = subprocess.run(
        [binary, "sort", "--masterlist", str(root / "masterlist.yaml"), "--plugins", str(root / "plugins.txt"),
         "--data", str(root / "Data"), "--extra-data", str(root / "Extra"), "--json"],
        capture_output=True, text=True, check=False,
    )
    if done.returncode in (0, 1):
        return json.loads(done.stdout)["sorted"]
    return f"error: {done.stderr.strip()}"


def sort_libloot(driver: str, root: Path, order: list[str]) -> list[str] | str:
    done = subprocess.run(
        [driver, str(root), str(root / "local"), str(root / "masterlist.yaml"), *order],
        capture_output=True, text=True, check=False,
        env={**os.environ, "LIBLOOT_DRIVER_EXTRA_DATA": str(root / "Extra")},
    )
    try:
        result = json.loads(done.stdout)
    except ValueError:
        return f"driver failed: {done.stderr.strip()[-300:]}"
    return f"error: {result['error']}" if isinstance(result, dict) else result


def compare_world(w: dict, binary: str, driver: str) -> tuple[bool, object, object, list[str]]:
    root = Path(tempfile.mkdtemp(prefix="random-diff-"))
    try:
        order = write(w, root / "game")
        ours = sort_ours(binary, root / "game")
        theirs = sort_libloot(driver, root / "game", order)
        both_refuse = isinstance(ours, str) and isinstance(theirs, str) and not theirs.startswith("driver failed")
        return (both_refuse or ours == theirs), ours, theirs, order
    finally:
        shutil.rmtree(root, ignore_errors=True)


def selftest() -> None:
    rng = random.Random(1)
    for _ in range(20):
        w = world(rng)
        assert 2 <= len(w["plugins"]) <= 12 and sorted(w["order"]) == sorted(p["name"] for p in w["plugins"])
        for plugin in w["plugins"]:
            assert plugin["masters"][0] == "FalloutNV.esm"
            assert all(not (plugin["esm"] and not other.endswith(".esm")) for other in plugin["masters"])
        root = Path(tempfile.mkdtemp(prefix="random-diff-selftest-"))
        try:
            order = write(w, root / "game")
            assert order[0] == "FalloutNV.esm" and len(order) == len(w["plugins"]) + 1
            for name in order:
                assert (root / "game" / "Data" / name).is_file() or (root / "game" / "Extra" / name).is_file()
            for plugin in w["plugins"]:
                home = "Extra" if plugin["extra"] else "Data"
                assert (root / "game" / home / plugin["name"]).is_file(), "a plugin is in its own directory"
                assert not (root / "game" / ("Data" if plugin["extra"] else "Extra") / plugin["name"]).exists()
            assert (root / "game" / "masterlist.yaml").read_text().startswith("groups:")
        finally:
            shutil.rmtree(root, ignore_errors=True)
    assert world(random.Random(5)) == world(random.Random(5)), "a seed gives one world"
    print("random_diff.py: selftest ok")


def main(argv: list[str]) -> int:
    if argv == ["--selftest"]:
        selftest()
        return 0

    def option(name: str, default: str | None = None) -> str | None:
        return argv[argv.index(name) + 1] if name in argv else default

    binary, driver = option("--loot-rs"), option("--driver")
    if binary is None:
        print(__doc__, file=sys.stderr)
        return 2
    if driver is None or not Path(driver).is_file():
        print("random_diff.py: no libloot driver, nothing compared (see --help text above)", file=sys.stderr)
        return 3
    count, seed, most = int(option("--count", "200")), int(option("--seed", "1")), int(option("--max-plugins", "12"))
    rng = random.Random(seed)
    differences = errors = 0
    for number in range(count):
        w = world(rng, most)
        same, ours, theirs, order = compare_world(w, binary, driver)
        if isinstance(ours, str) and isinstance(theirs, str):
            errors += 1
        if not same:
            differences += 1
            if differences <= 5:
                print(f"world {number} (seed {seed}): current order {order}\n  loot-rs {ours}\n  libloot {theirs}")
                print(f"  world: {json.dumps(w)}")
    print(f"{count} worlds (seed {seed}), {differences} difference(s), {errors} where both refused")
    return 1 if differences else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
