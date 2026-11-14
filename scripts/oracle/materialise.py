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


def synthetic_records(flags: int, masters: list[str], form_ids: list[int]) -> bytes:
    """The bytes of loot_rs::records::synthetic_records(flags, masters, &[(*b"NPC_", records of size 0)])."""
    body = b"".join(b"NPC_" + struct.pack("<IIIQ", 0, 0, form_id, 0)[:12] + b"\0" * 8 for form_id in form_ids)
    group = b"GRUP" + struct.pack("<I", 24 + len(body)) + b"NPC_" + b"\0" * 12 + body
    return synthetic_plugin(flags, masters) + group


def synthetic_bsa(folders: list[tuple[str, list[str]]]) -> bytes:
    """The bytes of loot_rs::archives::synthetic_bsa(104, folders): the tables of a version 104 archive, no data."""
    file_count = sum(len(files) for _, files in folders)
    folder_names_length = sum(len(name) + 1 for name, _ in folders)
    file_names_length = sum(len(file) + 1 for _, files in folders for file in files)
    baseline = 36 + 16 * len(folders) + file_names_length
    head = b"BSA\0" + struct.pack(
        "<8I", 104, 36, 3, len(folders), file_count, folder_names_length, file_names_length, 0
    )
    records, folder_records = b"", b""
    for name, files in folders:
        folder_records += b"\0" * 8 + struct.pack("<II", len(files), baseline + len(records))
        records += bytes([len(name) + 1]) + name.encode("latin-1") + b"\0" + b"\0" * (16 * len(files))
    names = b"".join(file.encode("latin-1") + b"\0" for _, files in folders for file in files)
    return head + folder_records + records + names


def materialise(case: Path, out: Path) -> None:
    data = out / "Data"
    data.mkdir(parents=True, exist_ok=True)
    for line in (case / "data.txt").read_text().splitlines():
        words = line.split()
        if not words:
            continue
        if words[0] == "plugin":
            flags, masters, form_ids = 0, [], []
            for word in words[2:]:
                if word == "esm":
                    flags |= FLAG_ESM
                elif word.startswith("masters="):
                    masters = word[len("masters="):].split(",")
                elif word.startswith("records="):
                    form_ids = [int(text, 16) for text in word[len("records="):].split(",")]
                else:
                    raise SystemExit(f"unknown plugin option {word!r}")
            target = data / words[1]
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(synthetic_records(flags, masters, form_ids) if form_ids else synthetic_plugin(flags, masters))
        elif words[0] == "bsa":
            folders = []
            for folder in words[2].split(";"):
                name, files = folder.split("=", 1)
                folders.append((name, [file for file in files.split(",") if file]))
            target = data / words[1]
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(synthetic_bsa(folders))
        elif words[0] == "file":
            target = data / words[1]
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(words[2].removeprefix("text=").encode())
        else:
            raise SystemExit(f"unknown data line {line!r}")
    (out / "plugins.txt").write_text((case / "plugins.txt").read_text())
    (out / "masterlist.yaml").write_text((case / "masterlist.yaml").read_text())


def ensure_master(out: Path, name: str = "FalloutNV.esm") -> None:
    """libloot refuses a game directory without the game's master file: add an empty one (and list it first)."""
    target = out / "Data" / name
    if not target.exists():
        target.write_bytes(synthetic_plugin(FLAG_ESM, []))
    listed = (out / "plugins.txt").read_text()
    if name.lower() not in [line.strip().lstrip("*").lower() for line in listed.splitlines()]:
        (out / "plugins.txt").write_text(f"{name}\n{listed}")


def selftest() -> None:
    plugin = synthetic_plugin(FLAG_ESM, ["FalloutNV.esm"])
    assert plugin[:4] == b"TES4"
    size, flags = struct.unpack_from("<II", plugin, 4)
    assert flags == FLAG_ESM and size == len(plugin) - 24
    assert b"MAST" in plugin and b"FalloutNV.esm\0" in plugin
    # Same bytes as the Rust side: header, then one group of 24-byte records.
    records = synthetic_records(0, [], [0x800, 0x01000001])
    header = synthetic_plugin(0, [])
    assert records[: len(header)] == header and len(records) == len(header) + 24 + 2 * 24
    assert records[len(header):][:4] == b"GRUP" and struct.unpack_from("<I", records, len(header) + 4)[0] == 24 + 48
    first = records[len(header) + 24:]
    assert first[:4] == b"NPC_" and struct.unpack_from("<I", first, 12)[0] == 0x800
    archive = synthetic_bsa([("meshes", ["a.nif", "b.nif"]), ("textures", ["c.dds"])])
    assert archive[:4] == b"BSA\0" and struct.unpack_from("<I", archive, 4)[0] == 104
    assert archive.endswith(b"a.nif\0b.nif\0c.dds\0") and len(archive) == 36 + 32 + (1 + 7 + 32) + (1 + 9 + 16) + 18
    with tempfile.TemporaryDirectory() as tmp:
        case = Path(tmp) / "case"
        case.mkdir()
        (case / "data.txt").write_text(
            "plugin A.esm esm\nplugin B.esp masters=A.esm records=800,1000001\nbsa B.bsa meshes=a.nif\nfile docs/x.txt text=hello\n"
        )
        (case / "plugins.txt").write_text("A.esm\nB.esp\n")
        (case / "masterlist.yaml").write_text("plugins: []\n")
        out = Path(tmp) / "out"
        materialise(case, out)
        assert (out / "Data" / "docs" / "x.txt").read_bytes() == b"hello"
        assert struct.unpack_from("<I", (out / "Data" / "A.esm").read_bytes(), 8)[0] == FLAG_ESM
        assert (out / "Data" / "B.esp").read_bytes() == synthetic_records(0, ["A.esm"], [0x800, 0x1000001])
        assert (out / "Data" / "B.bsa").read_bytes() == synthetic_bsa([("meshes", ["a.nif"])])
        assert (out / "plugins.txt").read_text() == "A.esm\nB.esp\n"
        ensure_master(out)
        assert (out / "plugins.txt").read_text() == "FalloutNV.esm\nA.esm\nB.esp\n"
        assert (out / "Data" / "FalloutNV.esm").exists()
        ensure_master(out)
        assert (out / "plugins.txt").read_text() == "FalloutNV.esm\nA.esm\nB.esp\n"
    print("materialise.py: selftest ok")


if __name__ == "__main__":
    if sys.argv[1:] == ["--selftest"]:
        selftest()
    elif len(sys.argv) == 3:
        materialise(Path(sys.argv[1]), Path(sys.argv[2]))
    else:
        raise SystemExit(__doc__)
