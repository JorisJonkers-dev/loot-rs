#!/usr/bin/env python3
"""oracle-run: run loot-rs on the owner's real load orders and compare it with libloot, keeping the output private.

    scripts/oracle/oracle_run.py --config ~/.config/nvmod/oracle.toml [--dataset NAME ...]
    scripts/oracle/oracle_run.py --selftest

Local only. It reads plugin HEADERS (the TES4 record) and directory listings; it never reads the body of a plugin or
of any other game file, never writes to the game or to an MO2 instance, and never writes into a git work tree.

The config lives outside every repository (default ~/.config/nvmod/oracle.toml) and holds the paths:

    [tools]
    loot_rs    = "/path/to/loot-rs"                     # release build, ideally with --features executable-versions
    masterlist = "/path/to/masterlist.yaml"
    libloot_driver = "/path/to/libloot-driver"          # the Rust driver of this repository (task oracle:random builds it)
    driver     = "python3 /path/to/loot_driver.py"      # alternative: libloot's Python bindings must be installed

    [oracle]                                            # optional
    perturbations = 20                                  # per dataset: also compare N seeded perturbations of the order
    seed          = 1                                   # (ORACLE_PERTURBATIONS and ORACLE_SEED override both)

    [output]
    root = "~/.local/share/nvmod-oracle"                # optional; this is the default

    [dataset.ttw]                                       # any name
    plugins   = "/path/to/profiles/Name/loadorder.txt"  # or plugins.txt; one plugin per line, "*" and "#" handled
    game_path = "/path/to/Fallout New Vegas"            # its Data/ is searched last
    mods_dir  = "/path/to/mods"                         # optional (MO2): <mods_dir>/<mod>/ searched by priority ...
    modlist   = "/path/to/profiles/Name/modlist.txt"    # ... as ordered and enabled ("+") in this file
    perturbations = 5                                   # optional: this dataset only

    [dataset.ttw-lite]                                  # a variant: the same plugin set with a subset enabled
    variant_of = "ttw"                                  # takes every key of [dataset.ttw]
    disable    = ["Some Plugin.esp"]                    # and/or: enabled_only = "/path/to/names.txt"

Plugins that are in the load order but found nowhere are listed as missing and left out of both runs, so a partial
install is compared on what is there (the counts say how partial). Nothing is compared when there is no driver.

Output has three levels, all under <root>/<run-id>/ (mode 0700), and only L0 is printed:

    l2/       full detail: names, paths, ours.json, loot.json, the differences. The owner reads it; it goes nowhere else.
    l1.json   redacted: ids (hashes), counts and digests. No names, no paths, no free text.
    l0.json   public status: {suite, status, counts, digest}. This is what stdout shows.

Exit status: 0 compared with no difference, 1 differences or a loot-rs failure, 2 usage or config error (including a
refused output location), 3 nothing was compared (no driver or no libloot), 4 compared and equal but some plugins of the
load order were not found (status "partial": the counts say how many).

The real libloot sorts by loading the plugins in full, in memory; it prints only the order, and that is all that is kept.
loot-rs reads headers and file lists only.
"""
import hashlib
import json
import os
import random
import secrets
import shlex
import subprocess
import struct
import sys
import tempfile
from datetime import datetime, timezone
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import compare  # noqa: E402
import run  # noqa: E402

DEFAULT_CONFIG = "~/.config/nvmod/oracle.toml"
DEFAULT_ROOT = "~/.local/share/nvmod-oracle"
PLUGIN_SUFFIXES = (".esp", ".esm", ".esl")
HEADER_CAP = 1 << 20
CATEGORIES = ("order", "after", "req", "inc", "master", "group", "message", "tag", "dirty", "load-after", "note", "info")


class Refused(Exception):
    """A location or a config that must not be used."""


def inside_git_worktree(path: Path) -> bool:
    """True when `path` (or its nearest existing ancestor) is in a git work tree or bare repository."""
    here = Path(os.path.realpath(path))
    while True:
        if (here / ".git").exists() or (here / "HEAD").is_file() and (here / "objects").is_dir():
            return True
        if here.parent == here:
            return False
        here = here.parent


def guard_write_location(path: Path, what: str) -> None:
    if inside_git_worktree(path):
        raise Refused(f"{what} is inside a git work tree; refusing to write there")


