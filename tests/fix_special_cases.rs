//! Port of `tests/fix/test_empty_files.py`, `tests/fix/test_unreachable_code.py`,
//! and `tests/fix/test_unused_imports.py`.
//!
//! The last of those documented a "dangling empty import stub" defect that the
//! port originally reproduced for parity. It is now fixed — emitting invalid
//! Python from `--fix` is not a quirk worth preserving — so the test below
//! asserts the statement is deleted outright. See
//! `remove_file_parts_from_content.rs`'s module doc comment.

mod common;
use common::Project;

mod empty_files {
    use super::*;

    #[test]
    fn whitespace_only_file_is_removed_as_dc11() {
        let p = Project::new();
        p.write("foo.py", "   \n\n  \n");
        let result = p.run(&["foo.py", "--no-color", "--fix"]).unwrap();
        assert!(result.contains("DC11 Empty file"));
        assert!(result.ends_with("Removed 1 unused code item!"));
        assert!(!p.exists("foo.py"));
    }

    #[test]
    fn whitespace_only_file_in_subdir_is_removed() {
        let p = Project::new();
        p.write("bar/foo.py", "   \n");
        let result = p.run(&["bar/foo.py", "--no-color", "--fix"]).unwrap();
        assert!(result.contains("DC11 Empty file"));
        assert!(!p.exists("bar/foo.py"));
    }
}

mod unreachable_literal_condition {
    use super::*;

    #[test]
    fn true_if_removes_else_body_and_keeps_a_valid_block() {
        let p = Project::new();
        let src = "if True:\n    print(\"a\")\nelse:\n    print(\"b\")\n";
        p.write("foo.py", src);
        let result = p.run(&["foo.py", "--no-color", "--fix"]).unwrap();
        assert!(result.contains("DC09 Unreachable `else` block"));
        assert_eq!(
            p.read("foo.py"),
            "if True:\n    print(\"a\")\nelse:\n    pass\n"
        );
    }

    #[test]
    fn false_if_without_else_removes_the_full_statement() {
        let p = Project::new();
        p.write("foo.py", "if False:\n    print(\"a\")\nprint(\"b\")\n");
        let result = p.run(&["foo.py", "--no-color", "--fix"]).unwrap();
        assert!(result.contains("foo.py:2:4: DC09 Unsatisfiable `if` condition"));
        assert_eq!(p.read("foo.py"), "print(\"b\")\n");
    }

    /// Upstream removes the whole statement here, `else:` included -- deleting
    /// code that always runs.
    #[test]
    fn false_if_keeps_its_reachable_else() {
        let p = Project::new();
        let src = "if False:\n    print(\"a\")\nelse:\n    print(\"b\")\n";
        p.write("foo.py", src);
        p.run(&["foo.py", "--no-color", "--fix"]).unwrap();
        assert_eq!(
            p.read("foo.py"),
            "if False:\n    pass\nelse:\n    print(\"b\")\n"
        );
    }

    #[test]
    fn true_if_removes_the_whole_elif_chain() {
        let p = Project::new();
        let src = "import sys\nif True:\n    print(\"c\")\nelif sys.argv:\n    print(\"d\")\n    print(\"e\")\nelse:\n    print(\"f\")\n";
        p.write("foo.py", src);
        let result = p.run(&["foo.py", "--no-color", "--fix"]).unwrap();
        assert!(result.contains("foo.py:4:0: DC09 Unreachable `else` block"));
        assert_eq!(p.read("foo.py"), "import sys\nif True:\n    print(\"c\")\n");
    }

    #[test]
    fn numeric_string_and_container_constants_are_evaluated() {
        let p = Project::new();
        p.write(
            "foo.py",
            "while 0:\n    print(1)\nif \"\":\n    print(2)\nif []:\n    print(3)\nif (1,):\n    print(4)\n",
        );
        let result = p.run(&["foo.py", "--no-color"]).unwrap();
        assert!(result.contains("foo.py:2:4: DC09 Unsatisfiable `while` condition"));
        assert!(result.contains("foo.py:4:4: DC09 Unsatisfiable `if` condition"));
        assert!(result.contains("foo.py:6:4: DC09 Unsatisfiable `if` condition"));
        assert!(result.contains("foo.py:7:0: DC09 Redundant `if` condition"));
    }

    /// Upstream removes a redundant `if True:` body that always runs, and
    /// cuts ternary operands out leaving invalid code.
    #[test]
    fn redundant_if_and_ternaries_are_reported_but_not_removed() {
        let p = Project::new();
        let src =
            "if True:\n    print(1)\nx = 1 if False else 2\ny = 1 if True else 2\nprint(x, y)\n";
        p.write("foo.py", src);
        let result = p.run(&["foo.py", "--no-color", "--fix"]).unwrap();
        assert!(result.contains("foo.py:1:0: DC09 Redundant `if` condition"));
        assert!(result.contains("foo.py:3:4: DC09 Unsatisfiable `ternary` condition"));
        assert!(result.contains("foo.py:4:19: DC09 Unreachable `else` expression"));
        assert_eq!(p.read("foo.py"), src);
    }

