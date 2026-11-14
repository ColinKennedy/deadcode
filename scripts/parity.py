#!/usr/bin/env python3
"""Compare the Rust CLI against the preserved upstream Python implementation.

    python scripts/parity.py              # run every case, check the baseline
    python scripts/parity.py --show       # also print each difference
    python scripts/parity.py --update     # rewrite the baseline from this run

Each case is a small project: a directory under tests/parity/cases/, plus every
fixture under legacy-python/tests/files/ (copied, never modified). A case may
hold an `args` file with extra CLI arguments, one per line.

Every case is checked three ways:

* ``report``     -- `deadcode . --no-color` must print the same findings and
                    exit with the same status in both implementations.
* ``dry``        -- `deadcode . --no-color --dry` must render the same diff.
* ``fix-parses`` -- after the Rust `deadcode . --fix`, every file that parsed
                    before must still parse. This one has no Python
                    counterpart: `--fix` writing broken code is always a bug.

Differences that are understood but not yet fixed are listed in
tests/parity/known_differences.txt, one `<case> <check>` per line with a
`# reason`. The run fails on any difference missing from that list, and on any
listed difference that no longer occurs -- so the list can only shrink, and
"parity" is something CI measures rather than something a PR description
claims.

Two cosmetic differences are normalised away rather than tracked: ANSI escape
codes, and the `./` prefix Rust puts on paths (upstream prints `m.py`, Rust
`./m.py`).

The legacy implementation runs in the interpreter executing this script, so run
it where `deadcode` (legacy-python) is importable -- `scripts/check.py parity`
arranges that with uv.
"""

from __future__ import annotations

import argparse
import ast
import difflib
import os
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
CASES = ROOT / "tests" / "parity" / "cases"
LEGACY_FIXTURES = ROOT / "legacy-python" / "tests" / "files"
BASELINE = ROOT / "tests" / "parity" / "known_differences.txt"
CHECKS = ("report", "dry", "fix-parses")

LEGACY_ENTRY = (
    "import sys; from deadcode.cli import print_main; "
    "sys.exit(print_main(sys.argv[1:]))"
)
ANSI = re.compile(r"\x1b\[[0-9;]*m")
DOT_SLASH = re.compile(r"(^|(?<=[-+]{3} ))\./", re.MULTILINE)


def rust_binary() -> Path:
    subprocess.run(["cargo", "build", "--quiet"], cwd=ROOT, check=True)
    name = "deadcode.exe" if os.name == "nt" else "deadcode"
    return ROOT / "target" / "debug" / name


def collect_cases() -> dict[str, Path]:
    cases = {path.name: path for path in sorted(CASES.iterdir()) if path.is_dir()}
    for fixture in sorted(LEGACY_FIXTURES.iterdir()):
        name = f"legacy-{fixture.stem}"
        if name in cases:
            sys.exit(f"case name collision: {name}")
        cases[name] = fixture
    return cases


def stage(source: Path, destination: Path) -> list[str]:
    """Copy a case into `destination`; return its extra CLI arguments."""
    if source.is_dir():
        shutil.copytree(source, destination)
    else:
        destination.mkdir()
        shutil.copy2(source, destination / source.name)
    args_file = destination / "args"
    if not args_file.exists():
        return []
    args = [line for line in args_file.read_text(encoding="utf-8").splitlines() if line]
    args_file.unlink()
    return args


def normalise(text: str) -> str:
    return DOT_SLASH.sub("", ANSI.sub("", text)).replace("\r\n", "\n").strip()


def run(command: list[str], cwd: Path) -> str:
    environment = {**os.environ, "PYTHONIOENCODING": "utf-8", "NO_COLOR": "1"}
    completed = subprocess.run(
        command, cwd=cwd, capture_output=True, encoding="utf-8", env=environment, check=False
    )
    return normalise(completed.stdout + completed.stderr) + f"\n[exit {completed.returncode}]"


def unparseable(directory: Path) -> set[str]:
    broken = set()
    for path in sorted(directory.rglob("*.py")):
        try:
            ast.parse(path.read_bytes())
        except (SyntaxError, ValueError):
            broken.add(path.relative_to(directory).as_posix())
    return broken