def header_digest(path: Path) -> str:
    """sha256 of the TES4 record (24-byte header and its fields) of a plugin; nothing after it is read."""
    with open(path, "rb") as handle:
        head = handle.read(24)
        if len(head) < 24 or head[:4] != b"TES4":
            return "invalid-header"
        size = min(struct.unpack("<I", head[4:8])[0], HEADER_CAP)
        return hashlib.sha256(head + handle.read(size)).hexdigest()


def short(text: str) -> str:
    return hashlib.sha256(text.encode()).hexdigest()[:12]


def read_names(path: Path) -> list[str]:
    names = []
    for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
        line = line.strip().lstrip("*").strip()
        if line and not line.startswith("#"):
            names.append(line)
    return names


def search_dirs(dataset: dict) -> list[Path]:
    """Directories to look for plugins in, highest priority first: enabled MO2 mods, then the game's Data."""
    dirs: list[Path] = []
    mods, modlist = dataset.get("mods_dir"), dataset.get("modlist")
    if mods and modlist:
        for line in Path(modlist).read_text(encoding="utf-8", errors="replace").splitlines():
            if line.startswith("+") and (Path(mods) / line[1:].strip()).is_dir():
                dirs.append(Path(mods) / line[1:].strip())
    if dataset.get("game_path"):
        dirs.append(Path(dataset["game_path"]) / "Data")
    return dirs


def index_dir(directory: Path) -> dict[str, Path]:
    try:
        return {entry.name.lower(): entry for entry in directory.iterdir() if entry.is_file()}
    except OSError:
        return {}


def stage(dataset: dict, names: list[str], out: Path) -> dict:
    """Symlink every plugin found into out/Data and write out/plugins.txt; returns the facts the logs need."""
    data = out / "Data"
    data.mkdir(parents=True)
    indexes = [index_dir(d) for d in search_dirs(dataset)]
    found, missing, archives = [], [], set()
    for index in indexes:
        archives.update(name for name in index if name.endswith(".bsa"))
    for name in names:
        hit = next((index[name.lower()] for index in indexes if name.lower() in index), None)
        if hit is None:
            missing.append(name)
            continue
        (data / name).symlink_to(hit)
        found.append(name)
    (out / "plugins.txt").write_text("\n".join(found) + "\n")
    digests = {name: header_digest(data / name) for name in found}
    return {"found": found, "missing": missing, "archives": sorted(archives), "digests": digests}


def category(line: str) -> str:
    word = line.split(" ", 1)[0].rstrip(":").lower()
    return word if word in CATEGORIES else "other"


def sort_libloot(driver: str, game: Path, local: Path, masterlist: Path, order: list[str]) -> object:
    """The Rust libloot driver (scripts/oracle/libloot-driver): the sorted names, or `error: ...` when libloot refuses.
    libloot loads the plugins in full in memory to sort them; only the resulting order is printed or kept."""
    local.mkdir(exist_ok=True)
    done = subprocess.run([*shlex.split(driver), str(game), str(local), str(masterlist), *order],
                          capture_output=True, text=True, check=False)
    try:
        result = json.loads(done.stdout)
    except ValueError as error:
        raise RuntimeError(f"the libloot driver failed ({done.returncode}): {done.stderr.strip()[-300:]}") from error
    return f"error: {result['error']}" if isinstance(result, dict) else result


def compare_sorted(ours: object, theirs: object) -> list[str]:
    """Equal orders, or both refusing, agree; anything else is one difference (the detail is for L2 only)."""
    if isinstance(ours, str) or isinstance(theirs, str):
        if isinstance(ours, str) and isinstance(theirs, str):
            return []
        return [f"order: only {'loot-rs' if isinstance(ours, str) else 'libloot'} refused to sort"]
    if ours == theirs:
        return []
    first = next((i for i, (a, b) in enumerate(zip(ours, theirs)) if a != b), min(len(ours), len(theirs)))
    moved = sum(1 for i, name in enumerate(ours) if i >= len(theirs) or theirs[i] != name)
    return [f"order: first divergence at position {first}, {moved} of {len(ours)} positions differ "
            f"(loot-rs {ours[first:first + 3]}, libloot {theirs[first:first + 3]})"]


PERTURBATION_KINDS = ("permutation", "reversed", "rotation")
DEFAULT_SEED = 1


