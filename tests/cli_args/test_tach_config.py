from pathlib import Path
from textwrap import dedent

from deadcode.cli import main


def write(path: Path, content: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(dedent(content))


def test_name_exposed_via_tach_interface_is_not_reported(tmp_path: Path) -> None:
    write(
        tmp_path / 'tach.toml',
        """
        source_roots = ["."]

        [[interfaces]]
        expose = ["get_data"]
        from = ["core"]
        """,
    )
    write(
        tmp_path / 'core.py',
        """
        def get_data():
            pass
        """,
    )

    result = main(
        [
            str(tmp_path / 'core.py'),
            '--no-color',
            '--tach-config',
            str(tmp_path / 'tach.toml'),
        ]
    )

    assert result is None


def test_name_not_covered_by_any_interface_is_still_reported(tmp_path: Path) -> None:
    write(
        tmp_path / 'tach.toml',
        """
        source_roots = ["."]

        [[interfaces]]
        expose = ["get_data"]
        from = ["core"]
        """,
    )
    write(
        tmp_path / 'core.py',
        """
        def get_data():
            pass
        """,
    )
    write(
        tmp_path / 'domain.py',
        """
        def helper():
            pass
        """,
    )

    result = main(
        [
            str(tmp_path / 'core.py'),
            str(tmp_path / 'domain.py'),
            '--no-color',
            '--tach-config',
            str(tmp_path / 'tach.toml'),
        ]
    )

    assert result is not None
    assert 'get_data' not in result
    assert 'helper' in result
    assert 'DC02' in result


def test_file_under_unchecked_module_is_skipped_entirely(tmp_path: Path) -> None:
    write(
        tmp_path / 'tach.toml',
        """
        source_roots = ["."]

        [[modules]]
        path = "legacy"
        unchecked = true
        """,
    )
    write(
        tmp_path / 'legacy.py',
        """
        def unused_in_legacy():
            pass
        """,
    )
    write(
        tmp_path / 'core.py',
        """
        def unused_in_core():
            pass
        """,
    )

    result = main(
        [
            str(tmp_path / 'legacy.py'),
            str(tmp_path / 'core.py'),
            '--no-color',
            '--tach-config',
            str(tmp_path / 'tach.toml'),
        ]
    )

    assert result is not None
    assert 'unused_in_legacy' not in result
    assert 'unused_in_core' in result


def test_name_exposed_via_literal_dotted_from_pattern_in_one_of_several_source_roots_is_not_reported(
    tmp_path: Path,
) -> None:
    # Regression test: `from = ["some.subpackage.module"]` is a plain literal dotted path
    # (no glob wildcard). "backend" and "frontend" are both listed as source_roots, but the
    # "some.subpackage.module" module only exists under "backend".
    write(
        tmp_path / 'tach.toml',
        """
        source_roots = ["backend", "frontend"]

        [[interfaces]]
        expose = ["some_function"]
        from = ["some.subpackage.module"]
        """,
    )
    write(
        tmp_path / 'backend' / 'some' / '__init__.py',
        '',
    )
    write(
        tmp_path / 'backend' / 'some' / 'subpackage' / '__init__.py',
        '',
    )
    write(
        tmp_path / 'backend' / 'some' / 'subpackage' / 'module.py',
        """
        def some_function():
            pass
        """,
    )
    write(
        tmp_path / 'frontend' / 'util.py',
        """
        def helper():
            pass
        """,
    )

    result = main(
        [
            str(tmp_path / 'backend' / 'some' / 'subpackage' / 'module.py'),
            str(tmp_path / 'frontend' / 'util.py'),
            '--no-color',
            '--tach-config',
            str(tmp_path / 'tach.toml'),
        ]
    )

    assert result is not None
    assert 'some_function' not in result
    assert 'helper' in result
    assert 'DC02' in result


def test_name_exposed_via_double_star_from_pattern_in_one_of_several_source_roots_is_not_reported(
    tmp_path: Path,
) -> None:
    # Regression test: `source_roots` lists two directories - "backend", organized into
    # subpackages, and "frontend", holding flat files. Only "backend" is covered by an
    # `[[interfaces]]` declaration, using a `**` pattern (the same "any depth" glob syntax
    # supported by `[[modules]] path`) to expose a name nested two levels deep. That pattern
    # used to be compiled as a raw regex, where `**` is invalid ("multiple repeat"), so the
    # interface silently never matched and the exposed name was reported as dead code.
    write(
        tmp_path / 'tach.toml',
        """
        source_roots = ["backend", "frontend"]

        [[interfaces]]
        expose = ["get_data"]
        from = ["pkg1.**"]
        """,
    )
    write(
        tmp_path / 'backend' / 'pkg1' / '__init__.py',
        '',
    )
    write(
        tmp_path / 'backend' / 'pkg1' / 'sub' / '__init__.py',
        '',
    )
    write(
        tmp_path / 'backend' / 'pkg1' / 'sub' / 'mod.py',
        """
        def get_data():
            pass
        """,
    )
    write(
        tmp_path / 'frontend' / 'util.py',
        """
        def helper():
            pass
        """,
    )

    result = main(
        [
            str(tmp_path / 'backend' / 'pkg1' / 'sub' / 'mod.py'),
            str(tmp_path / 'frontend' / 'util.py'),
            '--no-color',
            '--tach-config',
            str(tmp_path / 'tach.toml'),
        ]
    )

    assert result is not None
    assert 'get_data' not in result
    assert 'helper' in result
    assert 'DC02' in result


def test_file_next_to_source_root_is_not_scanned(tmp_path: Path) -> None:
    # Regression test: only directories listed in source_roots should be scanned when
    # --tach-config is given. A file living alongside a source root (but not inside it)
    # was being scanned anyway.
    write(
        tmp_path / 'tach.toml',
        """
        source_roots = ["src"]
        """,
    )
    write(
        tmp_path / 'src' / 'core.py',
        """
        def unused_in_core():
            pass
        """,
    )
    write(
        tmp_path / 'package.py',
        """
        def unused_outside_source_roots():
            pass
        """,
    )

    result = main(
        [
            str(tmp_path),
            '--no-color',
            '--tach-config',
            str(tmp_path / 'tach.toml'),
        ]
    )

    assert result is not None
    assert 'unused_in_core' in result
    assert 'unused_outside_source_roots' not in result


def test_explicit_directory_outside_tach_project_is_still_scanned(tmp_path: Path) -> None:
    # A directory explicitly passed on the command line that has nothing to do with the
    # tach project (not the project root or nested within it) is scanned regardless of
    # source_roots.
    write(
        tmp_path / 'project' / 'tach.toml',
        """
        source_roots = ["src"]
        """,
    )
    write(
        tmp_path / 'project' / 'src' / 'core.py',
        """
        def unused_in_core():
            pass
        """,
    )
    write(
        tmp_path / 'some_other_directory' / 'mod.py',
        """
        def unused_elsewhere():
            pass
        """,
    )

    result = main(
        [
            str(tmp_path / 'project' / 'src'),
            str(tmp_path / 'some_other_directory'),
            '--no-color',
            '--tach-config',
            str(tmp_path / 'project' / 'tach.toml'),
        ]
    )

    assert result is not None
    assert 'unused_in_core' in result
    assert 'unused_elsewhere' in result


def test_multiple_tach_config_values_are_merged(tmp_path: Path) -> None:
    write(
        tmp_path / 'a' / 'tach.toml',
        """
        source_roots = ["."]

        [[interfaces]]
        expose = ["a_public"]
        from = ["module_a"]
        """,
    )
    write(
        tmp_path / 'a' / 'module_a.py',
        """
        def a_public():
            pass
        """,
    )
    write(
        tmp_path / 'b' / 'tach.toml',
        """
        source_roots = ["."]

        [[interfaces]]
        expose = ["b_public"]
        from = ["module_b"]
        """,
    )
    write(
        tmp_path / 'b' / 'module_b.py',
        """
        def b_public():
            pass
        """,
    )

    result = main(
        [
            str(tmp_path / 'a' / 'module_a.py'),
            str(tmp_path / 'b' / 'module_b.py'),
            '--no-color',
            '--tach-config',
            str(tmp_path / 'a' / 'tach.toml'),
            '--tach-config',
            str(tmp_path / 'b' / 'tach.toml'),
        ]
    )

    assert result is None
