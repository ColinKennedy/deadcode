#!/usr/bin/env python3
"""Run this project's checks, locally or in CI.

    python scripts/check.py                     # everything
    python scripts/check.py rust                # fmt, clippy, test
    python scripts/check.py python              # ruff, ruff-format, mypy, privata
    python scripts/check.py pytest              # legacy suite on every Python
    python scripts/check.py pytest --python 3.12   # ...or just one
    python scripts/check.py --list

The GitHub workflow invokes these exact commands, so CI and local runs cannot
drift apart.

Deliberately stdlib-only and dependency-free, so it runs under any Python
without an install step. It shells out to `cargo` for Rust and to `uv` for
Python. `uv` is what makes the version matrix cheap: it fetches whichever
interpreter a check asks for, so `pytest` can run on 3.10 through 3.14 from a
single command on any OS. The legacy-python/Makefile cannot do this -- it
hardcodes Unix `.venv/bin/` paths and needs `make`.
"""

from __future__ import annotations

import argparse
import re
import shutil
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
LEGACY = ROOT / "legacy-python"

# Oldest supported (legacy-python's `requires-python`) through newest released.
# Keep in sync with the classifiers in legacy-python/pyproject.toml and the
# matrix in .github/workflows/ci.yml.
PYTHON_VERSIONS = ["3.10", "3.11", "3.12", "3.13", "3.14"]

# Interpreter used for checks that analyse code rather than run it. Any version
# gives the same answer, so this only needs to be one the tools install on.
LINT_PY = "3.12"


def pinned(package: str) -> str:
    """Return `package==version` using legacy-python's pinned dev requirements.

    Read at runtime rather than duplicated here, so the pins stay in one place.
    Only the tool itself is installed -- pulling the whole requirements file
    would drag in cffi, which has no wheel for the newest interpreters and
    fails to build from source.
    """
    requirements = (LEGACY / "requirements-dev.txt").read_text(encoding="utf-8")
    match = re.search(rf"^{re.escape(package)}==(\S+)$", requirements, re.MULTILINE)
    return f"{package}=={match.group(1)}" if match else package


class Check:
    def __init__(self, name, group, summary, command, cwd, per_python=False):
        self.name = name
        self.group = group
        self.summary = summary
        self.command = command
        self.cwd = cwd
        # True => runs once per entry in PYTHON_VERSIONS; `command` is a
        # callable taking the version.
        self.per_python = per_python


def _lint(tool: str, *args: str) -> list[str]:
    return [
        "uv", "run", "--quiet", "--no-project", "-p", LINT_PY,
        "--with", pinned(tool), tool, *args,
    ]


def _mypy() -> list[str]:
    """mypy needs the package's runtime imports resolvable, or it reports
    `import-not-found` for them rather than checking the code.

    `importlib-metadata` is not a runtime dependency -- deadcode/__init__.py
    only falls back to it when the stdlib `importlib.metadata` is missing --
    but mypy analyses both branches of that try/except, so it must be present
    to type-check.
    """
    return [
        "uv", "run", "--quiet", "--no-project", "-p", LINT_PY,
        "--with", pinned("mypy"),
        "--with-requirements", "requirements.txt",
        "--with", "importlib-metadata",
        "mypy", "deadcode",
    ]


def _pytest(version: str) -> list[str]:
    # Project mode (no --no-project) so the package is installed:
    # deadcode/__init__.py resolves __version__ from installed metadata and
    # raises on import without it.
    return [
        "uv", "run", "--quiet", "-p", version,
        "--with", "pytest", "--with", "pytest-cov",
        "pytest", "-q",
    ]


CHECKS = [
    Check("fmt", "rust", "cargo fmt --check", ["cargo", "fmt", "--check"], ROOT),
    Check("clippy", "rust", "cargo clippy (warnings denied)",
          ["cargo", "clippy", "--all-targets", "--", "-D", "warnings"], ROOT),
    Check("test", "rust", "cargo test", ["cargo", "test"], ROOT),
    Check("ruff", "python", "ruff lint (legacy-python)",
          lambda: _lint("ruff", "check", "deadcode", "tests"), LEGACY),
    Check("ruff-format", "python", "ruff format --check (legacy-python)",
          lambda: _lint("ruff", "format", "--check", "deadcode", "tests"), LEGACY),
    Check("mypy", "python", "mypy (legacy-python)", _mypy, LEGACY),
    Check("privata", "python", "module privacy (legacy-python)",
          ["uvx", "privata", "."], LEGACY),
    Check("pytest", "python", "legacy test suite, per Python version",
          _pytest, LEGACY, per_python=True),
]

BY_NAME = {c.name: c for c in CHECKS}
GROUPS = sorted({c.group for c in CHECKS})


def resolve(selectors):
    if not selectors:
        return list(CHECKS)
    chosen = []
    for selector in selectors:
        if selector in GROUPS:
            chosen += [c for c in CHECKS if c.group == selector]
        elif selector in BY_NAME:
            chosen.append(BY_NAME[selector])
        else:
            sys.exit(f"unknown check {selector!r}. Known: {', '.join([*GROUPS, *BY_NAME])}")
    return [c for c in CHECKS if c in chosen]


def run_one(label, command, cwd):
    if shutil.which(command[0]) is None:
        # A missing toolchain must never look like success.
        print(f"  FAIL  {label}: {command[0]!r} not found on PATH")
        return False

    print(f"\n=== {label}")
    print(f"    $ {' '.join(command)}   (in {cwd.name or '.'})")
    started = time.monotonic()
    completed = subprocess.run(command, cwd=cwd, check=False)
    elapsed = time.monotonic() - started
    if completed.returncode == 0:
        print(f"    PASS ({elapsed:.1f}s)")
        return True
    print(f"    FAIL ({elapsed:.1f}s) exit={completed.returncode}")
    return False


def main() -> int:
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    parser.add_argument("checks", nargs="*", help="check or group names; default all")
    parser.add_argument(
        "--python", metavar="VER",
        help=f"restrict per-version checks to one interpreter ({', '.join(PYTHON_VERSIONS)})",
    )
    parser.add_argument("--list", action="store_true", help="list checks and exit")
    args = parser.parse_args()

    if args.list:
        width = max(len(c.name) for c in CHECKS)
        for check in CHECKS:
            suffix = f"  [x{len(PYTHON_VERSIONS)} versions]" if check.per_python else ""
            print(f"  {check.name:<{width}}  [{check.group}]  {check.summary}{suffix}")
        return 0

    if args.python and args.python not in PYTHON_VERSIONS:
        sys.exit(f"unknown Python {args.python!r}. Known: {', '.join(PYTHON_VERSIONS)}")

    results = {}
    for check in resolve(args.checks):
        if check.per_python:
            for version in [args.python] if args.python else PYTHON_VERSIONS:
                label = f"{check.name} (Python {version}): {check.summary}"
                results[f"{check.name} py{version}"] = run_one(
                    label, check.command(version), check.cwd
                )
        else:
            command = check.command() if callable(check.command) else check.command
            results[check.name] = run_one(f"{check.name}: {check.summary}", command, check.cwd)

    print("\n" + "=" * 60)
    for name, ok in results.items():
        print(f"  {'ok  ' if ok else 'FAIL'}  {name}")
    failed = [name for name, ok in results.items() if not ok]
    if failed:
        print(f"\n{len(failed)} failed: {', '.join(failed)}")
        return 1
    print(f"\nall {len(results)} checks passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
