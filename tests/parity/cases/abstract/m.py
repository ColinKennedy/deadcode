from abc import ABC, abstractmethod
from typing import override
class Base(ABC):
    @abstractmethod
    def run(self): ...
class Child(Base):
    @override
    def run(self):
        return 1
Child()
