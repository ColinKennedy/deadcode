import typing
T = typing.TypeVar("T")
def f():
    global G
    G = 1
f()
