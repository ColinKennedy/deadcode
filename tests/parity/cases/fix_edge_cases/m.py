from os.path import (
    join,
    basename,
)
from collections import OrderedDict as OD, deque as dq  # trailing comment
import json; import re


class Holder:
    count: int
    label: str = "x"

    def unused_method(self):
        return 1


class Empty:
    def only_method(self):
        pass


def main():
    total = 0
    total += 1
    data = {
        "a": 1,
        "b": 2,
    }
    print(join, dq)


main()
Holder, Empty
