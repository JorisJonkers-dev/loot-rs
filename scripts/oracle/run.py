#!/usr/bin/env python3
"""Run loot-rs and libloot on the same load orders and compare them.

    # the synthetic cases of the repository (tests/golden, tests/golden_sort): no game files needed
    scripts/oracle/run.py --loot-rs target/release/loot-rs --driver "python3 scripts/oracle/loot_driver.py" --all

    # one case, or a real load order of the owner's machine (read only; nothing is written to the game)
    scripts/oracle/run.py --loot-rs ... --driver ... --case tests/golden_sort/groups
    scripts/oracle/run.py --loot-rs ... --driver ... --real --game-path ~/Games/FNV \\
        --data ~/Games/FNV/Data --masterlist masterlist.yaml --plugins plugins.txt

    scripts/oracle/run.py --selftest

Without --driver (or when libloot's bindings are not installed) the LOOT side is skipped and the exit status is 3:
that is "nothing was compared", never "no difference". Exit status: 0 compared, no difference; 1 differences;
2 error; 3 skipped. The results (ours.json, loot.json, and the differences) are left under --work (default: a new
directory under the system temporary directory), never in the repository.
"""
import json
import os
import shlex
import subprocess
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent
sys.path.insert(0, str(HERE))
import compare  # noqa: E402
import materialise  # noqa: E402

CASE_DIRS = ("tests/golden_sort", "tests/golden")


def run_ours(binary: str, data: Path, masterlist: Path, plugins: Path, extra: list[str]) -> dict:
    """`loot-rs sort --json`: exit 0 and 1 (violations) are results, anything else is an error."""
    command = [binary, "sort", "--masterlist", str(masterlist), "--plugins", str(plugins), "--data", str(data), *extra, "--json"]
    done = subprocess.run(command, capture_output=True, text=True, check=False)
    if done.returncode not in (0, 1):
        raise RuntimeError(f"loot-rs exited {done.returncode}: {done.stderr.strip()}")
    return json.loads(done.stdout)


def run_driver(driver: str, game_path: Path, masterlist: Path, plugins: Path) -> dict | None:
    """The libloot side; None when it is not available (exit status 3 of the driver)."""
    command = [*shlex.split(driver), "--game-path", str(game_path), "--masterlist", str(masterlist), "--plugins", str(plugins)]
    done = subprocess.run(command, capture_output=True, text=True, check=False)
    if done.returncode == 3:
        print(done.stderr.strip(), file=sys.stderr)
        return None
    if done.returncode != 0:
        raise RuntimeError(f"the driver exited {done.returncode}: {done.stderr.strip()}")
    return json.loads(done.stdout)


def run_one(name: str, binary: str, driver: str | None, game_path: Path, data: Path, masterlist: Path, plugins: Path,
            extra: list[str], work: Path) -> tuple[str, list[str]]:
    """("compared" or "skipped", the lines of compare.compare)."""
    ours = run_ours(binary, data, masterlist, plugins, extra)
    (work / "ours.json").write_text(json.dumps(ours, indent=1))
    loot = run_driver(driver, game_path, masterlist, plugins) if driver else None
    if loot is None:
        return "skipped", []
    (work / "loot.json").write_text(json.dumps(loot, indent=1))
    lines = compare.compare(ours, loot)
    (work / "differences.txt").write_text("\n".join(lines) + "\n")
    return "compared", lines


