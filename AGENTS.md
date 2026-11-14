# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

**deadcode** is a static analysis CLI tool that finds unused code (variables, functions, classes, imports, methods, etc.) in Python codebases via AST analysis, with optional auto-fix support.

As of the `refactor_to_rust` branch, the implementation is **Rust**, not Python — see `RUST_PORT_PLAN.md` for the full rationale, architecture decisions, and phase-by-phase log of the port. The original Python implementation and its test suite live on under `legacy-python/` for reference; it is not built or shipped anymore. The CLI interface (flags, output format, exit codes) is unchanged from the Python version.

- **Language:** Rust (edition 2021), packaged for PyPI via `maturin` with `bindings = "bin"` — the wheel ships a compiled native binary + console-script shim, no PyO3/Python bindings.
- **Entry point:** `src/main.rs` → `deadcode::cli::print_main()`
- **AST parsing:** `ruff_python_parser`/`ruff_python_ast` (Astral's parser, published on crates.io). This replaced `rustpython-parser`, whose newest release (0.4.0) tops out around Python 3.11 and could not parse 3.12+ source — see `src/actions/parse_abstract_syntax_tree.rs` for the specifics and `RUST_PORT_PLAN.md` for why the original choice was reversed.
- **Minimum Rust:** 1.95 (required by the ruff crates).
- **Config:** `pyproject.toml` at repo root now configures the `maturin` build, not tool behavior (there is no `[tool.deadcode]` section for the Rust CLI itself yet — config merging support could be re-added if needed, but the CLI flags are the primary interface).

## Development Setup

```bash
cargo build --release          # builds target/release/deadcode(.exe)
cargo test                     # unit tests (src/**) + integration tests (tests/**)
cargo clippy --all-targets     # lint
cargo fmt                      # format
```

To build and locally install the PyPI wheel:
```bash
pip install maturin
maturin build --release        # writes target/wheels/deadcode-*.whl
pip install target/wheels/deadcode-*.whl
```

## Architecture

Mirrors the original Python pipeline, module-for-module, with two structural differences: AST traversal is a hand-written exhaustive `match` over `ruff_python_ast`'s `Stmt`/`Expr` enums (no dynamic dispatch, no Python-style `ast.iter_fields` reflection), and `NestedScope` only tracks classes (confirmed behavior-preserving by tracing what actually reads it — see `src/utils/nested_scopes.rs`'s doc comment).

1. **`src/actions/parse_arguments.rs`** — Parses CLI args (`clap`) and merges `pyproject.toml`'s `[tool.deadcode]` (if present) — mirrors `flatten_lists_of_comma_separated_values` + `parse_pyproject_toml`.
2. **`src/actions/find_python_filenames.rs`** — Discovers `.py` files, applying `--exclude`/`--only` filters and tach.toml source-root rules.
3. **`src/visitor/dead_code_visitor.rs`** (`DeadCodeVisitor`) — Core AST traversal: collects all name definitions and usages, computes unused items through the usage graph (step 3a). The biggest module — read its module doc comment before changing anything, especially around `should_ignore_new_definitions` latching and the ignore-flag semantics (`--ignore-definitions` vs `--ignore-definitions-if-inherits-from` vs `--ignore-bodies-if-inherits-from` have subtly different scopes of effect).
   3a. **`src/visitor/usage_graph.rs`** — Port of upstream's definition-anchored usage graph. A bare name usage is owned by the definition whose body contains it (decorators and ignored definitions are roots). Usages resolve through `from x import name` and wildcard exports (`src/visitor/module_exports.rs`). A single reachability pass decides what is alive, so dead code used only by other dead code is reported in one run, and same-named definitions in other modules aren't kept alive by mistake.
4. **`src/visitor/ignore.rs`** — Ignore predicates: test-file/conftest detection, pytest fixture/hook/usefixtures, `typing.override`.
5. **`src/actions/fix_or_show_unused_code.rs`** — If `--fix`/`--dry`, removes unused code from files or renders a unified diff (`src/utils/unified_diff.rs`, a port of `difflib.unified_diff`, so `--dry` output matches upstream byte for byte).

### Key modules

| Path | Purpose |
|---|---|
| `src/constants.rs` | `UnusedCodeType` enum, DC01–DC13 error codes (DC10 deliberately absent) |
| `src/data_types.rs` | `Args` struct (doubles as the `clap` CLI definition), `Part` |
| `src/visitor/code_item.rs` | `CodeItem` — a single unused-code finding |
| `src/actions/parse_abstract_syntax_tree.rs` | The only place source is parsed; pins the parser's `target_version` to the newest Python grammar |
| `src/utils/line_index.rs` | Byte-offset → (line, column) conversion (the parser gives byte ranges, not line/col directly) |
| `src/utils/nested_scopes.rs` | Class inherits-from chain tracking |
| `src/utils/fnmatch.rs` | Rust port of Python's `fnmatch` glob semantics (backslashes count as `/`, as upstream) |
| `src/utils/unified_diff.rs` | Port of `difflib.SequenceMatcher` / `unified_diff` for `--dry` |
| `src/visitor/usage_graph.rs`, `src/visitor/module_exports.rs` | Reachability over bare usages; static `__all__` / public-name exports |
| `src/actions/get_unused_import_parts.rs` | Statement-aware removal parts for unused imports (commas, `;` separators, emptied statements) — port of upstream's `get_unused_import_parts.py` |
| `src/actions/remove_file_parts_from_content.rs` | Byte-level code removal for `--fix` — preserves the Python original's quirks except where they produce invalid Python (see its module doc comment) |
| `src/actions/parse_tach_config.rs` | tach.toml support (source_roots, interfaces/expose, unchecked modules, tach.domain.toml merging) |