    #[test]
    fn true_while_removes_else_body_and_keeps_a_valid_block() {
        let p = Project::new();
        let src = "while True:\n    print(\"a\")\n    break\nelse:\n    print(\"b\")\n";
        p.write("foo.py", src);
        let result = p.run(&["foo.py", "--no-color", "--fix"]).unwrap();
        assert!(result.contains("DC09 Unreachable `else` block"));
        assert_eq!(
            p.read("foo.py"),
            "while True:\n    print(\"a\")\n    break\nelse:\n    pass\n"
        );
    }

    #[test]
    fn false_while_keeps_its_reachable_else() {
        let p = Project::new();
        let src = "while False:\n    print(\"a\")\nelse:\n    print(\"b\")\n";
        p.write("foo.py", src);
        let result = p.run(&["foo.py", "--no-color", "--fix"]).unwrap();
        assert!(result.contains("DC09 Unsatisfiable `while` condition"));
        assert_eq!(
            p.read("foo.py"),
            "while False:\n    pass\nelse:\n    print(\"b\")\n"
        );
    }

    #[test]
    fn boolean_expression_with_an_unknown_still_reports_a_provable_outcome() {
        let p = Project::new();
        p.write("foo.py", "if unknown and False:\n    print(\"never\")\n");
        let result = p.run(&["foo.py", "--no-color"]).unwrap();
        assert!(result.contains("DC09 Unsatisfiable `if` condition"));
    }
}

mod unreachable_code_after_terminal_statement {
    use super::*;

    #[test]
    fn reports_and_removes_the_remaining_statement_list_suffix() {
        let p = Project::new();
        p.write(
            "foo.py",
            "def used():\n    return 1\n    print(\"unreachable\")\n\nused()\n",
        );
        let result = p.run(&["foo.py", "--no-color", "--fix"]).unwrap();
        assert!(result.contains("DC13 unreachable code after 'return'"));
        assert_eq!(p.read("foo.py"), "def used():\n    return 1\nused()\n");
    }

    #[test]
    fn noqa_suppresses_the_finding() {
        let p = Project::new();
        p.write(
            "foo.py",
            "def used():\n    return 1\n    print(\"unreachable\")  # noqa: DC13\n\nused()\n",
        );
        assert_eq!(p.run(&["foo.py", "--no-color"]), None);
    }
}

mod commented_out_code {
    use super::*;

    #[test]
    fn reports_and_removes_a_standalone_code_comment() {
        let p = Project::new();
        p.write("foo.py", "# print(\"old\")\nprint(\"live\")\n");
        let result = p.run(&["foo.py", "--no-color", "--fix"]).unwrap();
        assert!(result.contains("DC12 Commented out code"));
        assert_eq!(p.read("foo.py"), "print(\"live\")\n");
    }

    #[test]
    fn removes_an_inline_comment_but_keeps_its_code() {
        let p = Project::new();
        p.write("foo.py", "value = 1  # print(\"old\")\nprint(value)\n");
        let result = p.run(&["foo.py", "--no-color", "--fix"]).unwrap();
        assert!(result.contains("DC12 Commented out code"));
        assert_eq!(p.read("foo.py"), "value = 1\nprint(value)\n");
    }

    #[test]
    fn ignores_type_directives_and_noqa() {
        let p = Project::new();
        p.write(
            "foo.py",
            "# type: ignore\n# print(\"old\")  # noqa: DC12\nprint(\"live\")\n",
        );
        assert_eq!(p.run(&["foo.py", "--no-color"]), None);
    }
}

/// Regression: removing every name from a parenthesized multi-import
/// statement must delete the whole statement, not leave `from file1 import (`
/// / `)` behind. The Python original left the stub (`# TODO: empty imports
/// statements should be removed as well.`), which produces a file that no
/// longer parses.
#[test]
fn emptied_import_statement_is_removed_entirely() {
    let p = Project::new();
    p.write(
        "file1.py",
        "def foo():\n    pass\n\n\ndef bar():\n    pass\n\n\ndef xyz():\n    pass\n\n\ndef used():\n    pass\n",
    );
    p.write(
        "file2.py",
        "from file1 import (\n    foo,\n    bar,\n    xyz,\n)\nfrom file1 import used\n\nused()\n",
    );
    let file1 = p.path_str("file1.py");
    let file2 = p.path_str("file2.py");
    let result = p
        .run(&[&file1, &file2, "--no-color", "--fix", "-v"])
        .unwrap();
    assert!(result.contains("DC07 Import `foo`"));
    assert!(result.contains("DC07 Import `bar`"));
    assert!(result.contains("DC07 Import `xyz`"));

    let updated = p.read("file2.py");
    // The `used` import and its call survive untouched...
    assert!(updated.contains("from file1 import used"));
    assert!(updated.contains("used()"));
    // ...and the emptied statement is gone in full — no opening `from ... (`,
    // no orphaned closing paren, no bare `import`.
    assert!(
        !updated.contains("from file1 import ("),
        "dangling open stub left behind:\n{updated}"
    );
    for line in updated.lines() {
        assert_ne!(line.trim(), ")", "orphaned closing paren:\n{updated}");
        assert_ne!(
            line.trim(),
            "import",
            "bare `import` left behind:\n{updated}"
        );
    }
}

