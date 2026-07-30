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
