import functools
@functools.cache
def f():  # noqa: DC02
    return 1
@functools.cache
def g():  # noqa: DC
    return 1
