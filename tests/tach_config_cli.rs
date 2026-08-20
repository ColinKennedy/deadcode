//! Port of `tests/cli_args/test_tach_config.py` — CLI-level `--tach-config`
//! regression tests against a real filesystem.

mod common;
use common::Project;

#[test]
fn name_exposed_via_tach_interface_is_not_reported() {
    let p = Project::new();
    p.write(
        "tach.toml",
        "source_roots = [\".\"]\n\n[[interfaces]]\nexpose = [\"get_data\"]\nfrom = [\"core\"]\n",
    );
    p.write("core.py", "def get_data():\n    return 1\n");
    let tach = p.path_str("tach.toml");
    let core = p.path_str("core.py");
    let result = p.run(&[&core, "--tach-config", &tach, "--no-color"]);
    assert_eq!(result, None);
}

#[test]
fn name_not_covered_by_any_interface_is_still_reported() {
    let p = Project::new();
    p.write(
        "tach.toml",
        "source_roots = [\".\"]\n\n[[interfaces]]\nexpose = [\"get_data\"]\nfrom = [\"core\"]\n",
    );
    p.write("core.py", "def get_data():\n    return 1\n");
    p.write("domain.py", "def helper():\n    return 1\n");
    let tach = p.path_str("tach.toml");
    let core = p.path_str("core.py");
    let domain = p.path_str("domain.py");
    let result = p
        .run(&[&core, &domain, "--tach-config", &tach, "--no-color"])
        .unwrap();
    assert!(result.contains("DC02"));
    assert!(result.contains("helper"));
    assert!(!result.contains("get_data"));
}

#[test]
fn file_under_unchecked_module_is_skipped_entirely() {
    let p = Project::new();
    p.write(
        "tach.toml",
        "source_roots = [\".\"]\n\n[[modules]]\npath = \"legacy\"\nunchecked = true\n",
    );
    p.write("legacy.py", "def unused_in_legacy():\n    pass\n");
    p.write("core.py", "def unused_in_core():\n    pass\n");
    let tach = p.path_str("tach.toml");
    let legacy = p.path_str("legacy.py");
    let core = p.path_str("core.py");
    let result = p
        .run(&[&legacy, &core, "--tach-config", &tach, "--no-color"])
        .unwrap();
    assert!(!result.contains("unused_in_legacy"));
    assert!(result.contains("unused_in_core"));
}

#[test]
fn multiple_source_roots_with_literal_dotted_from_pattern() {
    let p = Project::new();
    p.write(
        "tach.toml",
        "source_roots = [\"backend\", \"frontend\"]\n\n[[interfaces]]\nexpose = [\"some_function\"]\nfrom = [\"some.subpackage.module\"]\n",
    );
    p.write(
        "backend/some/subpackage/module.py",
        "def some_function():\n    return 1\n",
    );
    p.write("frontend/util.py", "def helper():\n    return 1\n");
    let tach = p.path_str("tach.toml");
    let backend = p.path_str("backend/some/subpackage/module.py");
    let frontend = p.path_str("frontend/util.py");
    let result = p
        .run(&[&backend, &frontend, "--tach-config", &tach, "--no-color"])
        .unwrap();
    assert!(!result.contains("some_function"));
    assert!(result.contains("helper"));
    assert!(result.contains("DC02"));
}

#[test]
fn double_star_from_pattern_across_source_roots() {
    let p = Project::new();
    p.write(
        "tach.toml",
        "source_roots = [\"backend\", \"frontend\"]\n\n[[interfaces]]\nexpose = [\"get_data\"]\nfrom = [\"pkg1.**\"]\n",
    );
    p.write("backend/pkg1/sub/mod.py", "def get_data():\n    return 1\n");
    p.write("frontend/util.py", "def helper():\n    return 1\n");
    let tach = p.path_str("tach.toml");
    let backend = p.path_str("backend/pkg1/sub/mod.py");
    let frontend = p.path_str("frontend/util.py");
    let result = p
        .run(&[&backend, &frontend, "--tach-config", &tach, "--no-color"])
        .unwrap();
    assert!(!result.contains("get_data"));
    assert!(result.contains("helper"));
}

#[test]
fn file_next_to_source_root_is_not_scanned() {
    let p = Project::new();
    p.write("tach.toml", "source_roots = [\"src\"]\n");
    p.write("src/core.py", "def unused_in_core():\n    pass\n");
    p.write(
        "package.py",
        "def unused_outside_source_roots():\n    pass\n",
    );
    let tach = p.path_str("tach.toml");
    let root = p.dir.path().to_string_lossy().into_owned();
    let result = p
        .run(&[&root, "--tach-config", &tach, "--no-color"])
        .unwrap();
    assert!(result.contains("unused_in_core"));
    assert!(!result.contains("unused_outside_source_roots"));
}

#[test]
fn explicit_directory_outside_tach_project_is_still_scanned() {
    let p = Project::new();
    p.write("project/tach.toml", "source_roots = [\"src\"]\n");
    p.write("project/src/core.py", "def unused_in_core():\n    pass\n");
    p.write("elsewhere/other.py", "def unused_elsewhere():\n    pass\n");
    let tach = p.path_str("project/tach.toml");
    let project_src = p.path_str("project/src");
    let elsewhere = p.path_str("elsewhere");
    let result = p
        .run(&[
            &project_src,
            &elsewhere,
            "--tach-config",
            &tach,
            "--no-color",
        ])
        .unwrap();
    assert!(result.contains("unused_in_core"));
    assert!(result.contains("unused_elsewhere"));
}

#[test]
fn multiple_tach_config_values_are_merged() {
    let p = Project::new();
    p.write("a.toml", "source_roots = [\".\"]\n\n[[interfaces]]\nexpose = [\"a_public\"]\nfrom = [\"a_module\"]\n");
    p.write("b.toml", "source_roots = [\".\"]\n\n[[interfaces]]\nexpose = [\"b_public\"]\nfrom = [\"b_module\"]\n");
    p.write("a_module.py", "def a_public():\n    return 1\n");
    p.write("b_module.py", "def b_public():\n    return 1\n");
    let a_toml = p.path_str("a.toml");
    let b_toml = p.path_str("b.toml");
    let a_mod = p.path_str("a_module.py");
    let b_mod = p.path_str("b_module.py");
    let result = p.run(&[
        &a_mod,
        &b_mod,
        "--tach-config",
        &a_toml,
        "--tach-config",
        &b_toml,
        "--no-color",
    ]);
    assert_eq!(result, None);
}
