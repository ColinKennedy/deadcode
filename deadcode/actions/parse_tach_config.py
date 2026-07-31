import re
import sys
from dataclasses import dataclass, field
from logging import getLogger
from pathlib import Path
from typing import Any, Dict, Iterable, List, Optional

if sys.version_info >= (3, 11):
    import tomllib
else:
    import tomli as tomllib

logger = getLogger()


@dataclass
class TachInterface:
    expose: List[str]
    from_patterns: Optional[List[str]] = None


@dataclass
class TachModule:
    path_patterns: List[str]
    unchecked: bool = False


@dataclass
class TachConfig:
    source_roots: List[Path]
    project_root: Optional[Path] = None
    modules: List[TachModule] = field(default_factory=list)
    interfaces: List[TachInterface] = field(default_factory=list)


class TachIndex:
    """Answers dead-code-relevant questions derived from one or more tach.toml files."""

    def __init__(self, configs: List[TachConfig]) -> None:
        self._configs = configs

    def is_outside_source_roots(self, path: Path) -> bool:
        """True if `path` belongs to a tach project (is its project root or nested under it)

        but falls outside every one of that project's `source_roots`, and outside them for
        every other tach project it might also belong to. Paths unrelated to any tach project
        (e.g. an explicitly provided directory that lives elsewhere entirely) are unaffected.
        """
        path = path.resolve()

        related_to_any_config = False
        for config in self._configs:
            if config.project_root is None:
                continue
            relation = _relation_to_project_root(path, config.project_root)
            if relation == 'unrelated':
                continue
            related_to_any_config = True
            if relation == 'ancestor':
                # `path` is still above the project root, so it must be walked into to reach it.
                return False
            if any(_within_or_towards_source_root(path, source_root) for source_root in config.source_roots):
                return False

        return related_to_any_config

    def is_unchecked(self, file: Path) -> bool:
        for config in self._configs:
            module_path = _dotted_module_path(file, config.source_roots)
            if module_path is None:
                continue
            for module in config.modules:
                if module.unchecked and _match_any_dotted_glob(module.path_patterns, module_path):
                    return True
        return False

    def is_exposed(self, file: Path, name: str) -> bool:
        for config in self._configs:
            module_path = _dotted_module_path(file, config.source_roots)
            if module_path is None:
                continue
            for interface in config.interfaces:
                adopts_interface = interface.from_patterns is None or _match_any_regex(
                    interface.from_patterns, module_path
                )
                if adopts_interface and _match_any_regex(interface.expose, name):
                    return True
        return False


def load_tach_index(tach_config_paths: Iterable[str]) -> TachIndex:
    configs = []
    for raw_path in tach_config_paths:
        path = Path(raw_path).resolve()
        if not path.is_file():
            logger.error(f'Error: tach config {path} could not be found.')
            continue
        configs.append(_parse_tach_toml(path))
    return TachIndex(configs)


def _parse_tach_toml(path: Path) -> TachConfig:
    with open(path, 'rb') as f:
        data = tomllib.load(f)

    project_root = path.parent.resolve()
    raw_source_roots = data.get('source_roots') or ['.']
    source_roots = [(project_root / root).resolve() for root in raw_source_roots]

    config = TachConfig(source_roots=source_roots, project_root=project_root)
    config.modules.extend(_extract_modules(data))
    config.interfaces.extend(_extract_interfaces(data))

    for source_root in source_roots:
        if not source_root.is_dir():
            continue
        for domain_file in sorted(source_root.rglob('tach.domain.toml')):
            _merge_domain_toml(config, domain_file, source_root)

    return config


def _extract_modules(data: Dict[str, Any]) -> List[TachModule]:
    modules = []
    for raw in data.get('modules', []):
        if 'paths' in raw:
            patterns = list(raw['paths'])
        elif 'path' in raw:
            patterns = [raw['path']]
        else:
            continue
        modules.append(TachModule(path_patterns=patterns, unchecked=bool(raw.get('unchecked', False))))
    return modules


def _extract_interfaces(data: Dict[str, Any]) -> List[TachInterface]:
    interfaces = []
    for raw in data.get('interfaces', []):
        expose = list(raw.get('expose', []))
        if not expose:
            continue
        from_patterns = list(raw['from']) if 'from' in raw else None
        interfaces.append(TachInterface(expose=expose, from_patterns=from_patterns))
    return interfaces


def _merge_domain_toml(config: TachConfig, domain_file: Path, source_root: Path) -> None:
    with open(domain_file, 'rb') as f:
        data = tomllib.load(f)

    domain_root_dotted = _dotted_dir_path(domain_file.parent, source_root)

    def resolve(entry: str) -> str:
        if entry.startswith('//'):
            return entry[2:]
        if entry == '':
            return domain_root_dotted
        if domain_root_dotted:
            return f'{domain_root_dotted}.{entry}'
        return entry

    modules = _extract_modules(data)
    for module in modules:
        module.path_patterns = [resolve(pattern) for pattern in module.path_patterns]
    config.modules.extend(modules)

    interfaces = _extract_interfaces(data)
    for interface in interfaces:
        if interface.from_patterns is not None:
            interface.from_patterns = [resolve(pattern) for pattern in interface.from_patterns]
    config.interfaces.extend(interfaces)

    root_table = data.get('root')
    if isinstance(root_table, dict) and root_table.get('unchecked'):
        config.modules.append(TachModule(path_patterns=[domain_root_dotted], unchecked=True))


def _dotted_dir_path(directory: Path, source_root: Path) -> str:
    directory = directory.resolve()
    source_root = source_root.resolve()
    if directory == source_root:
        return ''
    return '.'.join(directory.relative_to(source_root).parts)


def _dotted_module_path(file: Path, source_roots: List[Path]) -> Optional[str]:
    file = file.resolve()

    best_root: Optional[Path] = None
    for source_root in source_roots:
        source_root = source_root.resolve()
        if source_root in file.parents and (best_root is None or len(source_root.parts) > len(best_root.parts)):
            best_root = source_root

    if best_root is None:
        return None

    parts = list(file.relative_to(best_root).parts)
    if parts and parts[-1] == '__init__.py':
        parts = parts[:-1]
    elif parts and parts[-1].endswith('.py'):
        parts[-1] = parts[-1][: -len('.py')]
    else:
        return None

    return '.'.join(parts) if parts else None


def _relation_to_project_root(path: Path, project_root: Path) -> str:
    if path == project_root or project_root in path.parents:
        return 'inside'
    if path in project_root.parents:
        return 'ancestor'
    return 'unrelated'


def _within_or_towards_source_root(path: Path, source_root: Path) -> bool:
    return path == source_root or source_root in path.parents or path in source_root.parents


def _dotted_glob_to_regex(pattern: str) -> re.Pattern[str]:
    regex_segments = [
        '.*' if segment == '**' else re.escape(segment).replace(r'\*', '[^.]*') for segment in pattern.split('.')
    ]
    return re.compile(r'\.'.join(regex_segments))


def _match_any_dotted_glob(patterns: Iterable[str], dotted_path: str) -> bool:
    return any(_dotted_glob_to_regex(pattern).fullmatch(dotted_path) for pattern in patterns)


def _try_compile_regex(pattern: str) -> Optional[re.Pattern[str]]:
    try:
        return re.compile(pattern)
    except re.error:
        logger.exception(f'Error: invalid tach interface regex pattern {pattern!r}.')
        return None


def _match_any_regex(patterns: Iterable[str], value: str) -> bool:
    return any(
        compiled.fullmatch(value)
        for pattern in patterns
        if (compiled := _try_compile_regex(pattern)) is not None
    )
