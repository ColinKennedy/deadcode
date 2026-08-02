from deadcode.cli import main
from deadcode.utils.base_test_case import BaseTestCase


class TestPytestFixtures(BaseTestCase):
    def test_fixture_in_conftest_is_never_flagged(self):
        self.files = {
            'conftest.py': b"""
                import pytest

                @pytest.fixture
                def db():
                    return {}
                """
        }
        unused_names = main(['conftest.py', '--no-color'])
        self.assertIsNone(unused_names)

    def test_autouse_fixture_in_conftest_is_never_flagged(self):
        self.files = {
            'conftest.py': b"""
                import pytest

                @pytest.fixture(autouse=True)
                def db():
                    return {}
                """
        }
        unused_names = main(['conftest.py', '--no-color'])
        self.assertIsNone(unused_names)

    def test_fixture_in_nested_conftest_is_never_flagged(self):
        self.files = {
            'tests/integration/conftest.py': b"""
                import pytest

                @pytest.fixture
                def db():
                    return {}
                """
        }
        unused_names = main(['tests/integration/conftest.py', '--no-color'])
        self.assertIsNone(unused_names)

    def test_fixture_in_test_file_is_never_flagged(self):
        self.files = {
            'tests/test_foo.py': b"""
                import pytest

                @pytest.fixture
                def db():
                    return {}
                """
        }
        unused_names = main(['tests/test_foo.py', '--no-color'])
        self.assertIsNone(unused_names)

    def test_fixture_method_on_test_class_is_never_flagged(self):
        self.files = {
            'tests/test_foo.py': b"""
                import pytest

                class TestFoo:
                    @pytest.fixture
                    def db(self):
                        return {}
                """
        }
        unused_names = main(['tests/test_foo.py', '--no-color'])
        self.assertIsNone(unused_names)

    def test_bare_fixture_import_is_recognized(self):
        self.files = {
            'conftest.py': b"""
                from pytest import fixture

                @fixture
                def db():
                    return {}
                """
        }
        unused_names = main(['conftest.py', '--no-color'])
        self.assertIsNone(unused_names)

    def test_pytest_asyncio_fixture_is_recognized(self):
        self.files = {
            'conftest.py': b"""
                import pytest_asyncio

                @pytest_asyncio.fixture
                async def db():
                    return {}
                """
        }
        unused_names = main(['conftest.py', '--no-color'])
        self.assertIsNone(unused_names)

    def test_fixture_outside_pytest_location_is_still_flagged(self):
        self.files = {
            'myapp/helpers.py': b"""
                import pytest

                @pytest.fixture
                def db():
                    return {}
                """
        }
        unused_names = main(['myapp/helpers.py', '--no-color'])
        self.assertEqual(
            unused_names,
            'myapp/helpers.py:4:0: DC02 Function `db` is never used',
        )

    def test_usefixtures_mark_on_function_counts_as_usage(self):
        # `db` is defined outside a recognized pytest location, so it is only
        # spared by the explicit `usefixtures` reference from another file.
        self.files = {
            'myapp/helpers.py': b"""
                import pytest

                @pytest.fixture
                def db():
                    return {}
                """,
            'tests/test_foo.py': b"""
                import pytest

                @pytest.mark.usefixtures("db")
                def test_something():
                    pass
                """,
        }
        unused_names = main(['myapp/helpers.py', 'tests/test_foo.py', '--no-color'])
        self.assertIsNone(unused_names)

    def test_usefixtures_mark_on_class_counts_as_usage(self):
        self.files = {
            'myapp/helpers.py': b"""
                import pytest

                @pytest.fixture
                def db():
                    return {}
                """,
            'tests/test_foo.py': b"""
                import pytest

                @pytest.mark.usefixtures("db")
                class TestSomething:
                    def test_it(self):
                        pass
                """,
        }
        unused_names = main(['myapp/helpers.py', 'tests/test_foo.py', '--no-color'])
        self.assertIsNone(unused_names)

    def test_getfixturevalue_counts_as_usage(self):
        self.files = {
            'myapp/helpers.py': b"""
                import pytest

                @pytest.fixture
                def db():
                    return {}
                """,
            'tests/test_foo.py': b"""
                def test_something(request):
                    request.getfixturevalue("db")
                """,
        }
        unused_names = main(['myapp/helpers.py', 'tests/test_foo.py', '--no-color'])
        self.assertIsNone(unused_names)
