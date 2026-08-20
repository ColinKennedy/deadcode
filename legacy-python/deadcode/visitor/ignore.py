import ast
from fnmatch import fnmatch, fnmatchcase
from functools import lru_cache
from pathlib import Path
from typing import Iterable, Set, Union

from deadcode.visitor.code_item import CodeItem


IGNORED_VARIABLE_NAMES = {'object', 'self'}
_PYTEST_FUNCTION_NAMES = {
    'setup_module',
    'teardown_module',
    'setup_function',
    'teardown_function',
}
_PYTEST_METHOD_NAMES = {
    'setup_class',
    'teardown_class',
    'setup_method',
    'teardown_method',
}
_PYTEST_FIXTURE_DECORATOR_NAMES = {
    '@pytest.fixture',
    '@pytest_asyncio.fixture',
    '@fixture',
}
PYTEST_USEFIXTURES_DECORATOR_NAMES = {
    '@pytest.mark.usefixtures',
    '@mark.usefixtures',
    '@usefixtures',
}
_OVERRIDE_DECORATOR_NAMES = {
    '@typing.override',
    '@typing_extensions.override',
    '@override',
}

ERROR_CODES = {
    'variable': b'DC01',
    'function': b'DC02',
    'class': b'DC03',
    'method': b'DC04',
    'attribute': b'DC05',
    'name': b'DC06',
    'import': b'DC07',
    'property': b'DC08',
    'unreachable_code': b'DC09',
    'empty_file': b'DC11',
    'commented_out_code': b'DC12',
    'ignore_expression': b'DC13',
}


def get_unused_items(defined_items: Iterable[CodeItem], used_names: Set[str]) -> Iterable[CodeItem]:
    unused_items = [item for item in defined_items if item.name not in used_names]
    unused_items.sort(key=lambda item: item.name.lower())
    return unused_items


def _is_special_name(name: str) -> bool:
    return name.startswith('__') and name.endswith('__')


def match(name: Union[str, Path], patterns: Iterable[str], case: bool = True) -> bool:
    func = fnmatchcase if case else fnmatch
    # .as_posix() (not str()) for Path names, so a pattern like '*/tests/*'
    # matches consistently regardless of the OS's native path separator.
    name_str = name.as_posix() if isinstance(name, Path) else name
    return any(func(name_str, pattern) for pattern in patterns)


def match_many(names: Union[Iterable[str], Iterable[Path]], patterns: Iterable[str], case: bool = True) -> bool:
    return any(match(name, patterns, case) for name in names)


@lru_cache(maxsize=None)
def _is_test_file(filename: Path) -> bool:
    # Called once per definition (function/class/method) in a file, so cache
    # per-filename: `filename.resolve()` is a filesystem syscall and the
    # answer never changes for repeated calls with the same file.
    return match(
        filename.resolve(),
        ['*/test/*', '*/tests/*', '*/test*.py', '*[-_]test.py'],
        case=False,
    )


def _is_conftest_file(filename: Path) -> bool:
    return filename.name == 'conftest.py'


def assigns_special_variable__all__(node: ast.Assign) -> bool:
    assert isinstance(node, ast.Assign)
    return isinstance(node.value, (ast.List, ast.Tuple)) and any(
        target.id == '__all__' for target in node.targets if isinstance(target, ast.Name)
    )


def ignore_class(filename: Path, class_name: str) -> bool:
    return _is_test_file(filename) and 'Test' in class_name


def ignore_import(filename: Path, import_name: str) -> bool:
    """
    Ignore star-imported names since we can't detect whether they are used.
    Ignore imports from __init__.py files since they're commonly used to
    collect objects from a package.
    """
    return filename.name == '__init__.py' or import_name == '*'


def ignore_function(filename: Path, function_name: str) -> bool:
    return (
        (function_name in _PYTEST_FUNCTION_NAMES or function_name.startswith('test_')) and _is_test_file(filename)
    ) or _ignore_pytest_hook(filename, function_name)


def _ignore_pytest_hook(filename: Path, function_name: str) -> bool:
    """
    `pytest_*` functions in conftest.py (e.g. `pytest_configure`,
    `pytest_collection_modifyitems`, `pytest_addoption`) are hook
    implementations that pytest calls automatically by name, never directly,
    so they would otherwise look unused.
    """
    return _is_conftest_file(filename) and function_name.startswith('pytest_')


def ignore_pytest_fixture(filename: Path, decorator_names: Iterable[str]) -> bool:
    """
    Pytest fixtures are consumed by name-matching (as a test's parameter, via
    `autouse=True`, `@pytest.mark.usefixtures`, or `request.getfixturevalue()`),
    so they're never called directly and would otherwise look unused.

    Trust any `@pytest.fixture`/`@pytest_asyncio.fixture` defined in conftest.py
    (auto-discovered project-wide by pytest) or in a recognized test file.
    Fixtures defined elsewhere must be exempted explicitly, e.g. via
    --ignore-names-if-decorated-with.
    """
    return (_is_conftest_file(filename) or _is_test_file(filename)) and match_many(
        decorator_names, _PYTEST_FIXTURE_DECORATOR_NAMES
    )


def ignore_override(decorator_names: Iterable[str]) -> bool:
    """
    `@typing.override` (and `@typing_extensions.override`) marks a method as
    overriding one declared on a base class. The base class's method is what
    calling code actually calls, so this override is used even though nothing
    calls it by its own name directly.
    """
    return match_many(decorator_names, _OVERRIDE_DECORATOR_NAMES)


def ignore_method(filename: Path, method_name: str) -> bool:
    return _is_special_name(method_name) or (
        (method_name in _PYTEST_METHOD_NAMES or method_name.startswith('test_')) and _is_test_file(filename)
    )


def is_self_attribute(node: ast.Attribute) -> bool:
    """Whether an attribute assignment target is `self.attr` (as opposed to
    e.g. `foo.attr`, where `foo` is some other object)."""
    return isinstance(node.value, ast.Name) and node.value.id == 'self'


def ignore_variable(filename: Path, varname: str) -> bool:
    """
    Ignore _ (Python idiom), _x (pylint convention) and
    __x__ (special variable or method), but not __x.
    """
    return (
        varname in IGNORED_VARIABLE_NAMES
        or (varname.startswith('_') and not varname.startswith('__'))
        or _is_special_name(varname)
    )
