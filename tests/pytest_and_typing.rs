//! Port of `tests/test_pytest_fixtures.py`, `tests/test_pytest_hooks.py`,
//! `tests/test_typing_override.py`. Covers the sub-cases not already
//! exercised by `src/visitor/dead_code_visitor.rs`'s unit tests.

mod common;
use common::Project;

mod pytest_fixtures {
    use super::*;

    #[test]
    fn autouse_call_form_fixture_in_conftest_is_never_flagged() {
        let p = Project::new();
        p.write(
            "conftest.py",
            "import pytest\n\n@pytest.fixture(autouse=True)\ndef db():\n    return {}\n",
        );
        assert_eq!(p.run(&["conftest.py", "--no-color"]), None);
    }

    #[test]
    fn fixture_in_nested_conftest_is_never_flagged() {
        let p = Project::new();
        p.write(
            "tests/integration/conftest.py",
            "import pytest\n\n@pytest.fixture\ndef db():\n    return {}\n",
        );
        assert_eq!(
            p.run(&["tests/integration/conftest.py", "--no-color"]),
            None
        );
    }

    #[test]
    fn fixture_in_test_file_is_never_flagged() {
        let p = Project::new();
        p.write(
            "tests/test_foo.py",
            "import pytest\n\n@pytest.fixture\ndef db():\n    return {}\n",
        );
        assert_eq!(p.run(&["tests/test_foo.py", "--no-color"]), None);
    }

    #[test]
    fn fixture_method_on_test_class_is_never_flagged() {
        let p = Project::new();
        p.write(
            "tests/test_foo.py",
            "import pytest\n\nclass TestSomething:\n    @pytest.fixture\n    def db(self):\n        return {}\n",
        );
        assert_eq!(p.run(&["tests/test_foo.py", "--no-color"]), None);
    }

    #[test]
    fn bare_fixture_import_is_recognized() {
        let p = Project::new();
        p.write(
            "conftest.py",
            "from pytest import fixture\n\n@fixture\ndef db():\n    return {}\n",
        );
        assert_eq!(p.run(&["conftest.py", "--no-color"]), None);
    }

    #[test]
    fn pytest_asyncio_fixture_is_recognized() {
        let p = Project::new();
        p.write(
            "conftest.py",
            "import pytest_asyncio\n\n@pytest_asyncio.fixture\nasync def db():\n    return {}\n",
        );
        assert_eq!(p.run(&["conftest.py", "--no-color"]), None);
    }

    #[test]
    fn fixture_outside_pytest_location_is_still_flagged() {
        let p = Project::new();
        p.write(
            "myapp/helpers.py",
            "import pytest\n\n@pytest.fixture\ndef db():\n    return {}\n",
        );
        let result = p.run(&["myapp/helpers.py", "--no-color"]).unwrap();
        assert!(result.contains("DC02"));
        assert!(result.contains("`db`"));
    }

    #[test]
    fn usefixtures_mark_on_class_counts_as_usage() {
        let p = Project::new();
        p.write(
            "myapp/helpers.py",
            "import pytest\n\n@pytest.fixture\ndef db():\n    return {}\n",
        );
        p.write(
            "tests/test_foo.py",
            "import pytest\n\n@pytest.mark.usefixtures(\"db\")\nclass TestSomething:\n    def test_it(self):\n        pass\n",
        );
        let result = p.run(&["myapp/helpers.py", "tests/test_foo.py", "--no-color"]);
        assert_eq!(result, None);
    }

    #[test]
    fn getfixturevalue_counts_as_usage() {
        let p = Project::new();
        p.write(
            "myapp/helpers.py",
            "import pytest\n\n@pytest.fixture\ndef db():\n    return {}\n",
        );
        p.write(
            "tests/test_foo.py",
            "def test_it(request):\n    request.getfixturevalue(\"db\")\n",
        );
        let result = p.run(&["myapp/helpers.py", "tests/test_foo.py", "--no-color"]);
        assert_eq!(result, None);
    }
}

mod pytest_hooks {
    use super::*;

    #[test]
    fn pytest_configure_hook_in_conftest_is_never_flagged() {
        let p = Project::new();
        p.write("conftest.py", "def pytest_configure(config):\n    pass\n");
        assert_eq!(p.run(&["conftest.py", "--no-color"]), None);
    }

    #[test]
    fn pytest_collection_modifyitems_hook_in_conftest_is_never_flagged() {
        let p = Project::new();
        p.write(
            "conftest.py",
            "def pytest_collection_modifyitems(config):\n    pass\n",
        );
        assert_eq!(p.run(&["conftest.py", "--no-color"]), None);
    }

    #[test]
    fn pytest_hook_in_nested_conftest_is_never_flagged() {
        let p = Project::new();
        p.write(
            "tests/integration/conftest.py",
            "def pytest_addoption(parser):\n    pass\n",
        );
        assert_eq!(
            p.run(&["tests/integration/conftest.py", "--no-color"]),
            None
        );
    }

    #[test]
    fn pytest_prefixed_function_outside_conftest_is_still_flagged() {
        let p = Project::new();
        p.write(
            "myapp/helpers.py",
            "def pytest_configure(config):\n    pass\n",
        );
        let result = p.run(&["myapp/helpers.py", "--no-color"]).unwrap();
        assert!(result.contains("DC02"));
        assert!(result.contains("pytest_configure"));
    }
}

mod typing_override {
    use super::*;

    #[test]
    fn method_decorated_with_typing_override_is_never_flagged() {
        let p = Project::new();
        p.write(
            "myapp/foo.py",
            "import typing\n\nclass Foo(Base):\n    @typing.override\n    def method(self):\n        pass\n\nFoo()\n",
        );
        assert_eq!(p.run(&["myapp/foo.py", "--no-color"]), None);
    }

    #[test]
    fn bare_override_import_is_recognized() {
        let p = Project::new();
        p.write(
            "myapp/foo.py",
            "from typing import override\n\nclass Foo(Base):\n    @override\n    def method(self):\n        pass\n\nFoo()\n",
        );
        assert_eq!(p.run(&["myapp/foo.py", "--no-color"]), None);
    }

    #[test]
    fn typing_extensions_override_is_recognized() {
        let p = Project::new();
        p.write(
            "myapp/foo.py",
            "import typing_extensions\n\nclass Foo(Base):\n    @typing_extensions.override\n    def method(self):\n        pass\n\nFoo()\n",
        );
        assert_eq!(p.run(&["myapp/foo.py", "--no-color"]), None);
    }

    #[test]
    fn method_without_override_decorator_is_still_flagged() {
        let p = Project::new();
        p.write(
            "myapp/foo.py",
            "class Foo(Base):\n    def method(self):\n        pass\n\nFoo()\n",
        );
        let result = p.run(&["myapp/foo.py", "--no-color"]).unwrap();
        assert!(result.contains("DC04 Method `method`"));
    }
}
