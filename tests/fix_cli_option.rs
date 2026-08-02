//! Port of `tests/cli_args/test_fix.py`. Fixture bodies are reconstructed
//! from the scenario descriptions (this port didn't have byte-exact access
//! to the original file contents) but exercise the same properties: exact
//! removal messages, singular/plural "item(s)", and blank-line-preservation
//! rules around removed blocks.

mod common;
use common::Project;

#[test]
fn unused_class_is_removed_with_singular_message() {
    let p = Project::new();
    p.write(
        "foo.py",
        "class UnusedClass:\n    pass\n\n\nprint(\"Keep the file\")\n",
    );
    let result = p.run(&["foo.py", "--no-color", "--fix"]).unwrap();
    assert!(result.contains("DC03 Class `UnusedClass` is never used"));
    assert!(result.ends_with("Removed 1 unused code item!"));
    assert_eq!(p.read("foo.py"), "print(\"Keep the file\")\n");
}

#[test]
fn unused_function_with_default_args_is_removed() {
    let p = Project::new();
    p.write(
        "foo.py",
        "def foo(bar: str = \"Bar\") -> str:\n    return bar\n\n\nprint(\"Keep the file\")\n",
    );
    let result = p.run(&["foo.py", "--no-color", "--fix"]).unwrap();
    assert!(result.contains("DC02 Function `foo` is never used"));
    assert_eq!(p.read("foo.py"), "print(\"Keep the file\")\n");
}

#[test]
fn function_removal_in_the_middle_preserves_surroundings() {
    let p = Project::new();
    p.write(
        "foo.py",
        "used_before = 1\n\n\ndef unused_function():\n    pass\n\n\nprint(used_before)\n",
    );
    p.run(&["foo.py", "--no-color", "--fix"]).unwrap();
    assert_eq!(
        p.read("foo.py"),
        "used_before = 1\n\n\nprint(used_before)\n"
    );
}

#[test]
fn multiple_items_are_listed_sorted_by_line_with_plural_message() {
    let p = Project::new();
    p.write(
        "foo.py",
        "unused_var = 1\n\n\ndef unused_function():\n    pass\n",
    );
    let result = p.run(&["foo.py", "--no-color", "--fix"]).unwrap();
    let dc01_pos = result.find("DC01").unwrap();
    let dc02_pos = result.find("DC02").unwrap();
    assert!(
        dc01_pos < dc02_pos,
        "DC01 (line 1) should be listed before DC02 (line 4)"
    );
    assert!(result.ends_with("Removed 2 unused code items!"));
}

mod add_pass_for_empty_block {
    use super::*;

    #[test]
    fn empty_class_block_gets_pass() {
        let p = Project::new();
        p.write(
            "foo.py",
            "class Example:\n    def unused_method(self):\n        pass\n\n\nExample()\n",
        );
        p.run(&["foo.py", "--no-color", "--fix"]).unwrap();
        assert_eq!(
            p.read("foo.py"),
            "class Example:\n    pass\n\n\nExample()\n"
        );
    }

    #[test]
    fn empty_function_block_gets_pass() {
        let p = Project::new();
        p.write("foo.py", "def foo():\n    unused_local = 1\n\n\nfoo()\n");
        p.run(&["foo.py", "--no-color", "--fix"]).unwrap();
        assert_eq!(p.read("foo.py"), "def foo():\n    pass\n\n\nfoo()\n");
    }

    #[test]
    fn empty_if_block_gets_pass() {
        let p = Project::new();
        p.write("foo.py", "import sys\n\nif sys.argv[1:]:\n    unused = 1\n");
        p.run(&["foo.py", "--no-color", "--fix"]).unwrap();
        assert_eq!(
            p.read("foo.py"),
            "import sys\n\nif sys.argv[1:]:\n    pass\n"
        );
    }

    #[test]
    fn empty_else_block_gets_pass_if_branch_untouched() {
        let p = Project::new();
        p.write(
            "foo.py",
            "import sys\n\nif sys.argv[1:]:\n    print(\"used\")\nelse:\n    unused = 1\n",
        );
        p.run(&["foo.py", "--no-color", "--fix"]).unwrap();
        assert_eq!(
            p.read("foo.py"),
            "import sys\n\nif sys.argv[1:]:\n    print(\"used\")\nelse:\n    pass\n"
        );
    }

    #[test]
    fn empty_with_block_gets_pass_and_as_binding_removed() {
        let p = Project::new();
        p.write("foo.py", "with open(\"tmp.txt\") as f:\n    unused = 1\n");
        p.run(&["foo.py", "--no-color", "--fix"]).unwrap();
        assert_eq!(p.read("foo.py"), "with open(\"tmp.txt\"):\n    pass\n");
    }
}

mod blank_line_preservation {
    use super::*;

    #[test]
    fn one_blank_line_kept_after_whole_class_removal() {
        let p = Project::new();
        p.write(
            "foo.py",
            "x = 1\n\nclass UnusedClass:\n    pass\n\nprint(x)\n",
        );
        p.run(&["foo.py", "--no-color", "--fix"]).unwrap();
        assert_eq!(p.read("foo.py"), "x = 1\n\nprint(x)\n");
    }

    #[test]
    fn two_blank_lines_kept_after_whole_class_removal() {
        let p = Project::new();
        p.write(
            "foo.py",
            "x = 1\n\n\nclass UnusedClass:\n    pass\n\n\nprint(x)\n",
        );
        p.run(&["foo.py", "--no-color", "--fix"]).unwrap();
        assert_eq!(p.read("foo.py"), "x = 1\n\n\nprint(x)\n");
    }
}
