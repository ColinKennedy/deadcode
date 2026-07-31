from pathlib import Path
from textwrap import dedent

from deadcode.actions.parse_tach_config import (
    TachConfig,
    TachIndex,
    TachInterface,
    TachModule,
    _dotted_glob_to_regex,
    _dotted_module_path,
    _match_any_dotted_glob,
    _parse_tach_toml,
    load_tach_index,
)


def write(path: Path, content: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(dedent(content))


class TestSourceRoots:
    def test_defaults_to_tach_toml_directory(self, tmp_path: Path) -> None:
        write(tmp_path / 'tach.toml', '')

        config = _parse_tach_toml(tmp_path / 'tach.toml')

        assert config.source_roots == [tmp_path.resolve()]

    def test_resolved_relative_to_tach_toml_directory(self, tmp_path: Path) -> None:
        write(tmp_path / 'tach.toml', 'source_roots = ["backend"]')

        config = _parse_tach_toml(tmp_path / 'tach.toml')

        assert config.source_roots == [(tmp_path / 'backend').resolve()]


class TestDottedModulePath:
    def test_plain_module(self, tmp_path: Path) -> None:
        source_root = tmp_path / 'src'
        file = source_root / 'pkg' / 'mod.py'

        assert _dotted_module_path(file, [source_root]) == 'pkg.mod'

    def test_init_file_collapses_to_package(self, tmp_path: Path) -> None:
        source_root = tmp_path / 'src'
        file = source_root / 'pkg' / '__init__.py'

        assert _dotted_module_path(file, [source_root]) == 'pkg'

    def test_file_outside_all_source_roots_is_none(self, tmp_path: Path) -> None:
        source_root = tmp_path / 'src'
        file = tmp_path / 'other' / 'mod.py'

        assert _dotted_module_path(file, [source_root]) is None

    def test_longest_source_root_wins(self, tmp_path: Path) -> None:
        file = tmp_path / 'src' / 'pkg' / 'mod.py'

        assert _dotted_module_path(file, [tmp_path, tmp_path / 'src']) == 'pkg.mod'


class TestDottedGlobMatching:
    def test_literal_pattern(self) -> None:
        assert _match_any_dotted_glob(['libs.module'], 'libs.module')
        assert not _match_any_dotted_glob(['libs.module'], 'libs.module2')

    def test_single_star_matches_one_segment(self) -> None:
        assert _match_any_dotted_glob(['libs.*'], 'libs.module')
        assert not _match_any_dotted_glob(['libs.*'], 'libs.module.sub')

    def test_double_star_matches_any_trailing_depth(self) -> None:
        assert _match_any_dotted_glob(['libs.**'], 'libs.module.sub')
        assert _match_any_dotted_glob(['libs.**'], 'libs.module')
        assert not _match_any_dotted_glob(['libs.**'], 'libs')
        assert not _match_any_dotted_glob(['libs.**'], 'other.module')

    def test_dotted_glob_to_regex_is_anchored(self) -> None:
        assert _dotted_glob_to_regex('libs.*').fullmatch('libs.module')
        assert not _dotted_glob_to_regex('libs.*').fullmatch('prefix.libs.module')


class TestInterfaceExposure:
    def test_exposed_name_from_matching_module(self, tmp_path: Path) -> None:
        config = TachConfig(
            source_roots=[tmp_path],
            interfaces=[TachInterface(expose=['get_data'], from_patterns=['core'])],
        )
        index = TachIndex([config])

        assert index.is_exposed(tmp_path / 'core.py', 'get_data')
        assert not index.is_exposed(tmp_path / 'core.py', 'other_name')
        assert not index.is_exposed(tmp_path / 'domain.py', 'get_data')

    def test_interface_without_from_applies_to_every_module(self, tmp_path: Path) -> None:
        config = TachConfig(
            source_roots=[tmp_path],
            interfaces=[TachInterface(expose=['PUBLIC'], from_patterns=None)],
        )
        index = TachIndex([config])

        assert index.is_exposed(tmp_path / 'core.py', 'PUBLIC')
        assert index.is_exposed(tmp_path / 'anything.py', 'PUBLIC')

    def test_matching_any_of_multiple_interfaces_is_sufficient(self, tmp_path: Path) -> None:
        config = TachConfig(
            source_roots=[tmp_path],
            interfaces=[
                TachInterface(expose=['read_data'], from_patterns=['api']),
                TachInterface(expose=['write_data'], from_patterns=['api']),
            ],
        )
        index = TachIndex([config])

        assert index.is_exposed(tmp_path / 'api.py', 'write_data')

    def test_double_star_from_pattern_exposes_nested_subpackage_across_multiple_source_roots(
        self, tmp_path: Path
    ) -> None:
        # Regression test: with two source_roots present (one holding subpackages, the
        # other flat files), a `**` `from` pattern targeting the subpackage root must use
        # dotted-glob matching (like `[[modules]] path` patterns), not raw regex - `**` is
        # invalid regex syntax and used to make the interface silently never match.
        backend = tmp_path / 'backend'
        frontend = tmp_path / 'frontend'
        config = TachConfig(
            source_roots=[backend, frontend],
            interfaces=[TachInterface(expose=['get_data'], from_patterns=['pkg1.**'])],
        )
        index = TachIndex([config])

        assert index.is_exposed(backend / 'pkg1' / 'sub' / 'mod.py', 'get_data')
        assert not index.is_exposed(frontend / 'util.py', 'get_data')

    def test_file_outside_source_roots_is_never_exposed(self, tmp_path: Path) -> None:
        config = TachConfig(
            source_roots=[tmp_path / 'src'],
            interfaces=[TachInterface(expose=['.*'], from_patterns=None)],
        )
        index = TachIndex([config])

        assert not index.is_exposed(tmp_path / 'outside.py', 'anything')


class TestUncheckedModules:
    def test_unchecked_module_matches(self, tmp_path: Path) -> None:
        config = TachConfig(
            source_roots=[tmp_path],
            modules=[TachModule(path_patterns=['parsing'], unchecked=True)],
        )
        index = TachIndex([config])

        assert index.is_unchecked(tmp_path / 'parsing.py')
        assert not index.is_unchecked(tmp_path / 'core.py')


class TestDomainTomlMerging:
    def test_domain_toml_is_discovered_and_merged(self, tmp_path: Path) -> None:
        write(tmp_path / 'proj' / 'tach.toml', 'source_roots = ["src"]')
        write(
            tmp_path / 'proj' / 'src' / 'tach' / 'filesystem' / 'tach.domain.toml',
            """
            [root]
            unchecked = true

            [[modules]]
            path = "service"

            [[modules]]
            path = "//other.absolute"

            [[interfaces]]
            expose = ["service.*"]
            from = [""]
            """,
        )

        config = _parse_tach_toml(tmp_path / 'proj' / 'tach.toml')

        module_patterns = [pattern for module in config.modules for pattern in module.path_patterns]
        assert 'tach.filesystem' in module_patterns
        assert 'tach.filesystem.service' in module_patterns
        assert 'other.absolute' in module_patterns

        root_module = next(m for m in config.modules if m.path_patterns == ['tach.filesystem'])
        assert root_module.unchecked is True

        assert any(
            interface.expose == ['service.*'] and interface.from_patterns == ['tach.filesystem']
            for interface in config.interfaces
        )

    def test_load_tach_index_reports_missing_file(self, tmp_path: Path) -> None:
        index = load_tach_index([str(tmp_path / 'does_not_exist.toml')])

        assert not index.is_exposed(tmp_path / 'anything.py', 'anything')
        assert not index.is_unchecked(tmp_path / 'anything.py')
