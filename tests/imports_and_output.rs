//! Port of `tests/test_imports.py` and `tests/test_output.py`.

mod common;
use common::{strip_ansi, Project};

mod imports {
    use super::*;

    #[test]
    fn relative_module_import_usage_is_tracked() {
        let p = Project::new();
        p.write("eggs/foo.py", "used_var = None\n");
        p.write(
            "eggs/spam/bar.py",
            "from .. import foo as f\nprint(f.used_var)\n",
        );
        let eggs = p.dir.path().join("eggs");
        assert_eq!(p.run(&[eggs.to_str().unwrap(), "--no-color"]), None);
    }

    #[test]
    fn recovers_after_a_syntax_error_in_a_previous_run() {
        let p = Project::new();
        p.write("foo.py", "unused_var = None this is syntax error");
        // The unparseable run fails (`Some("")` = exit non-zero, nothing more
        // on stdout) rather than reporting a clean pass...
        assert_eq!(p.run(&["foo.py", "--no-color"]), Some(String::new()));

        // ...and once the syntax is valid, analysis resumes normally.
        p.write("foo.py", "unused_var = None\n");
        let result = p.run(&["foo.py", "--no-color"]).unwrap();
        assert!(result.contains("DC01 Variable `unused_var`"));
    }

    #[test]
    fn variable_names_in_comments_are_not_usage() {
        let p = Project::new();
        p.write(
            "foo.py",
            "# unused_var mentioned in a comment\nunused_var = 1\n",
        );
        let result = p.run(&["foo.py", "--no-color"]).unwrap();
        assert!(result.contains(":2:0: DC01 Variable `unused_var`"));
    }

    #[test]
    fn variable_names_in_strings_are_not_usage() {
        let p = Project::new();
        p.write(
            "foo.py",
            "\"\"\"unused_var docstring mention\"\"\"\nx = 1\nprint(\"unused_var string mention\")\nunused_var = 1\n",
        );
        let result = p.run(&["foo.py", "--no-color"]).unwrap();
        assert!(result.contains(":4:0: DC01 Variable `unused_var`"));
    }
}

mod output {
    use super::*;

    #[test]
    fn count_option_counts_all_unused_definitions() {
        let p = Project::new();
        p.write(
            "foo.py",
            "class Foo:\n    bar = None\n    spam = None\n\n    def eggs(self):\n        return None\n\n\nclass Bar(Foo):\n    pass\n",
        );
        let result = p.run(&["foo.py", "--count"]).unwrap();
        assert_eq!(result, "4");
    }

    #[test]
    fn quiet_option_suppresses_all_text_output() {
        let p = Project::new();
        p.write(
            "foo.py",
            "class Foo:\n    bar = None\n    spam = None\n\n    def eggs(self):\n        return None\n",
        );
        let result = p.run(&["foo.py", "--quiet"]).unwrap();
        assert_eq!(result, "");
    }

    #[test]
    fn colorful_output_by_default() {
        let p = Project::new();
        p.write("foo.py", "unused_var = 1\n");
        let result = p.run(&["foo.py"]).unwrap();
        assert!(result.contains("\x1b[91mDC01\x1b[0m"));
        assert!(result.contains("`\x1b[1munused_var\x1b[0m`"));
    }

    #[test]
    fn no_color_option_strips_ansi() {
        let p = Project::new();
        p.write("foo.py", "unused_var = 1\n");
        let colorful = p.run(&["foo.py"]).unwrap();
        let plain = p.run(&["foo.py", "--no-color"]).unwrap();
        assert_eq!(strip_ansi(&colorful), plain);
        assert!(!plain.contains('\x1b'));
    }
}