/// The single-line forms of the same defect. Each of these previously left
/// behind a stub (`import `, `from foo import `) that is not valid Python.
#[test]
fn emptied_single_line_imports_are_removed_entirely() {
    for source in [
        "import os\nprint(1)\n",
        "from foo import bar\nprint(1)\n",
        "import os.path\nprint(1)\n",
        "import os as o\nprint(1)\n",
        "from . import thing\nprint(1)\n",
    ] {
        let p = Project::new();
        p.write("mod.py", source);
        p.run(&["mod.py", "--no-color", "--fix"]);
        let updated = p.read("mod.py");
        assert_eq!(
            updated, "print(1)\n",
            "expected the import statement to be removed outright, from: {source:?}"
        );
    }
}

/// Every unused name on one line must be removed, not just the first.
/// Previously `import os, sys` (both unused) removed only `os`.
#[test]
fn all_unused_names_on_one_line_are_removed() {
    let p = Project::new();
    p.write("mod.py", "import os, sys\nprint(1)\n");
    p.run(&["mod.py", "--no-color", "--fix"]);
    assert_eq!(p.read("mod.py"), "print(1)\n");

    // ...while a name that IS used on that same line survives.
    let p2 = Project::new();
    p2.write("mod.py", "import os, sys\nprint(sys.path)\n");
    p2.run(&["mod.py", "--no-color", "--fix"]);
    assert_eq!(p2.read("mod.py"), "import sys\nprint(sys.path)\n");
}

/// `--fix` must leave valid Python. Each case below produced a syntax error
/// before the upstream fixes were ported (or, for the last few, in upstream
/// too).
mod fix_keeps_valid_python {
    use super::*;

    fn fix(src: &str) -> String {
        let p = Project::new();
        p.write("foo.py", src);
        p.run(&["foo.py", "--no-color", "--fix"]).unwrap();
        p.read("foo.py")
    }

    #[test]
    fn multi_line_assignment_is_removed_whole() {
        let fixed = fix("unused = [\n    1,\n    2,\n]\nused = 1\nprint(used)\n");
        assert_eq!(fixed, "used = 1\nprint(used)\n");
    }

    #[test]
    fn unused_names_leave_a_valid_parenthesized_import() {
        let fixed = fix("from os import (\n    path,\n    sep,\n    getcwd,\n)\nprint(sep)\n");
        assert_eq!(fixed, "from os import (\n    sep\n)\nprint(sep)\n");
    }

    #[test]
    fn semicolon_separated_import_is_removed_with_its_comment() {
        let fixed = fix("import sys, json; import re  # comment\nprint(json)\n");
        assert_eq!(fixed, "import json\nprint(json)\n");
    }

    #[test]
    fn annotated_and_augmented_assignments_are_removed_whole() {
        let fixed = fix("class A:\n    count: int\n    label: str = \"x\"\n\n\nA\n");
        assert_eq!(fixed, "class A:\n    pass\n\n\nA\n");

        let fixed = fix("def f():\n    total = 0\n    total += 1\n    return 1\n\n\nf()\n");
        assert_eq!(fixed, "def f():\n    return 1\n\n\nf()\n");
    }

    #[test]
    fn emptied_class_body_before_a_removed_top_level_class_gets_an_indented_pass() {
        let src = "class Kept:\n    def unused(self):\n        pass\n\n\nclass Unused:\n    pass\n\n\nKept\n";
        let fixed = fix(src);
        assert_eq!(fixed, "class Kept:\n    pass\n\n\nKept\n");
    }
}

mod dry_run_never_touches_disk {
    use super::*;

    #[test]
    fn a_file_dry_would_empty_is_shown_not_deleted() {
        let p = Project::new();
        p.write("foo.py", "def unused():\n    pass\n");
        let result = p.run(&["foo.py", "--no-color", "--dry"]).unwrap();
        assert!(result.contains("-def unused():"));
        assert!(p.exists("foo.py"));
    }
}