def check_case(name: str, source: Path, rust: Path, work: Path) -> dict[str, str | None]:
    """Return {check: description of the difference, or None if they agree}."""
    results: dict[str, str | None] = {}
    for check, extra in (("report", []), ("dry", ["--dry"])):
        # A fresh copy for each side: upstream deletes files `--dry` would
        # empty, which would otherwise hide them from the Rust run.
        outputs = []
        for side, command in (("python", [sys.executable, "-c", LEGACY_ENTRY]), ("rust", [str(rust)])):
            directory = work / f"{name}-{check}-{side}"
            args = [".", "--no-color", *extra, *stage(source, directory)]
            outputs.append(run([*command, *args], directory))
        python, native = outputs
        results[check] = None if python == native else _diff(python, native)

    directory = work / f"{name}-fix"
    args = stage(source, directory)
    broken_before = unparseable(directory)
    subprocess.run([str(rust), ".", "--fix", "--no-color", *args], cwd=directory,
                   capture_output=True, check=False)
    newly_broken = sorted(unparseable(directory) - broken_before)
    results["fix-parses"] = (
        "--fix left unparseable files: " + ", ".join(newly_broken) if newly_broken else None
    )
    return results


def _diff(python: str, native: str) -> str:
    lines = difflib.unified_diff(
        python.splitlines(), native.splitlines(), "python", "rust", lineterm="", n=1
    )
    return "\n".join(lines)


def read_baseline() -> dict[tuple[str, str], str]:
    known = {}
    for line in BASELINE.read_text(encoding="utf-8").splitlines():
        entry, _, reason = line.partition("#")
        if not entry.strip():
            continue
        name, check = entry.split()
        if check not in CHECKS:
            sys.exit(f"{BASELINE.name}: unknown check {check!r} in {line!r}")
        known[(name, check)] = reason.strip()
    return known


def write_baseline(differences: set[tuple[str, str]], previous: dict[tuple[str, str], str]) -> None:
    header = [
        line for line in BASELINE.read_text(encoding="utf-8").splitlines()
        if line.startswith("#")
    ] if BASELINE.exists() else []
    body = [
        f"{name} {check}" + (f"  # {previous[(name, check)]}" if previous.get((name, check)) else "")
        for name, check in sorted(differences)
    ]
    BASELINE.write_text("\n".join([*header, *body]) + "\n", encoding="utf-8")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("cases", nargs="*", help="only run these cases")
    parser.add_argument("--show", action="store_true", help="print every difference")
    parser.add_argument("--update", action="store_true", help="rewrite the baseline")
    options = parser.parse_args()
    # Findings include emoji and non-ASCII names; a cp1252 console cannot print them.
    sys.stdout.reconfigure(errors="backslashreplace")  # type: ignore[union-attr]

    rust = rust_binary()
    cases = collect_cases()
    if options.cases:
        cases = {name: cases[name] for name in options.cases}
    known = read_baseline() if BASELINE.exists() else {}

    differences: dict[tuple[str, str], str] = {}
    with tempfile.TemporaryDirectory() as temporary:
        for name, source in cases.items():
            for check, difference in check_case(name, source, rust, Path(temporary)).items():
                if difference is not None:
                    differences[(name, check)] = difference

    if options.update:
        kept = {key: reason for key, reason in known.items() if key[0] not in cases}
        write_baseline(set(differences) | set(kept), known)
        print(f"wrote {len(differences) + len(kept)} entries to {BASELINE.relative_to(ROOT)}")
        return 0

    relevant = {key for key in known if key[0] in cases}
    unexpected = sorted(set(differences) - relevant)
    fixed = sorted(relevant - set(differences))
    for key in sorted(differences):
        if options.show or key in unexpected:
            label = "NEW " if key in unexpected else "known"
            print(f"--- {label} {key[0]} {key[1]}\n{differences[key]}\n")
    for name, check in fixed:
        print(f"FIXED {name} {check}: now matches -- remove it from {BASELINE.name}")

    total = len(cases) * len(CHECKS)
    print(f"{total - len(differences)}/{total} checks agree; "
          f"{len(differences)} known differences, {len(unexpected)} new, {len(fixed)} fixed")
    return 1 if unexpected or fixed else 0


if __name__ == "__main__":
    sys.exit(main())
