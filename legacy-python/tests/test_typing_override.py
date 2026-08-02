from deadcode.cli import main
from deadcode.utils.base_test_case import BaseTestCase


class TestTypingOverride(BaseTestCase):
    def test_method_decorated_with_typing_override_is_never_flagged(self):
        self.files = {
            'myapp/foo.py': b"""
                import typing


                class Foo(Base):
                    @typing.override
                    def method(self):
                        pass


                Foo()
                """
        }
        unused_names = main(['myapp/foo.py', '--no-color'])
        self.assertIsNone(unused_names)

    def test_method_decorated_with_bare_override_is_never_flagged(self):
        self.files = {
            'myapp/foo.py': b"""
                from typing import override


                class Foo(Base):
                    @override
                    def method(self):
                        pass


                Foo()
                """
        }
        unused_names = main(['myapp/foo.py', '--no-color'])
        self.assertIsNone(unused_names)

    def test_method_decorated_with_typing_extensions_override_is_never_flagged(self):
        self.files = {
            'myapp/foo.py': b"""
                import typing_extensions


                class Foo(Base):
                    @typing_extensions.override
                    def method(self):
                        pass


                Foo()
                """
        }
        unused_names = main(['myapp/foo.py', '--no-color'])
        self.assertIsNone(unused_names)

    def test_method_without_override_decorator_is_still_flagged(self):
        self.files = {
            'myapp/foo.py': b"""
                class Foo(Base):
                    def method(self):
                        pass


                Foo()
                """
        }
        unused_names = main(['myapp/foo.py', '--no-color'])
        self.assertEqual(
            unused_names,
            'myapp/foo.py:2:4: DC04 Method `method` is never used',
        )
