"""Pytest plugin: run legacy-python's test suite against the Rust binary.

    cargo build
    cd legacy-python
    export DEADCODE_RUST_BINARY=../target/debug/deadcode PYTHONPATH=../scripts
    python -m pytest -p rust_main_plugin -p no:cacheprovider tests

Upstream's end-to-end tests call `deadcode.cli.main` / `print_main` with an
in-memory `virtual_file_system` fixture (or a real `project_directory`). This
plugin replaces both entry points with ones that write the virtual files to a
temporary directory, run the Rust binary there, and report back what it wrote
or deleted, so the tests' own assertions judge the Rust output unchanged.

Tests that call internal Python functions directly keep testing Python; only
the CLI entry points are swapped.
"""

from __future__ import annotations

import os
import re
import subprocess
import tempfile
from pathlib import Path

import deadcode.cli
from deadcode.utils.fix_indent import fix_indent

BINARY = Path(os.environ["DEADCODE_RUST_BINARY"])
LEGACY_ROOT = Path(deadcode.cli.__file__).resolve().parent.parent
ANSI = re.compile(r"\x1b\[[0-9;]*m")
WELL_DONE = "Well done! ✨ \U0001f680 ✨"
# Options that take no value; every other `--option` consumes the next token.
FLAGS = {
    "--fix", "--dry", "--no-color", "--quiet", "--count", "--verbose", "-v",
    "--version", "-V", "--ignore-non-self-attributes", "--ignore-class-attributes",
}


def _virtual_file_system():
    """The active `virtual_file_system` fixture, found through the mock it installs."""
    side_effect = getattr(deadcode.cli.find_python_filenames, "side_effect", None)
    return getattr(side_effect, "__self__", None)


def _run(command_line_args):
    args = list(command_line_args or [])
    vfs = _virtual_file_system()
    if vfs is None:
        completed = _invoke(args, Path.cwd())
        return completed

    with tempfile.TemporaryDirectory() as directory:
        root = Path(directory)
        # Configuration tests write a real pyproject.toml into the working
        # directory next to their virtual files; legacy-python's own (the
        # working directory of every other test) is not part of the test.
        pyproject = Path.cwd() / "pyproject.toml"
        if pyproject.is_file() and Path.cwd() != LEGACY_ROOT:
            (root / "pyproject.toml").write_bytes(pyproject.read_bytes())
        written = {}
        for name, content in vfs.files.items():
            path = root / name.lstrip("/")
            path.parent.mkdir(parents=True, exist_ok=True)
            # The virtual file system's `readlines()` (what upstream diffs
            # and fixes) ends every line, the last included, with a newline.
            content = fix_indent(content) or b""
            if content and not content.endswith(b"\n"):
                content += b"\n"
            written[name] = content
            path.write_bytes(written[name])
        # The virtual file system feeds every file to deadcode regardless of
        # path arguments, so analyse the whole directory instead: the first
        # path argument becomes `.`, and the rest are dropped.
        rewritten = []
        takes_value = False
        for arg in args:
            if takes_value:
                rewritten.append(arg)
                takes_value = False
            elif arg.startswith("-"):
                rewritten.append(arg)
                takes_value = "=" not in arg and arg not in FLAGS
            elif "." not in rewritten:
                rewritten.append(".")
        completed = _invoke(rewritten, root)
        for name, before in written.items():
            path = root / name.lstrip("/")
            if not path.exists():
                vfs.os_remove(name)
            elif (after := path.read_bytes()) != before:
                vfs.updated_files[name] = after
    return completed


def _invoke(args, cwd):
    completed = subprocess.run(
        [str(BINARY), *args], cwd=cwd, capture_output=True, encoding="utf-8", check=False
    )
    # `println!` adds the one newline upstream's returned message lacks.
    output = completed.stdout.removesuffix("\n")
    if "--no-color" in args:
        output = ANSI.sub("", output)
    return output, completed.returncode


def main(command_line_args=None):
    output, status = _run(command_line_args)
    if status == 0 and output in ("", WELL_DONE):
        return None
    return output


def print_main(command_line_args=None):
    output, status = _run(command_line_args)
    if output:
        print(output)
    return status


def pytest_configure(config):
    deadcode.cli.main = main
    deadcode.cli.print_main = print_main
