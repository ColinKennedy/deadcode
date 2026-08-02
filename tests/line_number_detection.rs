//! Port of `tests/test_line_number_detection.py`.

mod common;
use common::Project;

#[test]
fn tuple_unpack_assignment_counts_every_name() {
    let p = Project::new();
    p.write("foo.py", "foo = None\nbar, spam, eggs = 1, 2, 3\n");
    let result = p.run(&["foo.py", "--count"]).unwrap();
    assert_eq!(result, "4");
}

#[test]
fn unused_function_with_used_argument_counts_only_the_function() {
    let p = Project::new();
    p.write(
        "foo.py",
        "def unused_function(an_arg):\n    return an_arg\n",
    );
    let result = p.run(&["foo.py", "--count"]).unwrap();
    assert_eq!(result, "1");
}

#[test]
fn unused_class_members_and_subclass_counted() {
    let p = Project::new();
    p.write(
        "foo.py",
        "class Foo:\n    bar = None\n    spam = None\n\n    def eggs(self):\n        return None\n\n\nclass Bar(Foo):\n    pass\n",
    );
    let result = p.run(&["foo.py", "--count"]).unwrap();
    assert_eq!(result, "4");
}

#[test]
fn lambda_assigned_to_a_name_counts_as_one() {
    let p = Project::new();
    p.write("foo.py", "my_func = lambda x: x\n");
    let result = p.run(&["foo.py", "--count"]).unwrap();
    assert_eq!(result, "1");
}
