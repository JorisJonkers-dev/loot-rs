#!/usr/bin/env python3
"""Compare the output of loot-rs with what LOOT (libloot) reports for the same load order.

PENDING REAL LOOT RUNS: nothing in this repository records LOOT output yet. This script only compares two
JSON files; docs/ORACLE.md describes how the LOOT side is produced and what is and is not comparable.

    loot-rs sort --json ... > ours.json          (or `check --json`: then there is no order to compare)
    scripts/oracle/loot_driver.py ... > loot.json
    scripts/oracle/compare.py --ours ours.json --loot loot.json

The LOOT file has the shape (every key optional):

    {
      "general_messages": [{"type": "say|warn|error", "text": "..."}],
      "plugins": {"A.esp": {"messages": [...], "tags_add": [...], "tags_remove": [...], "group": "..."}},
      "sorted": ["FalloutNV.esm", "A.esp", ...],      # the order LOOT's sort produced
      "dirty": [{"plugin": "A.esp", "crc": 1, "status": "dirty|clean", "utility": "", "itm": 0, "udr": 0, "nav": 0}]
    }

Compared: the sorted order (it should be equal), messages (type and normalised text, as sets: order is not significant), bash tags, groups when LOOT
gives them, cleaning data matched by CRC (the `dirty` list), and, when "sorted" is given, whether every
load-after constraint loot-rs reports as violated, and every `after`, `req` and header-master rule of `loot-rs sort`
(its `rules`), is satisfied by LOOT's own order. Rules that come from groups are printed as `note:` lines and
do not count, and so does how far the two orders are apart (`info:`): LOOT also sorts by overlap and priority.
Exit status: 0 no difference, 1 differences, 2 bad input.
"""
import json
import re
import sys


def normalise(text: str) -> str:
    """Whitespace and the markdown link syntax LOOT messages may carry are not significant here."""
    text = re.sub(r"\[([^\]]*)\]\([^)]*\)", r"\1", text)
    return re.sub(r"\s+", " ", text).strip()


def message_set(messages):
    return {(m["type"], normalise(m["text"])) for m in messages or []}


def violated_constraints(ours):
    """(later, earlier) pairs from the load-after findings: `later` must come after `earlier`."""
    pairs = []
    for plugin in ours.get("plugins", []):
        for finding in plugin.get("findings", []):
            if finding["kind"] != "load after":
                continue
            # "must load after B.esp, but B.esp is later..." / "requires B.esp, but B.esp is later..."
            match = re.match(r"(?:must load after|requires) (.+?), but .+ is later in the load order$", finding["text"])
            if match:
                pairs.append((plugin["name"], match.group(1)))
    return pairs


def compare(ours: dict, loot: dict) -> list[str]:
    """All lines; those starting with `note:` or `info:` are not differences."""
    differences = []
    ours_general = message_set(ours.get("general_messages"))
    if "general_messages" in loot and ours_general != message_set(loot["general_messages"]):
        differences.append(
            f"general messages differ: only loot-rs {sorted(ours_general - message_set(loot['general_messages']))}, "
            f"only LOOT {sorted(message_set(loot['general_messages']) - ours_general)}"
        )
    ours_plugins = {p["name"].lower(): p for p in ours.get("plugins", [])}
    for name, theirs in loot.get("plugins", {}).items():
        mine = ours_plugins.get(name.lower())
        if mine is None:
            differences.append(f"{name}: LOOT reports it, loot-rs does not list it")
            continue
        if "messages" in theirs and message_set(mine["messages"]) != message_set(theirs["messages"]):
            differences.append(
                f"{name}: messages differ: only loot-rs {sorted(message_set(mine['messages']) - message_set(theirs['messages']))}, "
                f"only LOOT {sorted(message_set(theirs['messages']) - message_set(mine['messages']))}"
            )
        for key in ("tags_add", "tags_remove"):
            if key in theirs and sorted(mine[key]) != sorted(theirs[key]):
                differences.append(f"{name}: {key} differ: loot-rs {sorted(mine[key])}, LOOT {sorted(theirs[key])}")
        if "group" in theirs and mine["group"] != theirs["group"]:
            differences.append(f"{name}: group differs: loot-rs {mine['group']!r}, LOOT {theirs['group']!r}")
    if "dirty" in loot:
        def cleaning(report):
            return {(d["plugin"].lower(), d["status"], d["crc"], d["utility"], d["itm"], d["udr"], d["nav"]) for d in report.get("dirty", [])}
        mine, theirs = cleaning(ours), cleaning(loot)
        if mine != theirs:
            differences.append(f"cleaning data differs: only loot-rs {sorted(mine - theirs)}, only LOOT {sorted(theirs - mine)}")
    if "sorted" in loot:
        position = {name.lower(): index for index, name in enumerate(loot["sorted"])}

        def before(first, second):
            return first.lower() in position and second.lower() in position and position[first.lower()] < position[second.lower()]

        def known(first, second):
            return first.lower() in position and second.lower() in position

        for later, earlier in violated_constraints(ours):
            if known(later, earlier) and not before(earlier, later):
                differences.append(f"loot-rs says {later} must load after {earlier}, LOOT's order puts it before")
        for rule in ours.get("rules", []):
            if not known(rule["before"], rule["after"]) or before(rule["before"], rule["after"]):
                continue
            text = f"{rule['source']} rule: {rule['before']} before {rule['after']}, LOOT's order has them the other way"
            differences.append(text if rule["source"] != "group" else "note: " + text)
        if "sorted" in ours:
            names = [name for name in ours["sorted"] if name.lower() in position]
            apart = sum(
                1 for i, a in enumerate(names) for b in names[i + 1:] if position[a.lower()] > position[b.lower()]
            )
            differences.append(f"info: {apart} of {len(names) * (len(names) - 1) // 2} plugin pairs are in a different order")
            # loot-rs sort ports LOOT's algorithm, so the same plugins in the same order should come out. (Until the
            # record and archive overlap data is read, a real load order whose plugins change the same records can
            # differ: see docs/COMPATIBILITY.md.)
            ours_order = [name.lower() for name in ours["sorted"]]
            loot_order = [name.lower() for name in loot["sorted"]]
            if sorted(ours_order) == sorted(loot_order) and ours_order != loot_order:
                at = next(i for i, (a, b) in enumerate(zip(ours_order, loot_order)) if a != b)
                differences.append(
                    f"order differs from LOOT's, first at position {at + 1}: loot-rs {ours['sorted'][at]}, LOOT {loot['sorted'][at]}"
                )
    return differences