### Where ruff's AST differs from CPython's `ast`

The visitor is a port of Python code and is written to match CPython's `ast`
semantics exactly. `ruff_python_ast` is not a 1:1 mirror of `ast`, so a few
places translate deliberately. Each is commented at its site; the ones worth
knowing before editing the visitor:

- **A decorated `def`/`class` node's range starts at its first decorator**, where
  CPython points at the `def`/`class` keyword. `push_definition` corrects for this
  via `definition_keyword_start`. Getting this wrong reports decorated definitions
  on the decorator's line and silently breaks `# noqa` lookup (the comment sits on
  the `def` line).
- **Merged node kinds:** `async def`/`def`, `async for`/`for`, `async with`/`with`
  are one node with an `is_async` flag; `try`/`except*` is one node with `is_star`.
- **Split literals:** CPython's single `ast.Constant` is one variant per literal
  kind (`StringLiteral`, `NumberLiteral`, …).
- **Restructured nodes:** `if`/`elif`/`else` is flattened into `elif_else_clauses`
  rather than nested in `orelse`; `Dict` pairs keys and values in one `items` list;
  `MatchClass` merges `kwd_attrs`/`kwd_patterns` into one `keywords` list.
- **f-strings/t-strings are not plain expression trees.** They are lists of parts
  (to support PEP 701 and implicit concatenation), so interpolations must be walked
  explicitly via `walk_interpolated_element` — this is what makes a name used only
  inside `f"{x}"` count as a usage.
- **`ExprContext` has a fourth `Invalid` variant** with no CPython equivalent, used
  for error-recovery nodes. Ignored.

### Deliberate differences from upstream

The Rust CLI tracks upstream `deadcode` (preserved in `legacy-python/`). It
departs from upstream only where upstream's `--fix`/`--dry` would destroy or
break code. Each case is listed with a reason in
`tests/parity/known_differences.txt` and
`tests/parity/upstream_expected_failures.txt`:

- `--fix` never leaves invalid Python. Cases:
  - annotated/augmented assignments are removed whole;
  - trailing import names lose all their commas;
  - an emptied `class A:  # note` body gets `pass`;
  - a commented-out `# def f():` gets no stray `pass`.
- DC09 keeps reachable code. Cases:
  - an `else:` under `if False:`/`while False:` stays;
  - a redundant `if True:` and constant ternaries are reported but not removed.
- `--dry` never touches the disk. Upstream deletes files it would empty.
- Unparseable files are reported on stderr and fail the run, instead of passing silently.

### Error codes

DC01 unused-variable, DC02 unused-function, DC03 unused-class, DC04 unused-method, DC05 unused-attribute, DC06 unused-name (a variable assigned in a class body), DC07 unused-import, DC08 unused-property, DC09 unreachable-body (constant `if`/`while`/ternary conditions), DC11 empty-file, DC12 commented-out-code, DC13 unreachable-code (after `return`/`raise`/`break`/`continue`). DC10 is deliberately absent.

## Testing

Unit tests live inline in `src/**` (`#[cfg(test)] mod tests`); integration tests live in `tests/*.rs`, using real temp directories (no filesystem mocking — this port always does real I/O) via the shared harness in `tests/common/mod.rs`. `tests/files/*.py` are real fixture files (not copies) whose exact line numbers are asserted against — do not reformat them without checking `tests/deadcode_integration.rs`.

**Parity with upstream:** two CI checks hold the Rust CLI to upstream's behaviour.

`python scripts/check.py upstream` runs legacy-python's own pytest suite against the Rust binary. `scripts/rust_main_plugin.py` swaps upstream's CLI entry points for the binary, so upstream's assertions judge it. The tests expected to fail are listed with reasons in `tests/parity/upstream_expected_failures.txt`.

`python scripts/check.py parity` runs the Rust CLI and `legacy-python/` side by side on `tests/parity/cases/*` and every legacy fixture, comparing findings, exit codes and `--dry` diffs, and checks that Rust `--fix` never leaves an unparseable file. Known differences are listed with reasons in `tests/parity/known_differences.txt`; the check fails on a new difference *and* on a listed one that disappeared, so delete the line when you fix one. `python scripts/parity.py --show <case>` prints a case's differences.

**Run a single test:**
```bash
cargo test --test fix_cli_option
cargo test unused_variable_is_reported
```

Six tests under `tests/aspirational_unimplemented.rs` are `#[ignore]`d on purpose — they document deep type/scope-flow tracking the Python original never implemented either (ported from its `@skip`ped tests). Don't "fix" one into passing without recognizing that's a real feature addition, not a bug fix.

## Legacy Python implementation

`legacy-python/` contains the original Python implementation and its pytest suite, kept for reference during/after the port. It has its own `pyproject.toml`/`Makefile` and is checked by `.github/workflows/check-deadcode-on-python310.yml`. It is not part of the Rust build and is not published.
