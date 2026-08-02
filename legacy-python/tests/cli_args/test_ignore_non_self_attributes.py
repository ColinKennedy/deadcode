from deadcode.cli import main
from deadcode.utils.base_test_case import BaseTestCase


class TestIgnoreNonSelfAttributes(BaseTestCase):
    def test_non_self_attribute_is_reported_by_default(self):
        self.files = {
            'foo.py': b"""
                class SomeObject:
                    pass

                foo = SomeObject()
                foo.bar = "thing"
                """
        }
        unused_names = main(['foo.py', '--no-color'])

        self.assertEqual(
            unused_names,
            'foo.py:5:0: DC05 Attribute `bar` is never used',
        )

    def test_non_self_attribute_is_ignored_with_flag(self):
        self.files = {
            'foo.py': b"""
                class SomeObject:
                    pass

                foo = SomeObject()
                foo.bar = "thing"
                """
        }
        unused_names = main(['foo.py', '--no-color', '--ignore-non-self-attributes'])

        self.assertEqual(unused_names, None)

    def test_self_attribute_is_still_reported_with_flag(self):
        self.files = {
            'foo.py': b"""
                class Widget:
                    def __init__(self):
                        self.baz = 1

                Widget()
                """
        }
        unused_names = main(['foo.py', '--no-color', '--ignore-non-self-attributes'])

        self.assertEqual(
            unused_names,
            'foo.py:3:8: DC05 Attribute `baz` is never used',
        )
