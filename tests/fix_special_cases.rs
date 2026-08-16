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

/// Negative-parity: literal `if True`/`if False`/`while True`/`while False`
/// branches are NOT pruned/fixed — this port deliberately doesn't implement
/// unreachable-code removal at all (see `dead_code_visitor.rs`'s module doc
/// comment: the Python original computes it but never surfaces it either).
mod unreachable_code_is_not_removed {
    use super::*;

    // Bodies deliberately define nothing (just literal-arg `print` calls) so
    // the only thing under test is "does a DC09 finding ever leak into the
    // report" — no unrelated DC01 confounds the `result == None` assertion.

    #[test]
    fn if_true_else_unchanged() {
        let p = Project::new();
        let src = "if True:\n    print(\"a\")\nelse:\n    print(\"b\")\n";
        p.write("foo.py", src);
        assert_eq!(p.run(&["foo.py", "--no-color", "--fix"]), None);
        assert_eq!(p.read("foo.py"), src);
    }

    #[test]
    fn if_false_else_unchanged() {
        let p = Project::new();
        let src = "if False:\n    print(\"a\")\nelse:\n    print(\"b\")\n";
        p.write("foo.py", src);
        assert_eq!(p.run(&["foo.py", "--no-color", "--fix"]), None);
        assert_eq!(p.read("foo.py"), src);
    }

    #[test]
    fn while_true_else_unchanged() {
        let p = Project::new();
        let src = "while True:\n    print(\"a\")\n    break\nelse:\n    print(\"b\")\n";
        p.write("foo.py", src);
        assert_eq!(p.run(&["foo.py", "--no-color", "--fix"]), None);
        assert_eq!(p.read("foo.py"), src);
    }

    #[test]
    fn while_false_else_unchanged() {
        let p = Project::new();
        let src = "while False:\n    print(\"a\")\nelse:\n    print(\"b\")\n";
        p.write("foo.py", src);
        assert_eq!(p.run(&["foo.py", "--no-color", "--fix"]), None);
        assert_eq!(p.read("foo.py"), src);
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
