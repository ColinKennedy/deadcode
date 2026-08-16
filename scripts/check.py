#!/usr/bin/env python3
"""Run this project's checks, locally or in CI.

    python scripts/check.py                # everything
    python scripts/check.py rust           # fmt, clippy, test
    python scripts/check.py python         # ruff, mypy, privata
    python scripts/check.py ruff mypy      # pick individual checks
    python scripts/check.py --list         # show what exists

The GitHub workflow invokes these exact commands, one job per check, so CI and
local runs cannot drift apart.

Deliberately stdlib-only and dependency-free, so it runs under any Python
without an install step. It shells out to `cargo` for Rust, and to `uv` for
Python — `uv` gives every Python tool the versions pinned in
legacy-python/requirements-dev.txt on any OS, which the existing
legacy-python/Makefile cannot do (it hardcodes Unix `.venv/bin/` paths).
"""

from __future__ import annotations

import argparse
import shutil
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
LEGACY = ROOT / "legacy-python"

# The Python implementation is archived but still checked, and it targets 3.10
# (pyproject `requires-python`, mypy `python_version`). Pinning the interpreter
# matters: on a newer default, installing the dev requirements tries to compile
# cffi from source and fails.
PY = "3.10"

# Pinned tool versions come from this file, so the checks match what the
# project actually declares rather than whatever is newest.
_PY_TOOL = [
    "uv", "run", "--quiet", "--no-project", "-p", PY,
    "--with-requirements", "requirements-dev.txt",
]


class Check:
    def __init__(
        self,
        name: str,
        group: str,
        summary: str,
        command: list[str],
        cwd: Path,
        blocking: bool = True,
    ) -> None:
        self.name = name
        self.group = group
        self.summary = summary
        self.command = command
        self.cwd = cwd
        self.blocking = blocking


CHECKS = [
    Check("fmt", "rust", "cargo fmt --check", ["cargo", "fmt", "--check"], ROOT),
    Check("clippy", "rust", "cargo clippy (warnings deny)",
          ["cargo", "clippy", "--all-targets", "--", "-D", "warnings"], ROOT),
    Check("test", "rust", "cargo test", ["cargo", "test"], ROOT),
    Check("ruff", "python", "ruff lint (legacy-python)",
          [*_PY_TOOL, "ruff", "check", "deadcode", "tests"], LEGACY),
    Check("ruff-format", "python", "ruff format --check (legacy-python)",
          [*_PY_TOOL, "ruff", "format", "--check", "deadcode", "tests"], LEGACY),
    Check("mypy", "python", "mypy (legacy-python)",
          [*_PY_TOOL, "mypy", "deadcode"], LEGACY),
    Check("privata", "python", "module privacy (legacy-python)",
          ["uvx", "privata", "."], LEGACY),
]

BY_NAME = {c.name: c for c in CHECKS}
GROUPS = sorted({c.group for c in CHECKS})


def resolve(selectors: list[str]) -> list[Check]:
    if not selectors:
        return list(CHECKS)
    chosen: list[Check] = []
    for selector in selectors:
        if selector in GROUPS:
            chosen += [c for c in CHECKS if c.group == selector]
        elif selector in BY_NAME:
            chosen.append(BY_NAME[selector])
        else:
            known = ", ".join([*GROUPS, *BY_NAME])
            sys.exit(f"unknown check {selector!r}. Known: {known}")
    # Preserve declaration order, drop duplicates.
    return [c for c in CHECKS if c in chosen]


def run(check: Check) -> bool:
    tool = check.command[0]
    if shutil.which(tool) is None:
        print(f"  SKIP  {check.name}: {tool!r} not found on PATH")
        # A missing toolchain must not silently look like success.
        return not check.blocking

    print(f"\n=== {check.name}: {check.summary}")
    print(f"    $ {' '.join(check.command)}   (in {check.cwd.name or '.'})")
    started = time.monotonic()
    completed = subprocess.run(check.command, cwd=check.cwd, check=False)
    elapsed = time.monotonic() - started
    ok = completed.returncode == 0
    if ok:
        print(f"    PASS ({elapsed:.1f}s)")
    elif check.blocking:
        print(f"    FAIL ({elapsed:.1f}s) exit={completed.returncode}")
    else:
        print(f"    FAIL ({elapsed:.1f}s) exit={completed.returncode} - advisory, not gating")
    return ok or not check.blocking


def main() -> int:
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    parser.add_argument("checks", nargs="*", help="check or group names; default all")
    parser.add_argument("--list", action="store_true", help="list checks and exit")
    args = parser.parse_args()

    if args.list:
        width = max(len(c.name) for c in CHECKS)
        for check in CHECKS:
            flag = "" if check.blocking else "  (advisory)"
            print(f"  {check.name:<{width}}  [{check.group}]  {check.summary}{flag}")
        return 0

    selected = resolve(args.checks)
    results = {check.name: run(check) for check in selected}

    print("\n" + "=" * 60)
    for check in selected:
        print(f"  {'ok  ' if results[check.name] else 'FAIL'}  {check.name}")
    failed = [name for name, ok in results.items() if not ok]
    if failed:
        print(f"\n{len(failed)} failed: {', '.join(failed)}")
        return 1
    print(f"\nall {len(selected)} checks passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
