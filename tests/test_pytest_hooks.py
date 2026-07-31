from deadcode.cli import main
from deadcode.utils.base_test_case import BaseTestCase


class TestPytestHooks(BaseTestCase):
    def test_pytest_configure_hook_in_conftest_is_never_flagged(self):
        self.files = {
            'conftest.py': b"""
                def pytest_configure(config):
                    pass
                """
        }
        unused_names = main(['conftest.py', '--no-color'])
        self.assertIsNone(unused_names)

    def test_pytest_collection_modifyitems_hook_in_conftest_is_never_flagged(self):
        self.files = {
            'conftest.py': b"""
                def pytest_collection_modifyitems(config, items):
                    pass
                """
        }
        unused_names = main(['conftest.py', '--no-color'])
        self.assertIsNone(unused_names)

    def test_pytest_hook_in_nested_conftest_is_never_flagged(self):
        self.files = {
            'tests/integration/conftest.py': b"""
                def pytest_addoption(parser):
                    pass
                """
        }
        unused_names = main(['tests/integration/conftest.py', '--no-color'])
        self.assertIsNone(unused_names)

    def test_pytest_prefixed_function_outside_conftest_is_still_flagged(self):
        self.files = {
            'myapp/helpers.py': b"""
                def pytest_configure(config):
                    pass
                """
        }
        unused_names = main(['myapp/helpers.py', '--no-color'])
        self.assertEqual(
            unused_names,
            'myapp/helpers.py:1:0: DC02 Function `pytest_configure` is never used',
        )
