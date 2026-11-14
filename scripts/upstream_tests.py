#!/usr/bin/env python3
"""Run legacy-python's own pytest suite against the Rust binary.

    python scripts/upstream_tests.py          # check against the baseline
    python scripts/upstream_tests.py --update # rewrite the baseline

`rust_main_plugin.py` swaps upstream's CLI entry points for the Rust binary,
so upstream's assertions judge the Rust output. Tests expected to fail are
listed in tests/parity/upstream_expected_failures.txt with a reason; the run
fails on any other failure and on a listed test that now passes, so the list
can only shrink.

Run it where legacy-python's `deadcode` and pytest are importable --
`scripts/check.py upstream` arranges that with uv.
"""

from __future__ import annotations

import argparse
import os
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
LEGACY = ROOT / "legacy-python"
BASELINE = ROOT / "tests" / "parity" / "upstream_expected_failures.txt"


def rust_binary() -> Path:
    subprocess.run(["cargo", "build", "--quiet"], cwd=ROOT, check=True)
    name = "deadcode.exe" if os.name == "nt" else "deadcode"
    return ROOT / "target" / "debug" / name


def failed_tests(binary: Path) -> set[str]:
    environment = {
        **os.environ,
        "DEADCODE_RUST_BINARY": str(binary),
        "PYTHONPATH": os.pathsep.join(
            filter(None, [str(ROOT / "scripts"), os.environ.get("PYTHONPATH")])
        ),
    }
    # `addopts=` drops upstream's 100% coverage gate: the Rust binary, not
    # the Python package, now runs the CLI.
    command = [
        sys.executable, "-m", "pytest", "-q", "-p", "rust_main_plugin",
        "-p", "no:cacheprovider", "-o", "addopts=", "-rf", "tests",
    ]
    completed = subprocess.run(
        command, cwd=LEGACY, env=environment, capture_output=True, encoding="utf-8",
        errors="replace", check=False,
    )
    if completed.returncode not in (0, 1):
        sys.exit(f"pytest could not run the suite:\n{completed.stdout}\n{completed.stderr}")
    return {
        line.split()[1]
        for line in completed.stdout.splitlines()
        if line.startswith("FAILED ")
    }


def read_baseline() -> dict[str, str]:
    expected = {}
    for line in BASELINE.read_text(encoding="utf-8").splitlines():
        entry, _, reason = line.partition("#")
        if entry.strip():
            expected[entry.strip()] = reason.strip()
    return expected


def main() -> int:
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    parser.add_argument("--update", action="store_true", help="rewrite the baseline")
    options = parser.parse_args()

    failures = failed_tests(rust_binary())
    expected = read_baseline() if BASELINE.exists() else {}

    if options.update:
        header = [
            line for line in BASELINE.read_text(encoding="utf-8").splitlines()
            if line.startswith("#")
        ] if BASELINE.exists() else []
        body = [
            f"{test}  # {expected[test]}" if expected.get(test) else test
            for test in sorted(failures)
        ]
        BASELINE.write_text("\n".join([*header, *body]) + "\n", encoding="utf-8")
        print(f"wrote {len(failures)} entries to {BASELINE.relative_to(ROOT)}")
        return 0

    unexpected = sorted(failures - set(expected))
    fixed = sorted(set(expected) - failures)
    for test in unexpected:
        print(f"NEW FAILURE {test}")
    for test in fixed:
        print(f"FIXED {test}: now passes -- remove it from {BASELINE.name}")
    print(f"{len(failures)} expected failures, {len(unexpected)} new, {len(fixed)} fixed")
    return 1 if unexpected or fixed else 0


if __name__ == "__main__":
    sys.exit(main())
