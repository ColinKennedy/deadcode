from typing import TYPE_CHECKING
if TYPE_CHECKING:
    from collections import OrderedDict
def f(x: "OrderedDict[str, int]") -> "list[OrderedDict]":
    return []
f(1)
