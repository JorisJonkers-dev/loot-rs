#!/usr/bin/env python3
"""Build the Data directory of a golden case (tests/golden/<case>/data.txt) in a directory.

The plugins are the same synthetic header-only plugins that the Rust tests write, so that loot-rs and LOOT
(libloot) can be run on identical bytes.

    scripts/oracle/materialise.py tests/golden/req_missing /tmp/oracle/req_missing
    scripts/oracle/materialise.py --selftest
"""
import struct
import sys
import tempfile
from pathlib import Path

FLAG_ESM = 0x1


def subrecord(kind: bytes, payload: bytes) -> bytes:
    return kind + struct.pack("<H", len(payload)) + payload


def synthetic_plugin(flags: int, masters: list[str]) -> bytes:
    """The bytes of loot_rs::plugin::synthetic_plugin(flags, masters, None)."""
    data = subrecord(b"HEDR", struct.pack("<fII", 1.34, 0, 0x800))
    data += subrecord(b"CNAM", b"tester\0")
    for master in masters:
        data += subrecord(b"MAST", master.encode("latin-1") + b"\0")
        data += subrecord(b"DATA", struct.pack("<Q", 0))
    header = b"TES4" + struct.pack("<IIIIHH", len(data), flags, 0, 0, 15, 0)
    return header + data


def materialise(case: Path, out: Path) -> None:
    data = out / "Data"
    data.mkdir(parents=True, exist_ok=True)
    for line in (case / "data.txt").read_text().splitlines():
        words = line.split()
        if not words:
            continue
        if words[0] == "plugin":
            flags, masters = 0, []
            for word in words[2:]:
                if word == "esm":
                    flags |= FLAG_ESM
                elif word.startswith("masters="):
                    masters = word[len("masters="):].split(",")
                else:
                    raise SystemExit(f"unknown plugin option {word!r}")
            target = data / words[1]
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(synthetic_plugin(flags, masters))
        elif words[0] == "file":
            target = data / words[1]
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(words[2].removeprefix("text=").encode())
        else:
            raise SystemExit(f"unknown data line {line!r}")
    (out / "plugins.txt").write_text((case / "plugins.txt").read_text())
    (out / "masterlist.yaml").write_text((case / "masterlist.yaml").read_text())


def selftest() -> None:
    plugin = synthetic_plugin(FLAG_ESM, ["FalloutNV.esm"])
    assert plugin[:4] == b"TES4"
    size, flags = struct.unpack_from("<II", plugin, 4)
    assert flags == FLAG_ESM and size == len(plugin) - 24
    assert b"MAST" in plugin and b"FalloutNV.esm\0" in plugin
    with tempfile.TemporaryDirectory() as tmp:
        case = Path(tmp) / "case"
        case.mkdir()
        (case / "data.txt").write_text(
            "plugin A.esm esm\nplugin B.esp masters=A.esm\nfile docs/x.txt text=hello\n"
        )
        (case / "plugins.txt").write_text("A.esm\nB.esp\n")
        (case / "masterlist.yaml").write_text("plugins: []\n")
        out = Path(tmp) / "out"
        materialise(case, out)
        assert (out / "Data" / "docs" / "x.txt").read_bytes() == b"hello"
        assert struct.unpack_from("<I", (out / "Data" / "A.esm").read_bytes(), 8)[0] == FLAG_ESM
        assert (out / "plugins.txt").read_text() == "A.esm\nB.esp\n"
    print("materialise.py: selftest ok")


if __name__ == "__main__":
    if sys.argv[1:] == ["--selftest"]:
        selftest()
    elif len(sys.argv) == 3:
        materialise(Path(sys.argv[1]), Path(sys.argv[2]))
    else:
        raise SystemExit(__doc__)