def is_master_name(name: str) -> bool:
    return name.lower().endswith((".esm", ".esl"))


def perturbed_orders(order: list[str], masters: set[str], count: int, seed: int, label: str) -> list[tuple[str, list[str]]]:
    """`count` reproducible perturbations of `order`, kinds in turn: a random permutation, the reversed order, and a
    rotation of the non-master block (the masters stay where they are). Each is a permutation of the same plugins."""
    result = []
    for index in range(count):
        kind = PERTURBATION_KINDS[index % len(PERTURBATION_KINDS)]
        rng = random.Random(f"{seed}:{label}:{index}")
        if kind == "permutation":
            changed = list(order)
            rng.shuffle(changed)
        elif kind == "reversed":
            changed = order[::-1]
        else:
            firsts = [name for name in order if name in masters]
            rest = [name for name in order if name not in masters]
            turn = rng.randrange(1, len(rest)) if len(rest) > 1 else 0
            changed = firsts + rest[turn:] + rest[:turn]
        result.append((kind, changed))
    return result


def variant_names(dataset: dict, names: list[str]) -> list[str]:
    """The plugins of a variant: `disable` (a list of names) removes plugins, `enabled_only` (a file of names) keeps
    only those. The same plugin set with a subset enabled, in the order of the base file."""
    if dataset.get("enabled_only"):
        keep = {name.lower() for name in read_names(Path(dataset["enabled_only"]))}
        names = [name for name in names if name.lower() in keep]
    drop = {str(name).lower() for name in dataset.get("disable", [])}
    return [name for name in names if name.lower() not in drop]


def compare_order(tools: dict, stage_root: Path, order: list[str], found: list[str], plugins: Path, work: Path) -> dict:
    """Run both tools on one load order. Returns {status, lines, sorted, digest}; the details go under `work` (L2)."""
    work.mkdir(parents=True, exist_ok=True)
    masterlist = Path(tools["masterlist"])
    outcome: dict = {"status": "error", "lines": [], "sorted": 0, "digest": ""}
    try:
        ours: object = run.run_ours(tools["loot_rs"], stage_root / "Data", masterlist, plugins, [])
    except RuntimeError as error:
        (work / "ours-error.txt").write_text(f"{error}\n")  # may contain names: L2 only
        ours = f"error: {error}"
    except (OSError, ValueError) as error:
        (work / "error.txt").write_text(f"{error}\n")
        return outcome
    if isinstance(ours, dict):
        (work / "ours.json").write_text(json.dumps(ours, indent=1))
        outcome["sorted"] = len(ours.get("sorted", []))
        outcome["digest"] = hashlib.sha256(json.dumps(ours.get("sorted", [])).encode()).hexdigest()
    lines: list[str] | None = None
    try:
        if tools.get("libloot_driver"):
            theirs = sort_libloot(tools["libloot_driver"], stage_root, work / "local", masterlist, order)
            (work / "libloot-sorted.json").write_text(json.dumps(theirs, indent=1))
            lines = compare_sorted(ours["sorted"] if isinstance(ours, dict) else ours, theirs)
        elif tools.get("driver") and isinstance(ours, dict):
            loot = run.run_driver(tools["driver"], stage_root, masterlist, plugins)
            if loot is not None:
                (work / "loot.json").write_text(json.dumps(loot, indent=1))
                lines = compare.compare(ours, loot)
    except (RuntimeError, OSError, ValueError) as error:
        (work / "error.txt").write_text(f"{error}\n")
        return outcome
    if lines is None:
        outcome["status"] = "skipped"
        return outcome
    (work / "differences.txt").write_text("\n".join(lines) + "\n")
    outcome["lines"] = lines
    outcome["status"] = "fail" if [line for line in lines if not line.startswith(("note:", "info:"))] else "pass"
    return outcome


