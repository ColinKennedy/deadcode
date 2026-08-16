# deadcode (legacy Python implementation)

This directory holds the original **Python** implementation of deadcode, kept
for reference after the port to Rust. It is **not built and not shipped** — the
published `lapsed` wheel contains the Rust binary (see the repository root's
`Cargo.toml` and `RUST_PORT_PLAN.md`).

It is still checked in CI so the reference copy does not rot: `ruff`, `mypy`,
and its own `pytest` suite run against it.

## Why this file exists

`pyproject.toml` here declares `readme = "README.md"`. When the Python
implementation moved into this subdirectory during the port, the repository's
`README.md` stayed at the root — so that reference dangled and **any** build of
this package failed with:

```
OSError: Readme file does not exist: README.md
```

That broke `uv pip install -e .`, which in turn broke `make .venv`, the test
suite (`deadcode/__init__.py` resolves its version through installed package
metadata), and the `check-deadcode-on-python310.yml` workflow. This file
restores it.

## Running the checks

From the repository root, the cross-platform runner covers this directory:

```bash
uv run scripts/check.py python      # ruff, mypy, privata
uv run scripts/check.py             # the above plus the Rust checks
```

Or directly, using the pinned tool versions from `requirements-dev.txt`:

```bash
cd legacy-python
uv run --no-project -p 3.10 --with-requirements requirements-dev.txt ruff check deadcode tests
uv run --no-project -p 3.10 --with-requirements requirements-dev.txt mypy deadcode
```
