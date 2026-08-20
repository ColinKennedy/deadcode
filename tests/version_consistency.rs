//! Guards against the version drifting between the two manifests.
//!
//! Since the Rust port the version lives in `Cargo.toml` (source of truth,
//! compiled in as `CARGO_PKG_VERSION` and reported by `--version`) *and* in
//! `pyproject.toml` (what maturin stamps on the published wheel). Nothing
//! links them, and a build succeeds happily when they disagree — so a release
//! could ship a wheel labelled one version whose binary reports another.
//!
//! Every version bump before the port touched only `pyproject.toml`, which is
//! exactly the habit that would now go wrong silently.

use std::path::Path;

fn manifest_dir() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn read_toml(relative_path: &str) -> toml::Value {
    let path = manifest_dir().join(relative_path);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("could not read {}: {e}", path.display()));
    text.parse::<toml::Value>()
        .unwrap_or_else(|e| panic!("could not parse {}: {e}", path.display()))
}

fn string_at(value: &toml::Value, table: &str, key: &str, source: &str) -> String {
    value
        .get(table)
        .and_then(|t| t.get(key))
        .and_then(toml::Value::as_str)
        .unwrap_or_else(|| panic!("{source} has no [{table}] {key}"))
        .to_string()
}

#[test]
fn cargo_and_pyproject_versions_match() {
    let cargo_version = string_at(&read_toml("Cargo.toml"), "package", "version", "Cargo.toml");
    let pyproject_version = string_at(
        &read_toml("pyproject.toml"),
        "project",
        "version",
        "pyproject.toml",
    );

    assert_eq!(
        cargo_version, pyproject_version,
        "version mismatch: Cargo.toml says {cargo_version}, pyproject.toml says \
         {pyproject_version}. Both must be bumped together — the binary reports \
         Cargo.toml's value while the published wheel is labelled with \
         pyproject.toml's."
    );
}

/// The compiled-in version must also agree, which catches a stale build or a
/// hand-edited constant.
#[test]
fn compiled_version_matches_the_manifests() {
    let cargo_version = string_at(&read_toml("Cargo.toml"), "package", "version", "Cargo.toml");
    assert_eq!(
        deadcode::cli::VERSION,
        cargo_version,
        "`--version` would report {} but Cargo.toml declares {cargo_version}",
        deadcode::cli::VERSION
    );
}

/// The PyPI distribution is `lapsed`; the command it installs is `deadcode`.
/// Both matter and they are set in different files, so pin them here — a
/// well-meaning "make the names consistent" edit to `[[bin]]` would silently
/// rename the CLI users invoke.
#[test]
fn distribution_is_lapsed_but_the_command_stays_deadcode() {
    let cargo = read_toml("Cargo.toml");
    let pyproject = read_toml("pyproject.toml");

    assert_eq!(
        string_at(&pyproject, "project", "name", "pyproject.toml"),
        "lapsed",
        "PyPI distribution name changed"
    );

    let bin_name = cargo
        .get("bin")
        .and_then(toml::Value::as_array)
        .and_then(|bins| bins.first())
        .and_then(|b| b.get("name"))
        .and_then(toml::Value::as_str)
        .expect("Cargo.toml has no [[bin]] name");
    assert_eq!(
        bin_name, "deadcode",
        "[[bin]] name is the executable filename and the command users type; \
         it must stay `deadcode`"
    );
}