def one_dataset(label: str, dataset: dict, tools: dict, run_dir: Path, perturbations: int = 0, seed: int = DEFAULT_SEED) -> dict:
    l2 = run_dir / "l2" / label
    l2.mkdir(parents=True)
    names = variant_names(dataset, read_names(Path(dataset["plugins"])))
    facts = stage(dataset, names, l2 / "stage")
    (l2 / "missing.txt").write_text("\n".join(facts["missing"]) + "\n")
    (l2 / "found.txt").write_text("\n".join(facts["found"]) + "\n")
    dataset_digest = hashlib.sha256(
        "\n".join(f"{n.lower()} {facts['digests'][n]}" for n in sorted(facts["found"], key=str.lower)).encode()).hexdigest()
    result = {"id": f"ds-{short(label)}", "in_order": len(names), "present": len(facts["found"]),
              "missing": len(facts["missing"]), "archives": len(facts["archives"]), "dataset_digest": dataset_digest,
              "status": "error", "differences": 0, "notes": 0, "sorted": 0, "categories": {}, "difference_ids": [],
              "seed": seed, "n_perturbations": 0, "n_differences": 0, "perturbation_status": {},
              "perturbation_kinds": {}, "perturbation_digests": []}
    stage_root = l2 / "stage"
    base = compare_order(tools, stage_root, facts["found"], facts["found"], stage_root / "plugins.txt", l2)
    result["sorted"] = base["sorted"]
    if base["digest"]:
        result["ours_digest"] = base["digest"]
    if base["status"] in ("error", "skipped"):
        result["status"] = base["status"]
        return result
    counted = [line for line in base["lines"] if not line.startswith(("note:", "info:"))]
    result["differences"] = len(counted)
    result["notes"] = len(base["lines"]) - len(counted)
    for line in counted:
        result["categories"][category(line)] = result["categories"].get(category(line), 0) + 1
    result["difference_ids"] = [short(line) for line in counted]
    result["status"] = base["status"]
    result["n_differences"] = len(counted)
    masters = {name for name in facts["found"] if is_master_name(name)}
    for index, (kind, order) in enumerate(perturbed_orders(facts["found"], masters, perturbations, seed, label)):
        work = l2 / "perturbed" / f"{index:03d}-{kind}"  # the perturbed orders are written under L2 only
        work.mkdir(parents=True)
        plugins = work / "plugins.txt"
        plugins.write_text("\n".join(order) + "\n")
        outcome = compare_order(tools, stage_root, order, order, plugins, work)
        diffs = [line for line in outcome["lines"] if not line.startswith(("note:", "info:"))]
        result["n_perturbations"] += 1
        result["n_differences"] += len(diffs)
        result["perturbation_status"][outcome["status"]] = result["perturbation_status"].get(outcome["status"], 0) + 1
        kinds = result["perturbation_kinds"].setdefault(kind, {})
        kinds[outcome["status"]] = kinds.get(outcome["status"], 0) + 1
        result["perturbation_digests"].append(outcome["digest"])
        if outcome["status"] != "pass":
            if outcome["status"] == "fail" and result["status"] != "error":
                result["status"] = "fail"
            elif outcome["status"] != "fail":
                result["status"] = "error"
    return result


def public_status(results: list[dict], version: str) -> dict:
    statuses = {r["status"] for r in results}
    status = "fail" if statuses & {"fail", "error"} else "skipped" if "skipped" in statuses or not results else "pass"
    if status == "pass" and any(r["missing"] for r in results):
        status = "partial"  # compared and equal, but on fewer plugins than the load order has: never a green gate
    digest = hashlib.sha256(json.dumps([[r["dataset_digest"], r.get("ours_digest", ""), r["differences"], r["status"], r["seed"],
                                         r["n_perturbations"], r["n_differences"], r["perturbation_digests"]]
                                        for r in results], sort_keys=True).encode()).hexdigest()
    return {"suite": "load-order-vs-libloot", "status": status, "n_datasets": len(results),
            "n_plugins": sum(r["present"] for r in results), "n_missing": sum(r["missing"] for r in results),
            "n_differences": sum(r["n_differences"] for r in results), "digest": digest, "tool": version}


def write_private(path: Path, data) -> None:
    path.write_text(json.dumps(data, indent=1, sort_keys=True) + "\n")
    path.chmod(0o600)


def setting(env: str, configured: object, default: int) -> int:
    """A non-negative integer: the environment variable (the private workflow's dispatch input) over the config."""
    value = os.environ.get(env) or configured
    if value is None or value == "":
        return default
    if isinstance(value, bool) or not str(value).isdigit() or int(value) > 10**6:
        raise Refused(f"{env} (or its config key) is not a small non-negative integer")
    return int(value)


