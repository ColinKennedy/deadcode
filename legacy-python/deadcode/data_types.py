import ast

from dataclasses import dataclass
from typing import Iterable, NamedTuple


AbstractSyntaxTree = ast.Module  # Should be module instead of ast
FileContent = bytes
Filename = str  # Contains full path to existing file
_Pathname = str  # Can contain wildewards


@dataclass
class Args:
    fix: bool = False
    verbose: bool = False
    version: bool = False
    dry: bool = False
    only: Iterable[_Pathname] = ()
    paths: Iterable[_Pathname] = ()
    exclude: Iterable[_Pathname] = ()
    ignore_definitions: Iterable[_Pathname] = ()
    ignore_definitions_if_decorated_with: Iterable[_Pathname] = ()
    ignore_definitions_if_inherits_from: Iterable[_Pathname] = ()
    ignore_bodies_of: Iterable[_Pathname] = ()
    ignore_bodies_if_decorated_with: Iterable[_Pathname] = ()
    ignore_bodies_if_inherits_from: Iterable[_Pathname] = ()
    ignore_if_decorated_with: Iterable[_Pathname] = ()
    ignore_if_inherits_from: Iterable[_Pathname] = ()
    ignore_names: Iterable[_Pathname] = ()
    ignore_names_if_decorated_with: Iterable[_Pathname] = ()
    ignore_names_if_inherits_from: Iterable[_Pathname] = ()
    ignore_names_in_files: Iterable[_Pathname] = ()
    ignore_non_self_attributes: bool = False
    ignore_class_attributes: bool = False
    tach_config: Iterable[_Pathname] = ()
    no_color: bool = False
    quiet: bool = False
    count: bool = False


class Part(NamedTuple):
    """Code file part"""

    line_start: int
    line_end: int
    col_start: int
    col_end: int
