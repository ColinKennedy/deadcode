//! Port of `tests/fix/test_empty_files.py`, `tests/fix/test_unreachable_code.py`,
//! and `tests/fix/test_unused_imports.py` (the last documents a known-buggy
//! "dangling empty import stub" behavior that must be reproduced, not fixed —
//! see `remove_file_parts_from_content.rs`'s module doc comment).

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

/// Documents a known-buggy behavior in the Python original (`# TODO: empty
/// imports statements should be removed as well.`): removing every name from
/// a parenthesized multi-import statement leaves a dangling empty stub
/// instead of deleting the whole statement. Reproduced here, not fixed.
#[test]
fn unused_imports_leave_dangling_empty_stub() {
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
    // The `used` import/call survive; the fully-emptied parenthesized
    // import leaves a dangling stub rather than being deleted outright.
    assert!(updated.contains("used()"));
    assert!(updated.contains("from file1 import ("));
}