def main(argv: list[str]) -> int:
    if argv == ["--selftest"]:
        selftest()
        return 0
    options = {"--loot-rs": None, "--driver": None, "--work": None, "--game-path": None, "--data": None,
               "--masterlist": None, "--plugins": None}
    cases: list[Path] = []
    real = all_cases = False
    args = iter(argv)
    for arg in args:
        if arg in options:
            options[arg] = next(args, None)
        elif arg == "--case":
            cases.append(Path(next(args, "")))
        elif arg == "--all":
            all_cases = True
        elif arg == "--real":
            real = True
        else:
            print(__doc__, file=sys.stderr)
            return 2
    if not options["--loot-rs"] or not (real or cases or all_cases):
        print(__doc__, file=sys.stderr)
        return 2
    work = Path(options["--work"] or tempfile.mkdtemp(prefix="loot-rs-oracle-"))
    binary, driver = options["--loot-rs"], options["--driver"]
    jobs: list[tuple[str, Path, Path, Path, Path, list[str]]] = []
    try:
        if real:
            missing = [key for key in ("--game-path", "--data", "--masterlist", "--plugins") if not options[key]]
            if missing:
                print(f"--real needs {', '.join(missing)}", file=sys.stderr)
                return 2
            out = work / "real"
            out.mkdir(parents=True, exist_ok=True)
            jobs.append(("real", Path(options["--game-path"]), Path(options["--data"]), Path(options["--masterlist"]),
                         Path(options["--plugins"]), []))
        if all_cases:
            cases += sorted(p for d in CASE_DIRS for p in (ROOT / d).iterdir() if p.is_dir())
        for case in cases:
            out = work / case.parent.name / case.name
            out.mkdir(parents=True, exist_ok=True)
            materialise.materialise(case, out)
            materialise.ensure_master(out)
            extra = (case / "args").read_text().split() if (case / "args").exists() else []
            jobs.append((f"{case.parent.name}/{case.name}", out, out / "Data", out / "masterlist.yaml", out / "plugins.txt", extra))
        failed = skipped = 0
        for name, game, data, masterlist, plugins, extra in jobs:
            folder = work / name
            folder.mkdir(parents=True, exist_ok=True)
            status, lines = run_one(name, binary, driver, game, data, masterlist, plugins, extra, folder)
            counted = [line for line in lines if not line.startswith(("note:", "info:"))]
            print(f"{name}: {status}" + (f", {len(counted)} difference(s)" if status == "compared" else ""))
            for line in lines:
                print(f"    {line}")
            skipped += status == "skipped"
            failed += bool(counted)
    except (RuntimeError, OSError, ValueError, KeyError) as error:
        print(f"run.py: {error}", file=sys.stderr)
        return 2
    print(f"results under {work}")
    if skipped:
        return 3
    return 1 if failed else 0


def selftest() -> None:
    """The orchestration with a stand-in for loot-rs and for the driver. These are NOT LOOT results."""
    with tempfile.TemporaryDirectory() as tmp:
        tmp = Path(tmp)
        case = tmp / "cases" / "one"
        case.mkdir(parents=True)
        (case / "data.txt").write_text("plugin A.esp\nplugin B.esp\n")
        (case / "plugins.txt").write_text("A.esp\nB.esp\n")
        (case / "masterlist.yaml").write_text("plugins: []\n")
        ours = {"sorted": ["FalloutNV.esm", "A.esp", "B.esp"], "plugins": [], "rules": [{"before": "A.esp", "after": "B.esp", "source": "after"}]}
        stub = tmp / "loot-rs"
        stub.write_text(f"#!/bin/sh\necho '{json.dumps(ours)}'\n")
        stub.chmod(0o755)
        agree = tmp / "agree.py"
        agree.write_text("import json,sys\nprint(json.dumps({'sorted': ['FalloutNV.esm', 'A.esp', 'B.esp']}))\n")
        differ = tmp / "differ.py"
        differ.write_text("import json,sys\nprint(json.dumps({'sorted': ['B.esp', 'A.esp']}))\n")
        absent = tmp / "absent.py"
        absent.write_text("import sys\nprint('no libloot', file=sys.stderr)\nsys.exit(3)\n")
        broken = tmp / "broken.py"
        broken.write_text("import sys\nsys.exit(1)\n")

        def run(driver: Path | None) -> int:
            work = tmp / f"work-{driver.name if driver else 'none'}"
            argv = ["--loot-rs", str(stub), "--case", str(case), "--work", str(work)]
            if driver:
                argv += ["--driver", f"{sys.executable} {driver}"]
            return main(argv)

        assert run(agree) == 0
        assert run(differ) == 1
        assert run(absent) == 3
        assert run(None) == 3
        assert run(broken) == 2
        assert (tmp / "work-agree.py" / "cases" / "one" / "ours.json").exists()
        assert (tmp / "work-differ.py" / "cases" / "one" / "differences.txt").read_text().startswith("after rule")
        assert main([]) == 2 and main(["--bogus"]) == 2 and main(["--loot-rs", "x"]) == 2
        assert main(["--loot-rs", "x", "--real"]) == 2
        assert main(["--loot-rs", str(tmp / "nonexistent"), "--case", str(case), "--work", str(tmp / "w")]) == 2
        real = tmp / "real-game" / "Data"
        real.mkdir(parents=True)
        (tmp / "real-game" / "plugins.txt").write_text("A.esp\n")
        (tmp / "real-game" / "m.yaml").write_text("plugins: []\n")
        argv = ["--loot-rs", str(stub), "--real", "--game-path", str(tmp / "real-game"), "--data", str(real),
                "--masterlist", str(tmp / "real-game" / "m.yaml"), "--plugins", str(tmp / "real-game" / "plugins.txt"),
                "--work", str(tmp / "real-work"), "--driver", f"{sys.executable} {agree}"]
        assert main(argv) == 0
        assert os.path.exists(tmp / "real-work" / "real" / "ours.json")
    print("run.py: selftest ok")


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