def selftest() -> None:
    """Checks of the comparison logic on invented data. These are NOT LOOT results."""
    ours = {
        "general_messages": [{"type": "say", "text": "Hello  world"}],
        "plugins": [
            {
                "name": "A.esp",
                "group": "default",
                "messages": [{"type": "warn", "text": "See [the page](http://x) now"}],
                "tags_add": ["Relev"],
                "tags_remove": [],
                "findings": [
                    {"kind": "load after", "hard": True, "text": "must load after B.esp, but B.esp is later in the load order"}
                ],
            }
        ],
    }
    same = {
        "general_messages": [{"type": "say", "text": "Hello world"}],
        "plugins": {"a.esp": {"messages": [{"type": "warn", "text": "See the page now"}], "tags_add": ["Relev"], "tags_remove": [], "group": "default"}},
        "sorted": ["B.esp", "A.esp"],
    }
    assert compare(ours, same) == [], compare(ours, same)
    assert len(compare(ours, {"general_messages": []})) == 1
    changed = {"plugins": {"A.esp": {"messages": [{"type": "error", "text": "See the page now"}]}}}
    assert "messages differ" in compare(ours, changed)[0]
    assert "tags_add differ" in compare(ours, {"plugins": {"A.esp": {"tags_add": []}}})[0]
    assert "group differs" in compare(ours, {"plugins": {"A.esp": {"group": "Late"}}})[0]
    assert "does not list it" in compare(ours, {"plugins": {"Z.esp": {}}})[0]
    wrong_order = {"sorted": ["A.esp", "B.esp"]}
    assert "LOOT's order puts it before" in compare(ours, wrong_order)[0]
    assert compare(ours, {"sorted": ["Other.esp"]}) == []
    assert violated_constraints(ours) == [("A.esp", "B.esp")]
    assert normalise(" a \n b ") == "a b"
    sorted_ours = {
        "sorted": ["B.esp", "A.esp", "C.esp"],
        "plugins": [],
        "rules": [
            {"before": "B.esp", "after": "A.esp", "source": "after"},
            {"before": "C.esp", "after": "A.esp", "source": "group"},
        ],
        "dirty": [{"plugin": "A.esp", "crc": 5, "status": "dirty", "utility": "u", "itm": 1, "udr": 0, "nav": 0}],
    }
    agree = {"sorted": ["B.esp", "A.esp", "C.esp"], "dirty": sorted_ours["dirty"]}
    lines = compare(sorted_ours, agree)
    assert [line for line in lines if not line.startswith("note:")] == ["info: 0 of 3 plugin pairs are in a different order"], lines
    other_order = {"sorted": ["B.esp", "C.esp", "A.esp"], "dirty": sorted_ours["dirty"]}
    lines = compare(sorted_ours, other_order)
    assert any("1 of 3" in line for line in lines), lines
    assert "order differs from LOOT's, first at position 2: loot-rs A.esp, LOOT C.esp" in lines, lines
    flipped = compare(sorted_ours, {"sorted": ["A.esp", "B.esp", "C.esp"]})
    assert any(line.startswith("after rule: B.esp before A.esp") for line in flipped), flipped
    assert any(line.startswith("note: group rule: C.esp before A.esp") for line in flipped), flipped
    assert "cleaning data differs" in compare(sorted_ours, {"dirty": []})[0]
    assert compare(sorted_ours, {"sorted": ["X.esp"]}) == ["info: 0 of 0 plugin pairs are in a different order"]
    print("compare.py: selftest ok")


def main(argv: list[str]) -> int:
    if argv == ["--selftest"]:
        selftest()
        return 0
    try:
        ours_path, loot_path = argv[argv.index("--ours") + 1], argv[argv.index("--loot") + 1]
        with open(ours_path, encoding="utf-8") as ours, open(loot_path, encoding="utf-8") as loot:
            differences = compare(json.load(ours), json.load(loot))
    except (ValueError, IndexError, OSError, KeyError) as error:
        print(f"compare.py: {error}\n\n{__doc__}", file=sys.stderr)
        return 2
    for line in differences:
        print(line)
    counted = [line for line in differences if not line.startswith(("note:", "info:"))]
    print(f"{len(counted)} difference(s)")
    return 1 if counted else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
