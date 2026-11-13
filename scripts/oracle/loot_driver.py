#!/usr/bin/env python3
"""Run libloot on a load order and write the JSON that compare.py reads.

Needs the Python bindings of libloot (the `loot_api` module, https://github.com/loot/libloot, GPL-3.0), which are
NOT part of this repository and are not installed in CI. The calls below follow the public API documentation
(https://loot-api.readthedocs.io); the names are looked up defensively because they changed between libloot
releases, and the first real run is what proves them. Nothing here records or invents LOOT output.

    loot_driver.py --game-path <dir that holds Data> --masterlist masterlist.yaml --plugins plugins.txt > loot.json
    loot_driver.py --selftest

The load order is the plugin names of plugins.txt (a leading * is dropped), in file order.
"""
import json
import sys
from pathlib import Path


def plugin_names(text: str) -> list[str]:
    names = []
    for line in text.splitlines():
        line = line.strip()
        if line and not line.startswith("#"):
            names.append(line.removeprefix("*").strip())
    return names


def english(content) -> str:
    """A message's text: libloot 0.2x gives a list of (text, language), older ones a string."""
    if isinstance(content, str):
        return content
    items = list(content or [])
    for item in items:
        if getattr(item, "language", "en") in ("en", "eng", ""):
            return item.text
    return items[0].text if items else ""


def message_json(message) -> dict:
    kind = getattr(message.type, "name", str(message.type)).split(".")[-1].lower()
    return {"type": kind, "text": english(message.content).strip()}


def cleaning_json(plugin: str, status: str, crc: int, data) -> dict:
    return {
        "plugin": plugin,
        "crc": crc,
        "status": status,
        "utility": getattr(data, "cleaning_utility", ""),
        "itm": getattr(data, "itm_count", 0),
        "udr": getattr(data, "deleted_reference_count", 0),
        "nav": getattr(data, "deleted_navmesh_count", 0),
    }


def collect(loot_api, game_path: str, masterlist: str, names: list[str]) -> dict:
    """Ask libloot (`loot_api`, or a stand-in with the same calls) what it says about the load order."""
    game_type = next(
        (getattr(loot_api.GameType, name) for name in ("fonv", "fnv", "FalloutNV") if hasattr(loot_api.GameType, name)),
        None,
    )
    if game_type is None:
        raise SystemExit("loot_driver.py: this libloot has no Fallout: New Vegas game type")
    handle = loot_api.create_game_handle(game_type, game_path)
    database = handle.get_database()
    database.load_masterlist(masterlist)
    handle.load_plugins(names, True)
    result = {
        "general_messages": [message_json(m) for m in database.general_messages(True)],
        "plugins": {},
        "dirty": [],
        "sorted": list(handle.sort_plugins(names)),
    }
    for name in names:
        metadata = database.plugin_metadata(name, False, True)
        entry = {"messages": [], "tags_add": [], "tags_remove": [], "group": "default"}
        if metadata is not None:
            entry["messages"] = [message_json(m) for m in metadata.messages]
            for tag in metadata.tags:
                entry["tags_add" if tag.is_addition else "tags_remove"].append(tag.name)
            entry["group"] = metadata.group or "default"
            crc = handle.get_plugin(name).crc()
            for status, items in (("dirty", metadata.dirty_info), ("clean", metadata.clean_info)):
                for data in items:
                    if crc is not None and data.crc == crc:
                        result["dirty"].append(cleaning_json(name, status, crc, data))
        result["plugins"][name] = entry
    return result


def selftest() -> None:
    """The conversion on a stand-in for loot_api. These are NOT LOOT results."""
    from types import SimpleNamespace as NS

    kind = lambda name: NS(name=name)  # noqa: E731
    tag = lambda name, add: NS(name=name, is_addition=add)  # noqa: E731
    cleaning = NS(crc=7, cleaning_utility="FNVEdit", itm_count=2, deleted_reference_count=1, deleted_navmesh_count=0)
    metadata = {
        "A.esp": NS(
            messages=[NS(type=kind("warn"), content=[NS(text="Hola", language="es"), NS(text=" Hello ", language="en")])],
            tags=[tag("Relev", True), tag("Delev", False)],
            group="late",
            dirty_info=[cleaning, NS(crc=8, cleaning_utility="x", itm_count=0, deleted_reference_count=0, deleted_navmesh_count=0)],
            clean_info=[],
        )
    }
    database = NS(
        load_masterlist=lambda path: None,
        general_messages=lambda evaluate: [NS(type=kind("say"), content="General")],
        plugin_metadata=lambda name, user, evaluate: metadata.get(name),
    )
    handle = NS(
        get_database=lambda: database,
        load_plugins=lambda names, headers: None,
        sort_plugins=lambda names: list(reversed(names)),
        get_plugin=lambda name: NS(crc=lambda: 7),
    )
    fake = NS(GameType=NS(fonv=object()), create_game_handle=lambda game, path: handle)
    result = collect(fake, "/game", "m.yaml", ["A.esp", "B.esp"])
    assert result["sorted"] == ["B.esp", "A.esp"]
    assert result["general_messages"] == [{"type": "say", "text": "General"}]
    assert result["plugins"]["A.esp"] == {
        "messages": [{"type": "warn", "text": "Hello"}],
        "tags_add": ["Relev"],
        "tags_remove": ["Delev"],
        "group": "late",
    }
    assert result["plugins"]["B.esp"] == {"messages": [], "tags_add": [], "tags_remove": [], "group": "default"}
    assert result["dirty"] == [
        {"plugin": "A.esp", "crc": 7, "status": "dirty", "utility": "FNVEdit", "itm": 2, "udr": 1, "nav": 0}
    ]
    assert plugin_names("# c\n*A.esm\n B.esp \n\n") == ["A.esm", "B.esp"]
    try:
        collect(NS(GameType=NS()), "/g", "m", [])
    except SystemExit:
        pass
    else:
        raise AssertionError("a libloot without the game type must stop")
    print("loot_driver.py: selftest ok")


def main(argv: list[str]) -> int:
    if argv == ["--selftest"]:
        selftest()
        return 0
    try:
        game = argv[argv.index("--game-path") + 1]
        masterlist = argv[argv.index("--masterlist") + 1]
        plugins = argv[argv.index("--plugins") + 1]
    except (ValueError, IndexError):
        print(__doc__, file=sys.stderr)
        return 2
    try:
        import loot_api  # type: ignore[import-not-found]
    except ImportError:
        print("loot_driver.py: the loot_api module (libloot's Python bindings) is not installed", file=sys.stderr)
        return 3
    names = plugin_names(Path(plugins).read_text(encoding="utf-8"))
    json.dump(collect(loot_api, game, masterlist, names), sys.stdout)
    print()
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
