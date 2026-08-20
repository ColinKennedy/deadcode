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
3. **`src/visitor/dead_code_visitor.rs`** (`DeadCodeVisitor`) — Core AST traversal: collects all name definitions and usages, computes unused items. The biggest module — read its module doc comment before changing anything, especially around `should_ignore_new_definitions` latching and the ignore-flag semantics (`--ignore-definitions` vs `--ignore-definitions-if-inherits-from` vs `--ignore-bodies-if-inherits-from` have subtly different scopes of effect).
4. **`src/visitor/ignore.rs`** — Ignore predicates: test-file/conftest detection, pytest fixture/hook/usefixtures, `typing.override`.
5. **`src/actions/fix_or_show_unused_code.rs`** — If `--fix`/`--dry`, removes unused code from files or renders a unified diff (`similar` crate).

### Key modules

| Path | Purpose |
|---|---|
| `src/constants.rs` | `UnusedCodeType` enum, DC01–DC13 error codes (DC10 deliberately absent) |
| `src/data_types.rs` | `Args` struct (doubles as the `clap` CLI definition), `Part` |
| `src/visitor/code_item.rs` | `CodeItem` — a single unused-code finding |
| `src/actions/parse_abstract_syntax_tree.rs` | The only place source is parsed; pins the parser's `target_version` to the newest Python grammar |
| `src/utils/line_index.rs` | Byte-offset → (line, column) conversion (the parser gives byte ranges, not line/col directly) |
| `src/utils/nested_scopes.rs` | Class inherits-from chain tracking |
| `src/utils/fnmatch.rs` | Rust port of Python's `fnmatch` glob semantics |
| `src/actions/remove_file_parts_from_content.rs` | Byte-level code removal for `--fix` — preserves several documented quirks/bugs from the Python original for exact parity (see its module doc comment) |
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

### Deliberately NOT ported (confirmed dead/inert in the Python original)

- `unreachable_code`/DC09 detection: computed in the Python code but explicitly excluded from `get_unused_code_items()`'s output and untested — zero observable effect, so it isn't implemented here at all.
- The `--ignore-*-if-decorated-with` flags: parsed (so `--help` matches) but functionally inert, matching a confirmed no-op in the Python original (`ignore_decorators` hardcoded to `[]`).

### Error codes

DC01 unused-variable, DC02 unused-function, DC03 unused-class, DC04 unused-method, DC05 unused-attribute, DC06 unused-name, DC07 unused-import, DC08 unused-property, DC09 unreachable-if-block (not implemented, see above), DC11 empty-file, DC12 commented-out-code (not implemented), DC13 unreachable-code (not implemented).

## Testing

Unit tests live inline in `src/**` (`#[cfg(test)] mod tests`); integration tests live in `tests/*.rs`, using real temp directories (no filesystem mocking — this port always does real I/O) via the shared harness in `tests/common/mod.rs`. `tests/files/*.py` are real fixture files (not copies) whose exact line numbers are asserted against — do not reformat them without checking `tests/deadcode_integration.rs`.

**Run a single test:**
```bash
cargo test --test fix_cli_option
cargo test unused_variable_is_reported
```

Six tests under `tests/aspirational_unimplemented.rs` are `#[ignore]`d on purpose — they document deep type/scope-flow tracking the Python original never implemented either (ported from its `@skip`ped tests). Don't "fix" one into passing without recognizing that's a real feature addition, not a bug fix.

## Legacy Python implementation

`legacy-python/` contains the original Python implementation and its pytest suite, kept for reference during/after the port. It has its own `pyproject.toml`/`Makefile` and is checked by `.github/workflows/check-deadcode-on-python310.yml`. It is not part of the Rust build and is not published.
