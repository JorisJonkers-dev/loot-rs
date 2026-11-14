#!/usr/bin/env python3
"""oracle-ci-report: turn the result of an oracle_run.py run into the public `digest/real-data` status and an issue body.

    scripts/oracle/oracle_ci_report.py --exit-code N [--l0 FILE] --sha SHA --run-url URL
    scripts/oracle/oracle_ci_report.py --selftest

This is the only thing between the private runner and public text (ADR-0002). It reads oracle_run.py's L0 line
(`{suite, status, n_datasets, n_plugins, n_missing, n_differences, digest, tool}`) and prints one JSON object for the
workflow to post:

    {"state": "success|failure|error", "description": "...", "issue_title": "...", "issue_body": "..."}

Everything printed is built from fixed words and integers that passed an allow-list. No string of the input is
copied except the 64 hex digits of the digest and the 7 to 40 hex digits of the tool commit; anything else (an unknown
field, a wrong type, a status that does not agree with the exit code) makes the result `error` with a fixed
description, so free text from the real data cannot reach a status or an issue by this path. Only a run that
compared every plugin of the load order and found no difference is `success`: `skipped` and `partial` are never green.

Exit status: 0 printed a result (including `failure` and `error`; the workflow decides from the state), 2 usage.
"""
import contextlib
import io
import json
import re
import sys
from pathlib import Path

SUITE = "load-order-vs-libloot"
CONTEXT = "digest/real-data"
STATUSES = {"pass": 0, "fail": 1, "skipped": 3, "partial": 4}
COUNTS = ("n_datasets", "n_plugins", "n_missing", "n_differences")
FIELDS = {"suite", "status", "digest", "tool", *COUNTS}
MARKER = "<!-- oracle-ci:real-data -->"
TITLE = "Real-data oracle is red"


def _count(value) -> bool:
    return isinstance(value, int) and not isinstance(value, bool) and 0 <= value < 10**9


def parse(raw: str | None, exit_code: int) -> dict | None:
    """The validated L0 record, or None when it cannot be trusted."""
    if raw is None:
        return None
    try:
        record = json.loads(raw)
    except ValueError:
        return None
    if not isinstance(record, dict) or set(record) != FIELDS or record["suite"] != SUITE:
        return None
    if record["status"] not in STATUSES or STATUSES[record["status"]] != exit_code:
        return None
    if not all(_count(record[key]) for key in COUNTS):
        return None
    if not (isinstance(record["digest"], str) and re.fullmatch(r"[0-9a-f]{64}", record["digest"])):
        return None
    if not (isinstance(record["tool"], str) and re.fullmatch(r"[0-9a-f]{7,40}", record["tool"])):
        return None
    return record


def report(raw: str | None, exit_code: int, sha: str, run_url: str) -> dict:
    record = parse(raw, exit_code)
    short = sha[:7]
    if record is None:
        word = "the oracle run failed to produce a trusted result" if exit_code in (0, 1, 3, 4) else \
            "the oracle run stopped before it compared anything"
        return {"state": "error", "description": f"error: {word}",
                "issue_title": TITLE, "issue_body": body(f"error ({word})", None, short, run_url)}
    counts = (f"datasets {record['n_datasets']}, plugins {record['n_plugins']}, missing {record['n_missing']}, "
              f"differences {record['n_differences']}, digest {record['digest'][:12]}")
    if record["status"] == "pass":
        return {"state": "success", "description": f"pass: {counts}", "issue_title": TITLE,
                "issue_body": body("pass", record, short, run_url)}
    return {"state": "failure", "description": f"{record['status']}: {counts}", "issue_title": TITLE,
            "issue_body": body(record["status"], record, short, run_url)}


def body(status: str, record: dict | None, short: str, run_url: str) -> str:
    lines = [MARKER, f"Result of the private real-data oracle on `{short}`: **{status}**.", ""]
    if record:
        lines += [f"- datasets: {record['n_datasets']}", f"- plugins compared: {record['n_plugins']}",
                  f"- plugins of the load order not found: {record['n_missing']}",
                  f"- differences: {record['n_differences']}", f"- digest: `{record['digest']}`", ""]
    lines += ["The detail (names, paths, both orders) stays on the runner and is not in this issue. The owner reads it "
              "there and in the run log of the private repository; the run is " + run_url + ".", "",
              "This is fixed forward: nothing was reverted. The `digest/real-data` status turns green on the next run "
              "that compares every plugin and finds no difference, and until then it blocks the release pull request."]
    return "\n".join(lines)


def main(argv: list[str]) -> int:
    if argv == ["--selftest"]:
        selftest()
        return 0
    opts = {"--exit-code": None, "--l0": None, "--sha": None, "--run-url": None}
    args = iter(argv)
    for arg in args:
        if arg not in opts:
            print(__doc__, file=sys.stderr)
            return 2
        opts[arg] = next(args, None)
    code, sha, url = opts["--exit-code"], opts["--sha"], opts["--run-url"]
    if code is None or not code.isdigit() or not sha or not re.fullmatch(r"[0-9a-f]{7,40}", sha) or not url \
            or not re.fullmatch(r"https://[A-Za-z0-9./_-]+", url):
        print(__doc__, file=sys.stderr)
        return 2
    raw = None
    if opts["--l0"]:
        try:
            raw = Path(opts["--l0"]).read_text()
        except OSError:
            raw = None
    print(json.dumps(report(raw, int(code), sha, url)))
    return 0


def selftest() -> None:
    sha, url = "a" * 40, "https://github.com/o/r/actions/runs/1"
    good = {"suite": SUITE, "status": "pass", "n_datasets": 2, "n_plugins": 300, "n_missing": 0, "n_differences": 0,
            "digest": "b" * 64, "tool": "c" * 40}

    def run(record, code):
        return report(json.dumps(record) if record is not None else None, code, sha, url)

    assert run(good, 0)["state"] == "success"
    assert run(good, 0)["description"].startswith("pass: datasets 2, plugins 300")
    assert run({**good, "status": "fail", "n_differences": 3}, 1)["state"] == "failure"
    assert run({**good, "status": "skipped"}, 3)["state"] == "failure"
    assert run({**good, "status": "partial", "n_missing": 4}, 4)["state"] == "failure"
    # the status and the exit code must agree; a green line with a failing exit is an error
    assert run(good, 1)["state"] == "error"
    assert run({**good, "status": "fail"}, 0)["state"] == "error"
    # free text, extra fields, wrong types and a missing record never pass through
    leak = {**good, "status": "fail", "note": "Secret Mod.esp"}
    assert run(leak, 1)["state"] == "error" and "Secret" not in json.dumps(run(leak, 1))
    for bad in ({**good, "digest": "Secret Mod.esp"}, {**good, "tool": "../etc"}, {**good, "n_plugins": "3"},
                {**good, "n_plugins": True}, {**good, "n_plugins": -1}, {**good, "suite": "other"}, [good], "x"):
        assert run(bad, 0)["state"] == "error", bad
    assert report("not json", 0, sha, url)["state"] == "error"
    assert run(None, 2)["description"].endswith("before it compared anything")
    assert len(run(good, 0)["description"]) <= 140
    assert MARKER in run(good, 0)["issue_body"]
    with contextlib.redirect_stderr(io.StringIO()):  # the usage text
        assert main([]) == 2 and main(["--exit-code", "x", "--sha", sha, "--run-url", url]) == 2
        assert main(["--exit-code", "0", "--sha", "zz", "--run-url", url]) == 2
        assert main(["--exit-code", "0", "--sha", sha, "--run-url", "javascript:1"]) == 2
    print("oracle_ci_report.py: selftest ok")


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