def expand_variants(datasets: dict) -> dict:
    """A dataset with `variant_of = "<other>"` takes every key of that dataset (same plugin set, paths) and overrides
    what it sets itself, usually `disable = [...]` or `enabled_only = "<file>"`."""
    expanded = {}
    for label, dataset in datasets.items():
        base = dataset.get("variant_of")
        if base is not None:
            if base not in datasets or datasets[base].get("variant_of") is not None:
                raise Refused(f"dataset {short(label)}: variant_of must name a dataset that is not itself a variant")
            dataset = {**datasets[base], **{k: v for k, v in dataset.items() if k != "variant_of"}}
        expanded[label] = dataset
    return expanded


def execute(config_path: Path, only: list[str]) -> int:
    import tomllib
    guard_write_location(config_path, "the config file")  # paths never belong in a repository
    config = tomllib.loads(config_path.read_text())
    tools = config.get("tools", {})
    for key in ("loot_rs", "masterlist"):
        if not tools.get(key):
            raise Refused(f"[tools] {key} is missing from the config")
    datasets = expand_variants(config.get("dataset", {}))
    chosen = {k: v for k, v in datasets.items() if not only or k in only}
    if not chosen or (only and set(only) - set(datasets)):
        raise Refused("no such dataset in the config")
    for label, dataset in chosen.items():
        if not dataset.get("plugins") or not Path(dataset["plugins"]).is_file():
            raise Refused(f"dataset {short(label)}: the plugins file is missing")
    root = Path(os.path.expanduser(config.get("output", {}).get("root", DEFAULT_ROOT)))
    guard_write_location(root, "the output root")
    run_dir = root / (datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ") + "-" + secrets.token_hex(3))
    run_dir.mkdir(parents=True, mode=0o700)
    root.chmod(0o700)
    seed = setting("ORACLE_SEED", config.get("oracle", {}).get("seed"), DEFAULT_SEED)
    default_count = config.get("oracle", {}).get("perturbations", 0)
    results = [one_dataset(label, dataset, tools, run_dir, setting("ORACLE_PERTURBATIONS", dataset.get("perturbations"),
                                                                 default_count), seed)
               for label, dataset in chosen.items()]
    version = os.environ.get("ORACLE_TOOL_SHA", "unknown")
    public = public_status(results, version)
    write_private(run_dir / "l1.json", {"run": run_dir.name, "datasets": results, "tool": version})
    write_private(run_dir / "l0.json", public)
    print(json.dumps(public, sort_keys=True))
    print(f"full detail: {run_dir}/l2 (local only)", file=sys.stderr)
    return {"pass": 0, "fail": 1, "skipped": 3, "partial": 4}[public["status"]]


def main(argv: list[str]) -> int:
    if argv == ["--selftest"]:
        selftest()
        return 0
    config, only, args = DEFAULT_CONFIG, [], iter(argv)
    for arg in args:
        if arg == "--config":
            config = next(args, "")
        elif arg == "--dataset":
            only.append(next(args, ""))
        else:
            print(__doc__, file=sys.stderr)
            return 2
    try:
        return execute(Path(os.path.expanduser(config)), only)
    except (Refused, OSError, KeyError, ValueError) as error:
        print(f"oracle-run: {error}", file=sys.stderr)
        return 2


def selftest() -> None:
    """Invented plugins, a stand-in loot-rs and drivers. These are NOT LOOT results."""
    with tempfile.TemporaryDirectory() as tmp:
        tmp = Path(tmp)
        git = tmp / "repo"
        (git / ".git").mkdir(parents=True)
        assert inside_git_worktree(git / "sub" / "deeper") and not inside_git_worktree(tmp / "outside")
        names = ["Base.esm", "Two.esm", *[f"P{i}.esp" for i in range(8)]]
        masters = {"Base.esm", "Two.esm"}
        first = perturbed_orders(names, masters, 9, 3, "x")
        assert first == perturbed_orders(names, masters, 9, 3, "x") != perturbed_orders(names, masters, 9, 4, "x")
        assert [kind for kind, _ in first[:3]] == list(PERTURBATION_KINDS)
        for kind, order in first:
            assert sorted(order) == sorted(names), kind  # a permutation of the same plugin set
        assert first[1][1] == names[::-1]
        for kind, order in first:
            if kind == "rotation":
                assert order[:2] == ["Base.esm", "Two.esm"] and order != names  # masters stay, the rest rotates
        assert perturbed_orders(names, masters, 0, 3, "x") == []
        assert variant_names({"disable": ["p1.ESP"]}, ["P0.esp", "P1.esp"]) == ["P0.esp"]

        def plugin(path: Path, extra: bytes = b"") -> None:
            path.parent.mkdir(parents=True, exist_ok=True)
            body = b"HEDR" + struct.pack("<H", 12) + b"\0" * 12
            path.write_bytes(b"TES4" + struct.pack("<I", len(body)) + b"\0" * 16 + body + extra)

        game = tmp / "game"
        plugin(game / "Data" / "FalloutNV.esm")
        plugin(game / "Data" / "A.esp", b"BODYSECRET")
        plugin(tmp / "mods" / "ModB" / "B.esp")
        (game / "Data" / "x.bsa").write_bytes(b"")
        (tmp / "modlist.txt").write_text("# header\n+ModB\n-Off\n")
        (tmp / "order.txt").write_text("*FalloutNV.esm\nA.esp\nB.esp\nGone.esp\n")
        ours = {"sorted": ["FalloutNV.esm", "A.esp", "B.esp"], "plugins": [], "rules": [{"before": "A.esp", "after": "B.esp", "source": "after"}]}
        stub = tmp / "loot-rs"
        stub.write_text(f"#!/bin/sh\necho '{json.dumps(ours)}'\n")
        stub.chmod(0o755)
        agree = tmp / "agree.py"
        agree.write_text("import json\nprint(json.dumps({'sorted': ['FalloutNV.esm', 'A.esp', 'B.esp']}))\n")
        differ = tmp / "differ.py"
        differ.write_text("import json\nprint(json.dumps({'sorted': ['B.esp', 'A.esp']}))\n")
        absent = tmp / "absent.py"
        absent.write_text("import sys\nsys.exit(3)\n")
        native = tmp / "native.sh"
        native.write_text("#!/bin/sh\necho '[\"FalloutNV.esm\",\"A.esp\",\"B.esp\"]'\n")
        native.chmod(0o755)
        native_differs = tmp / "native-differs.sh"
        native_differs.write_text("#!/bin/sh\necho '[\"FalloutNV.esm\",\"B.esp\",\"A.esp\"]'\n")
        native_differs.chmod(0o755)
        (tmp / "m.yaml").write_text("plugins: []\n")
        root = tmp / "out"

        def config(driver: Path | None, where: Path = tmp, native: Path | None = None) -> Path:
            lines = [f'[tools]\nloot_rs = "{stub}"\nmasterlist = "{tmp / "m.yaml"}"\n', f'[output]\nroot = "{root}"\n',
                     f'[dataset.t]\nplugins = "{tmp / "order.txt"}"\ngame_path = "{game}"\nmods_dir = "{tmp / "mods"}"\n'
                     f'modlist = "{tmp / "modlist.txt"}"\n']
            if driver:
                lines[0] += f'driver = "{sys.executable} {driver}"\n'
            if native:
                lines[0] += f'libloot_driver = "{native}"\n'
            path = where / "oracle.toml"
            path.write_text("".join(lines))
            return path

        def last_run() -> Path:
            return max(root.iterdir(), key=lambda d: d.stat().st_mtime_ns)

        import contextlib
        import io
        with contextlib.redirect_stderr(io.StringIO()):
            with contextlib.redirect_stdout(io.StringIO()) as out:
                assert main(["--config", str(config(agree))]) == 4
                assert main(["--config", str(config(None, native=native))]) == 4
                assert main(["--config", str(config(None, native=native_differs))]) == 1
                assert main(["--config", str(config(agree))]) == 4
            l0 = json.loads(out.getvalue().splitlines()[0])
            assert (l0["status"], l0["n_plugins"], l0["n_missing"], l0["n_differences"]) == ("partial", 3, 1, 0)
            blob = (last_run() / "l0.json").read_text() + (last_run() / "l1.json").read_text() + out.getvalue()
            for secret in ("A.esp", "B.esp", "Gone", "FalloutNV", str(tmp), "BODYSECRET"):
                assert secret not in blob, secret
            assert (last_run() / "l2" / "t" / "missing.txt").read_text().strip() == "Gone.esp"
            assert os.stat(last_run()).st_mode & 0o777 == 0o700
            with contextlib.redirect_stdout(io.StringIO()):
                assert main(["--config", str(config(differ))]) == 1
                assert main(["--config", str(config(absent))]) == 3
                assert main(["--config", str(config(None))]) == 3
                assert main(["--config", str(config(agree)), "--dataset", "nope"]) == 2
                assert main(["--bogus"]) == 2 and main(["--config", str(tmp / "missing.toml")]) == 2
                runs = len(list(root.iterdir()))
                assert main(["--config", str(config(agree, git))]) == 2  # config inside a work tree
                guarded = config(agree).read_text().replace(str(root), str(git / "logs"))
                (tmp / "guarded.toml").write_text(guarded)
                assert main(["--config", str(tmp / "guarded.toml")]) == 2 and not (git / "logs").exists()
                assert len(list(root.iterdir())) == runs
            (tmp / "order.txt").write_text("FalloutNV.esm\nA.esp\nB.esp\n")
            with contextlib.redirect_stdout(io.StringIO()) as full:
                assert main(["--config", str(config(agree))]) == 0
            assert json.loads(full.getvalue())["status"] == "pass"
            # perturbations: complete order, 20 seeded perturbations fed to both tools
            base_text = config(None, native=native).read_text()
            (tmp / "p.toml").write_text(base_text.replace("[output]", "[oracle]\nperturbations = 20\nseed = 7\n\n[output]"))
            with contextlib.redirect_stdout(io.StringIO()) as perturbed:
                assert main(["--config", str(tmp / "p.toml")]) == 0
            l1 = json.loads((last_run() / "l1.json").read_text())["datasets"][0]
            assert (l1["n_perturbations"], l1["n_differences"], l1["status"], l1["seed"]) == (20, 0, "pass", 7)
            assert l1["perturbation_status"] == {"pass": 20} and set(l1["perturbation_kinds"]) == set(PERTURBATION_KINDS)
            blob = perturbed.getvalue() + (last_run() / "l0.json").read_text() + (last_run() / "l1.json").read_text()
            for secret in ("A.esp", "B.esp", "FalloutNV", str(tmp)):
                assert secret not in blob, secret
            assert len(list((last_run() / "l2" / "t" / "perturbed").iterdir())) == 20
            digest = json.loads(perturbed.getvalue())["digest"]
            with contextlib.redirect_stdout(io.StringIO()) as again:
                assert main(["--config", str(tmp / "p.toml")]) == 0
            assert json.loads(again.getvalue())["digest"] == digest  # reproducible
            (tmp / "p2.toml").write_text((tmp / "p.toml").read_text().replace("seed = 7", "seed = 8"))
            with contextlib.redirect_stdout(io.StringIO()) as reseeded:
                assert main(["--config", str(tmp / "p2.toml")]) == 0
            assert json.loads(reseeded.getvalue())["digest"] != digest  # the seed is part of the digest
            (tmp / "p3.toml").write_text(base_text.replace("native.sh", "native-differs.sh"))
            os.environ["ORACLE_PERTURBATIONS"] = "4"
            try:
                with contextlib.redirect_stdout(io.StringIO()) as differing:
                    assert main(["--config", str(tmp / "p3.toml")]) == 1
                os.environ["ORACLE_PERTURBATIONS"] = "many"
                with contextlib.redirect_stdout(io.StringIO()):
                    assert main(["--config", str(tmp / "p3.toml")]) == 2
            finally:
                del os.environ["ORACLE_PERTURBATIONS"]
            assert json.loads(differing.getvalue())["n_differences"] == 5
            # a variant: the same plugin set without B.esp
            (tmp / "v.toml").write_text(base_text + '\n[dataset.lite]\nvariant_of = "t"\ndisable = ["b.ESP"]\n')
            with contextlib.redirect_stdout(io.StringIO()):
                main(["--config", str(tmp / "v.toml")])
            sizes = sorted(d["present"] for d in json.loads((last_run() / "l1.json").read_text())["datasets"])
            assert sizes == [2, 3]
            with contextlib.redirect_stdout(io.StringIO()):
                (tmp / "v2.toml").write_text(base_text + '\n[dataset.x]\nvariant_of = "nope"\n')
                assert main(["--config", str(tmp / "v2.toml")]) == 2
            assert compare_sorted(["a"], "error: x") != [] and compare_sorted("error: x", "error: y") == []
    print("oracle_run.py: selftest ok")


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
