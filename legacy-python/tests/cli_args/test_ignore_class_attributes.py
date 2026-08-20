from deadcode.cli import main
from deadcode.utils.base_test_case import BaseTestCase


class TestIgnoreClassAttributes(BaseTestCase):
    def test_class_attribute_is_reported_by_default(self):
        self.files = {
            'foo.py': b"""
                class Bar:
                    pass

                class Foo(Bar):
                    THING = "blah"

                Foo()
                """
        }
        unused_names = main(['foo.py', '--no-color'])

        self.assertEqual(
            unused_names,
            'foo.py:5:4: DC01 Variable `THING` is never used',
        )

    def test_class_attribute_is_ignored_with_flag(self):
        self.files = {
            'foo.py': b"""
                class Bar:
                    pass

                class Foo(Bar):
                    THING = "blah"

                Foo()
                """
        }
        unused_names = main(['foo.py', '--no-color', '--ignore-class-attributes'])

        self.assertEqual(unused_names, None)

    def test_nested_class_attribute_is_ignored_with_flag(self):
        self.files = {
            'foo.py': b"""
                class Foo:
                    class Meta:
                        ordering = ["-created"]

                Foo.Meta
                """
        }
        unused_names = main(['foo.py', '--no-color', '--ignore-class-attributes'])

        self.assertEqual(unused_names, None)

    def test_local_variable_is_still_reported_with_flag(self):
        self.files = {
            'foo.py': b"""
                class Foo:
                    THING = "blah"

                    def method(self):
                        local_unused = 1
                        return 2

                Foo()
                """
        }
        unused_names = main(['foo.py', '--no-color', '--ignore-class-attributes'])

        self.assertEqual(
            unused_names,
            'foo.py:4:4: DC04 Method `method` is never used\n' 'foo.py:5:8: DC01 Variable `local_unused` is never used',
        )
