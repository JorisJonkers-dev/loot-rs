#!/usr/bin/env python3
"""Differential test: loot-rs against the real libloot on random synthetic load orders.

Each world is a handful of invented plugins (masters, records, BSA archives), some in an additional data
directory (`Extra`, searched before `Data`), groups with `after` lists and
`after` and `req` rules; both tools sort the same directory, given the same current order, and the sorted orders
must be equal. An error is a result too: both refusing counts as agreement. Nothing here uses game files.

The minimal-change mode (`loot-rs sort --mode minimal`) is checked on the same worlds against libloot as the oracle for
constraint satisfaction: the order must keep every hard constraint (master flag, header masters, `req`, `after`,
`FalloutNV.esm` first) that libloot's order keeps, must move no more plugins than libloot's order displaces and report a
reason for each move, and an order that is already valid (libloot's) must come back unchanged with no moves.

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
import re
import shutil
import subprocess
import sys
import tempfile
import zlib
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import compare  # noqa: E402
import materialise as m  # noqa: E402

TAGS = ["Relev", "Delev", "Actors.ACBS", "Graphics", "NoMerge", "Stats"]
WORDS = ["patch", "requires", "conflict", "clean", "see", "the", "readme", "mod", "update", "load"]
CRC_TOKEN = re.compile(r"\{crc:([^}\s]+)\}")


def condition(rng: random.Random, names: list[str], depth: int = 0) -> str:
    """A random condition over the invented plugins. `{crc:NAME}` becomes the CRC-32 of that plugin's bytes when the
    world is written, and `{crc:?}` a CRC that matches nothing."""
    def atom() -> str:
        name = rng.choice(names + ["Missing.esp"])
        kind = rng.choice(["file", "active", "is_master", "checksum", "many", "many_active"])
        if kind == "checksum":
            return f'checksum("{name}", {{crc:{rng.choice([name, "?"])}}})'
        if kind in ("many", "many_active"):
            return f'{kind}("P[0-3]\\.es[pm]")'
        return f'{kind}("{name}")'
    if depth > 1 or rng.random() < 0.4:
        return atom()
    shape = rng.choice(["not", "and", "or", "group", "negated"])
    one, two = condition(rng, names, depth + 1), condition(rng, names, depth + 1)
    # libloot's grammar has no `not not x` (loot-rs accepts it; see docs/ORACLE.md), so a negation takes parentheses
    return {"not": f"not ({one})", "and": f"({one}) and ({two})", "or": f"({one}) or ({two})",
            "group": f"({one} or {two}) and {atom()}", "negated": f"not ({one} and {two})"}[shape]


def message(rng: random.Random, names: list[str]) -> dict:
    words = " ".join(rng.choice(WORDS) for _ in range(rng.randint(1, 5)))
    entry: dict = {"type": rng.choice(["say", "warn", "error"])}
    roll = rng.random()
    if roll < 0.25:
        entry["content"] = words + " {0} and {1}"
        entry["subs"] = [rng.choice(names), "[the page](http://example.invalid/x)"]
    elif roll < 0.35:
        entry["content"] = [{"lang": "de", "text": "deutsch " + words}, {"lang": "en", "text": words}]
    elif roll < 0.5:
        entry["content"] = f"**{words}** with [a link](http://example.invalid/y)"
    else:
        entry["content"] = words
    if rng.random() < 0.6:
        entry["condition"] = condition(rng, names)
    return entry


def metadata(rng: random.Random, names: list[str], own_crc: bool) -> dict:
    """Messages, tags and cleaning data for one masterlist entry. `own_crc` entries name one plugin, so their
    cleaning data can carry that plugin's CRC (`{crc:self}`); a regex entry's never match."""
    meta: dict = {}
    if rng.random() < 0.5:
        meta["msg"] = [message(rng, names) for _ in range(rng.randint(1, 3))]
    if rng.random() < 0.5:
        tags = []
        for _ in range(rng.randint(1, 3)):
            tag = {"name": rng.choice(["", "-"]) + rng.choice(TAGS)}
            if rng.random() < 0.5:
                tag["condition"] = condition(rng, names)
            tags.append(tag)
        meta["tag"] = tags
    for key in ("dirty", "clean"):
        if rng.random() < 0.3:
            matching = own_crc and rng.random() < 0.7
            item = {"crc": "0x{crc:self}" if matching else rng.choice(["0x1", "0xDEADBEEF", "0x00ABCDEF"]), "util": "FNVEdit"}
            if key == "dirty":
                item.update({"itm": rng.randint(0, 9), "udr": rng.randint(0, 9), "nav": rng.randint(0, 3)})
            meta.setdefault(key, []).append(item)
    return meta


def add_metadata(rng: random.Random, w: dict) -> None:
    """Conditions, messages with `subs`, tags and cleaning data on the masterlist entries, general messages, and
    entries whose names are regular expressions (they apply to every plugin they match)."""
    names = [p["name"] for p in w["plugins"]]
    w["globals"] = [message(rng, names) for _ in range(rng.randint(0, 3))]
    for entry in w["entries"]:
        entry.update(metadata(rng, names, own_crc=True))
    patterns = [r"P[0-4]\.esp", r"P\d\.es[pm]", r"P1?\d\.esp"]
    for _ in range(rng.randint(0, 2)):
        regex = {"name": rng.choice(patterns)}
        regex.update(metadata(rng, names, own_crc=False))
        if rng.random() < 0.3:
            regex["group"] = rng.choice(["g1", "g2", "g3"])
        w["entries"].append(regex)


# What a plugin's description field says: formats `extract_version` reads (and one that has no version).
VERSION_DESCRIPTIONS = ["Version: 1.2", "v2.0", "3.1.4 beta", "Updated: 10/09/2016 13:15:18", "Adds things.", "1.10.0-rc.1"]
VERSION_OPERATORS = ["==", "!=", "<", ">", "<=", ">="]
VERSION_VALUES = ["1.2", "1.10", "2", "2.0.0", "3.1.4", "0.9"]


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
                        "extra": rng.random() < 0.3,
                        "description": rng.choice(VERSION_DESCRIPTIONS)})
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
        # An `after` rule that only holds while another plugin's version (from its description) satisfies a test.
        if i > 0 and rng.random() < 0.25:
            guard, target = rng.choice(names), rng.choice(names[:i])
            condition = f'version("{guard}", "{rng.choice(VERSION_VALUES)}", {rng.choice(VERSION_OPERATORS)})'
            entry["version_after"] = [{"name": target, "condition": condition}]
        entries.append(entry)
    order = names[:]
    rng.shuffle(order)
    result = {"plugins": plugins, "group_after": group_after, "entries": entries, "order": order}
    add_metadata(random.Random(rng.random()), result)
    return result


def message_yaml(item: dict, own: str | None, scalar) -> str:
    """One message as a block mapping (the caller indents the continuation lines)."""
    out = [f"type: {item['type']}"]
    content = item["content"]
    if isinstance(content, list):
        out.append("content:")
        out.extend(f"  - lang: {c['lang']}\n    text: {scalar(c['text'], own)}" for c in content)
    else:
        out.append(f"content: {scalar(content, own)}")
    if "subs" in item:
        out.append(f"subs: [{', '.join(scalar(sub, own) for sub in item['subs'])}]")
    if "condition" in item:
        out.append(f"condition: {scalar(item['condition'], own)}")
    return "\n".join(out)


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
            blob = m.synthetic_records(flags, plugin["masters"], plugin["records"], plugin["description"])
        else:
            blob = m.synthetic_plugin(flags, plugin["masters"], plugin["description"])
        home = extra if plugin.get("extra") else data
        (home / plugin["name"]).write_bytes(blob)
        if plugin["archive"]:
            (home / (Path(plugin["name"]).stem + ".bsa")).write_bytes(m.synthetic_bsa(plugin["archive"]))
    crcs = {path.name: zlib.crc32(path.read_bytes()) for home in (data, extra) for path in home.iterdir() if path.is_file()}

    def fill(text: str, own: str | None) -> str:
        def one(found: re.Match) -> str:
            target = own if found.group(1) == "self" else found.group(1)
            return f"{crcs[target]:08X}" if target in crcs else "00000001"  # no such file, or "?": matches nothing
        return CRC_TOKEN.sub(one, text)

    def scalar(value: object, own: str | None) -> str:
        return json.dumps(fill(value, own) if isinstance(value, str) else value)

    def flow(mapping: dict, own: str | None) -> str:
        return "{" + ", ".join(f"{k}: {v}" for k, v in mapping.items()) + "}"

    lines = []
    if w.get("globals"):
        lines.append("globals:")
        lines.extend("  - " + message_yaml(item, None, scalar).replace("\n", "\n    ") for item in w["globals"])
    lines.append("groups:")
    for group, after in w["group_after"].items():
        lines.append(f"  - name: {group}")
        if after:
            lines.append(f"    after: [{', '.join(after)}]")
    lines.append("plugins:")
    for entry in w["entries"]:
        own = entry["name"] if entry["name"] in crcs else None
        lines.append(f"  - name: {json.dumps(entry['name'])}")
        for key in ("group", "after", "req"):
            if key in entry or (key == "after" and "version_after" in entry):
                if key == "group":
                    lines.append(f"    group: {entry['group']}")
                    continue
                items = list(entry.get(key, []))
                if key == "after":
                    items += [f"{{name: {c['name']}, condition: '{c['condition']}'}}" for c in entry.get("version_after", [])]
                lines.append(f"    {key}: [{', '.join(items)}]")
        if entry.get("msg"):
            lines.append("    msg:")
            lines.extend("      - " + message_yaml(item, own, scalar).replace("\n", "\n        ") for item in entry["msg"])
        if entry.get("tag"):
            lines.append("    tag:")
            for tag in entry["tag"]:
                lines.append("      - " + flow({k: scalar(v, own) for k, v in tag.items()}, own))
        for key in ("dirty", "clean"):
            if entry.get(key):
                lines.append(f"    {key}:")
                for item in entry[key]:
                    lines.append("      - " + flow({k: (fill(v, own) if k == "crc" else json.dumps(v) if k == "util" else v)
                                                      for k, v in item.items()}, own))
    (root / "masterlist.yaml").write_text("\n".join(lines) + "\n")
    order = ["FalloutNV.esm"] + w["order"]
    (root / "plugins.txt").write_text("\n".join(order) + "\n")
    (root / "local").mkdir()
    return order


def run_ours_json(binary: str, root: Path, extra: list[str] | None = None) -> dict | str:
    """`loot-rs sort --json` of a world: the whole result, or `error: ...`."""
    done = subprocess.run(
        [binary, "sort", "--masterlist", str(root / "masterlist.yaml"), "--plugins", str(root / "plugins.txt"),
         "--data", str(root / "Data"), "--extra-data", str(root / "Extra"), *(extra or []), "--json"],
        capture_output=True, text=True, check=False,
    )
    if done.returncode in (0, 1):
        return json.loads(done.stdout)
    return f"error: {done.stderr.strip()}"


def sort_ours(binary: str, root: Path) -> list[str] | str:
    result = run_ours_json(binary, root)
    return result["sorted"] if isinstance(result, dict) else result


def libloot_metadata(driver: str, root: Path, order: list[str]) -> dict | str:
    """What libloot says about messages, tags, groups and cleaning data (the driver's `--metadata` mode)."""
    done = subprocess.run(
        [driver, "--metadata", str(root), str(root / "local"), str(root / "masterlist.yaml"), *order],
        capture_output=True, text=True, check=False,
        env={**os.environ, "LIBLOOT_DRIVER_EXTRA_DATA": str(root / "Extra")},
    )
    try:
        return json.loads(done.stdout)
    except ValueError:
        return f"driver failed: {done.stderr.strip()[-300:]}"


def expected_groups(w: dict) -> list[dict]:
    """The groups of the world's masterlist plus the `default` group libloot always has, as the driver prints them."""
    groups = {name: sorted(after) for name, after in w["group_after"].items()}
    groups.setdefault("default", [])
    return [{"name": name, "after": after} for name, after in sorted(groups.items())]


COVERED = {"worlds": 0, "messages": 0, "general_messages": 0, "tags": 0, "dirty": 0, "clean": 0}


def compare_metadata(w: dict, ours: dict, theirs: dict) -> list[str]:
    """Differences in messages, tags, groups and cleaning data (compare.py); the merged groups of libloot's database are
    checked against the groups the world declares, since loot-rs does not print its group graph."""
    lines = [line for line in compare.compare(ours, theirs)
             if not line.startswith(("note:", "info:"))]
    COVERED["worlds"] += 1
    COVERED["general_messages"] += len(theirs.get("general_messages", []))
    for plugin in theirs.get("plugins", {}).values():
        COVERED["messages"] += len(plugin["messages"])
        COVERED["tags"] += len(plugin["tags_add"]) + len(plugin["tags_remove"])
    for entry in theirs.get("dirty", []):
        COVERED[entry["status"]] += 1
    seen = sorted(({"name": g["name"], "after": sorted(g["after"])} for g in theirs.get("groups", [])), key=lambda g: g["name"])
    if seen != expected_groups(w):
        lines.append(f"groups: libloot has {seen}, the masterlist declares {expected_groups(w)}")
    return lines


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


def compare_world_metadata(w: dict, binary: str, driver: str) -> list[str]:
    """Messages, tags, groups and cleaning data of one world; empty when both tools agree. Worlds in which `loot-rs
    sort` refuses have no result to compare (their order is compared by `compare_world`)."""
    root = Path(tempfile.mkdtemp(prefix="random-diff-meta-"))
    try:
        order = write(w, root / "game")
        ours = run_ours_json(binary, root / "game")
        if isinstance(ours, str):
            return []
        theirs = libloot_metadata(driver, root / "game", order)
        if isinstance(theirs, str):
            return [f"metadata: {theirs}"]
        return compare_metadata(w, ours, theirs)
    finally:
        shutil.rmtree(root, ignore_errors=True)


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


def hard_constraints(w: dict) -> list[tuple[str, str, str]]:
    """The hard constraints of a world as `(before, after, class)`: header masters, `req`, `after`, the master flag
    (every master before every other plugin) and `FalloutNV.esm` before everything."""
    found: list[tuple[str, str, str]] = []
    names = [plugin["name"] for plugin in w["plugins"]]
    for plugin in w["plugins"]:
        found.extend((master, plugin["name"], "header-master") for master in plugin["masters"] if master in names)
        found.append(("FalloutNV.esm", plugin["name"], "hardcoded"))
    for entry in w["entries"]:
        for key, label in (("after", "after"), ("req", "req")):
            found.extend((other, entry["name"], label) for other in entry.get(key, []) if entry["name"] in names)
    esm = [plugin["name"] for plugin in w["plugins"] if plugin["esm"]]
    found.extend((first, other, "master-flag") for first in esm for other in names if other not in esm)
    return found


def violated(order: list[str], constraints: list[tuple[str, str, str]]) -> set[tuple[str, str, str]]:
    at = {name: index for index, name in enumerate(order)}
    return {c for c in constraints if c[0] in at and c[1] in at and at[c[0]] > at[c[1]]}


def displaced(given: list[str], result: list[str]) -> int:
    """Plugins that are not where a stable, order-keeping move of the others would leave them: given minus the
    longest common subsequence (the fewest moves any tool could make to turn one order into the other)."""
    a, b = given, result
    table = [[0] * (len(b) + 1) for _ in range(len(a) + 1)]
    for i in range(len(a) - 1, -1, -1):
        for j in range(len(b) - 1, -1, -1):
            table[i][j] = table[i + 1][j + 1] + 1 if a[i] == b[j] else max(table[i + 1][j], table[i][j + 1])
    return len(a) - table[0][0]


def check_minimal(w: dict, given: list[str], minimal: dict, libloot: list[str]) -> list[str]:
    """Differences between `loot-rs sort --mode minimal --json` of the world and what libloot's order satisfies."""
    lines = []
    constraints = hard_constraints(w)
    result = minimal["sorted"]
    if sorted(result) != sorted(given):
        return [f"minimal: not a permutation: {result}"]
    if violated(libloot, constraints):
        lines.append(f"minimal: libloot's order violates {sorted(violated(libloot, constraints))}: a bug in the oracle's constraints")
    if violated(result, constraints):
        lines.append(f"minimal: violates {sorted(violated(result, constraints))}")
    moves = minimal.get("moves", [])
    if minimal.get("mode") != "minimal":
        lines.append("minimal: the JSON has no mode")
    if not violated(given, constraints) and (result != given or moves):
        lines.append(f"minimal: the given order is valid but came back as {result} with {len(moves)} move(s)")
    if displaced(given, result) > len(moves):
        lines.append(f"minimal: {displaced(given, result)} plugin(s) displaced but only {len(moves)} move(s) reported")
    if len(moves) > displaced(given, libloot):
        lines.append(f"minimal: {len(moves)} move(s), libloot's order displaces {displaced(given, libloot)}")
    for move in moves:
        if not move.get("reason") or move["plugin"] not in given:
            lines.append(f"minimal: a move without a reason: {move}")
    return lines


def compare_world_minimal(w: dict, binary: str, driver: str) -> list[str]:
    root = Path(tempfile.mkdtemp(prefix="random-diff-min-"))
    try:
        given = write(w, root / "game")
        theirs = sort_libloot(driver, root / "game", given)
        ours = run_ours_json(binary, root / "game", ["--mode", "minimal"])
        if isinstance(theirs, str) or isinstance(ours, str):
            return []  # a refusal is compared by compare_world
        lines = check_minimal(w, given, ours, theirs)
        # libloot's own order is valid: minimal-change returns it unchanged
        again = {**w, "order": [name for name in theirs if name != "FalloutNV.esm"]}
        root2 = root / "second"
        write(again, root2)
        second = run_ours_json(binary, root2, ["--mode", "minimal"])
        if isinstance(second, dict) and (second["sorted"] != theirs or second.get("moves")):
            lines.append(f"minimal: libloot's order {theirs} came back as {second['sorted']} with moves {second.get('moves')}")
        return lines
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
            assert "groups:" in (root / "game" / "masterlist.yaml").read_text()
        finally:
            shutil.rmtree(root, ignore_errors=True)
    assert world(random.Random(5)) == world(random.Random(5)), "a seed gives one world"
    # the metadata generators: conditions of every function, subs, tags, cleaning data, regex entries, CRCs of the bytes
    seen: set[str] = set()
    rng = random.Random(3)
    for _ in range(60):
        w = world(rng)
        text = json.dumps(w)
        seen.update(token for token in ("file(", "active(", "many(", "many_active(", "checksum(", "is_master(", " and ", " or ", "not (",
                                        "subs", "\"tag\"", "\"dirty\"", "\"clean\"", "{crc:self}", "{crc:?}", "\\\\d") if token in text)
        root = Path(tempfile.mkdtemp(prefix="random-diff-selftest-"))
        try:
            write(w, root / "game")
            masterlist = (root / "game" / "masterlist.yaml").read_text()
            assert not CRC_TOKEN.search(masterlist)
            for line in masterlist.splitlines():
                found = re.search(r"checksum\(\"([^\"]+)\", ([0-9A-F]{8})\)", line)
                if found and found.group(2) != "00000001" and found.group(1) in w["order"] + ["FalloutNV.esm"]:
                    holder = next(path for home in ("Data", "Extra") if (path := root / "game" / home / found.group(1)).is_file())
                    assert int(found.group(2), 16) == zlib.crc32(holder.read_bytes())
            for entry in w["entries"]:
                for item in entry.get("dirty", []) + entry.get("clean", []):
                    if item["crc"] == "0x{crc:self}":
                        file = next(path for home in ("Data", "Extra") if (path := root / "game" / home / entry["name"]).is_file())
                        crc = zlib.crc32(file.read_bytes())
                        assert f"0x{crc:08X}" in masterlist  # the cleaning data matches the invented plugin bytes
        finally:
            shutil.rmtree(root, ignore_errors=True)
    assert len(seen) >= 15, sorted(seen)
    # an altered message, tag, group or cleaning entry makes the comparison fail
    sample = {"group_after": {"g1": []}}
    ours = {"general_messages": [], "plugins": [{"name": "A.esp", "group": "g1", "messages": [{"type": "say", "text": "t"}],
                                                  "tags_add": ["Relev"], "tags_remove": []}],
            "dirty": [{"plugin": "A.esp", "crc": 1, "status": "dirty", "utility": "u", "itm": 1, "udr": 0, "nav": 0}]}
    loot = {"general_messages": [], "groups": [{"name": "g1", "after": []}, {"name": "default", "after": []}],
            "plugins": {"A.esp": {"group": "g1", "messages": [{"type": "say", "text": "t"}], "tags_add": ["Relev"], "tags_remove": []}},
            "dirty": ours["dirty"]}
    assert compare_metadata(sample, ours, loot) == []
    for altered in ({**loot, "plugins": {"A.esp": {**loot["plugins"]["A.esp"], "messages": []}}},
                    {**loot, "plugins": {"A.esp": {**loot["plugins"]["A.esp"], "tags_add": []}}},
                    {**loot, "plugins": {"A.esp": {**loot["plugins"]["A.esp"], "group": "default"}}},
                    {**loot, "dirty": []}, {**loot, "groups": [{"name": "g1", "after": ["default"]}, {"name": "default", "after": []}]}):
        assert compare_metadata(sample, ours, altered), altered
    # the minimal-change checks: a valid order passes, a violated constraint, a missing reason and a changed valid order fail
    w = {"plugins": [{"name": "A.esm", "esm": True, "masters": ["FalloutNV.esm"]},
                     {"name": "B.esp", "esm": False, "masters": ["FalloutNV.esm", "A.esm"]},
                     {"name": "C.esp", "esm": False, "masters": ["FalloutNV.esm"]}],
         "entries": [{"name": "C.esp", "after": ["B.esp"]}]}
    valid = ["FalloutNV.esm", "A.esm", "B.esp", "C.esp"]
    assert check_minimal(w, valid, {"sorted": valid, "mode": "minimal", "moves": []}, valid) == []
    wrong = ["FalloutNV.esm", "A.esm", "C.esp", "B.esp"]
    assert any("violates" in line for line in check_minimal(w, valid, {"sorted": wrong, "mode": "minimal", "moves": []}, valid))
    assert any("valid but came back" in line for line in check_minimal(w, valid, {"sorted": valid, "mode": "minimal", "moves": [{"plugin": "C.esp", "reason": "after"}]}, valid))
    assert check_minimal(w, wrong, {"sorted": valid, "mode": "minimal", "moves": [{"plugin": "C.esp", "reason": "after"}]}, valid) == []
    assert any("without a reason" in line for line in check_minimal(w, wrong, {"sorted": valid, "mode": "minimal", "moves": [{"plugin": "C.esp"}]}, valid))
    assert displaced(["a", "b", "c"], ["a", "c", "b"]) == 1 and displaced(["a", "b"], ["a", "b"]) == 0
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
    differences = errors = meta_differences = minimal_differences = 0
    for number in range(count):
        w = world(rng, most)
        lines = compare_world_metadata(w, binary, driver)
        if lines:
            meta_differences += 1
            if meta_differences <= 5:
                print(f"world {number} (seed {seed}): metadata\n  " + "\n  ".join(lines)[:2000])
                print(f"  world: {json.dumps(w)}")
        lines = compare_world_minimal(w, binary, driver)
        if lines:
            minimal_differences += 1
            if minimal_differences <= 5:
                print(f"world {number} (seed {seed}): minimal-change\n  " + "\n  ".join(lines)[:2000])
                print(f"  world: {json.dumps(w)}")
        same, ours, theirs, order = compare_world(w, binary, driver)
        if isinstance(ours, str) and isinstance(theirs, str):
            errors += 1
        if not same:
            differences += 1
            if differences <= 5:
                print(f"world {number} (seed {seed}): current order {order}\n  loot-rs {ours}\n  libloot {theirs}")
                print(f"  world: {json.dumps(w)}")
    print(f"{count} worlds (seed {seed}), {differences} order difference(s), {meta_differences} metadata difference(s), "
          f"{minimal_differences} minimal-change difference(s), {errors} where both refused")
    print("what libloot reported and loot-rs matched (after conditions): " + ", ".join(f"{k} {v}" for k, v in COVERED.items()))
    return 1 if differences or meta_differences or minimal_differences else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
