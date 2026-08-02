//! Port of `deadcode/cli.py`.

use std::path::Path;

use crate::actions::find_python_filenames::find_python_filenames;
use crate::actions::fix_or_show_unused_code::fix_or_show_unused_code;
use crate::actions::get_unused_names_error_message::get_unused_names_error_message;
use crate::actions::parse_arguments::parse_arguments_with_config;
use crate::actions::parse_tach_config::load_tach_index;
use crate::visitor::dead_code_visitor::DeadCodeVisitor;

/// Version is resolved at compile time from `Cargo.toml`, the single source
/// of truth for this crate's version (see Phase 10: `pyproject.toml`'s
/// declared version is kept in sync with it for packaging, not the other
/// way around).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Returns `Ok(Some(output))` when there's something to print (either an
/// error/report listing or the version string), `Ok(None)` when everything
/// is clean (the "Well done!" case), or `Err` on a CLI-argument parsing
/// failure (mirrors argparse's own `sys.exit(2)`-on-bad-args behavior,
/// surfaced here as a `clap::Error` for the caller to `.exit()`).
///
/// `command_line_args = None` means "use the real process argv" (the normal
/// case); `Some(args)` lets callers — tests, primarily — provide an explicit
/// argv, matching the Python `main(command_line_args=None)` signature.
pub fn main(command_line_args: Option<&[String]>) -> Result<Option<String>, clap::Error> {
    main_with_config(command_line_args, Path::new("pyproject.toml"))
}

/// Same as [`main`], but with an explicit `pyproject.toml` path instead of
/// always resolving one relative to the process's current directory.
///
/// This exists because `cargo test` runs every integration test with CWD set
/// to the crate root — i.e. THIS repo's own `pyproject.toml`, with its own
/// `[tool.deadcode]` `exclude`/`ignore_names` — so any test calling `main()`
/// directly would silently inherit that config. Tests must pass a path that
/// doesn't exist (or a per-test fixture toml) to stay isolated; only the
/// real `deadcode` binary (via `print_main`) uses the CWD-relative default.
pub fn main_with_config(
    command_line_args: Option<&[String]>,
    pyproject_path: &Path,
) -> Result<Option<String>, clap::Error> {
    let real_argv: Vec<String> = std::env::args().skip(1).collect();

    // Matches Python's exact (slightly odd) precedence: `(command_line_args
    // and '--version' in command_line_args) or '--version' in sys.argv` —
    // the real process argv is checked regardless of what was explicitly
    // passed in.
    let requests_version = command_line_args.is_some_and(|a| a.iter().any(|s| s == "--version"))
        || real_argv.iter().any(|s| s == "--version");
    if requests_version {
        return Ok(Some(VERSION.to_string()));
    }

    let argv: Vec<String> = command_line_args
        .map(<[String]>::to_vec)
        .unwrap_or(real_argv);
    let args = parse_arguments_with_config(&argv, pyproject_path)?;

    let tach_index = load_tach_index(&args.tach_config);
    let filenames = find_python_filenames(&args, &tach_index);

    let mut visitor = DeadCodeVisitor::new(&args, &tach_index);
    visitor.visit_files(&filenames);
    let unused_names = visitor.get_unused_code_items();

    let mut file_diff = String::new();
    if (args.fix || args.dry) && !unused_names.is_empty() {
        file_diff = fix_or_show_unused_code(&unused_names, &args);
    }

    if let Some(error_message) = get_unused_names_error_message(&unused_names, &args) {
        let suffix = if file_diff.is_empty() {
            String::new()
        } else {
            format!("\n\n{file_diff}")
        };
        return Ok(Some(error_message + &suffix));
    }

    if !args.count && !args.quiet {
        // Python guards this print with a `UnicodeEncodeError` fallback for
        // consoles that can't encode the emoji (e.g. Windows cp1252). That's
        // a CPython text-I/O-encoding concern with no Rust equivalent —
        // `println!` writes UTF-8 bytes directly and can't fail this way, so
        // there's nothing to fall back from.
        println!("\x1b[1mWell done!\x1b[0m \u{2728} \u{1f680} \u{2728}");
    }

    Ok(None)
}

/// What `print_main` should do once `main()` has resolved — split out as
/// plain data (rather than calling `std::process::exit`/`clap::Error::exit`
/// directly) so tests can assert on the *decision* without actually
/// terminating the test process. Python's equivalent test uses
/// `try/except SystemExit` to catch `sys.exit()` in-process; Rust's
/// `process::exit()` has no in-process equivalent to catch, so the
/// decision has to be extracted as data instead.
#[derive(Debug)]
pub enum ExitAction {
    /// Nothing printed, process should exit 0 (or just return normally).
    Clean,
    /// `stdout` line (if non-empty) already decided, then exit with this code.
    Print { stdout: String, code: i32 },
    /// CLI-argument parsing failed; caller should hand this to
    /// `clap::Error::exit()` (prints usage/error and exits ~2).
    ClapError(clap::Error),
}

/// Testable core of `print_main`: resolves what should happen without
/// actually printing or exiting.
pub fn print_main_action(
    command_line_args: Option<&[String]>,
    pyproject_path: &Path,
) -> ExitAction {
    let real_argv: Vec<String> = std::env::args().skip(1).collect();
    let is_version_request = command_line_args
        .map(|a| a.iter().any(|s| s == "--version"))
        .unwrap_or_else(|| real_argv.iter().any(|s| s == "--version"));

    match main_with_config(command_line_args, pyproject_path) {
        Ok(Some(result)) => {
            if is_version_request {
                ExitAction::Print {
                    stdout: result,
                    code: 0,
                }
            } else {
                // Matches Python's `if result:` (truthiness, not `is not
                // None`) — an empty string (from `--quiet`) prints nothing,
                // but the exit code still fires for it.
                ExitAction::Print {
                    stdout: result,
                    code: 1,
                }
            }
        }
        Ok(None) => ExitAction::Clean,
        Err(e) => ExitAction::ClapError(e),
    }
}

/// The `deadcode` console-script entry point.
pub fn print_main() {
    match print_main_action(None, Path::new("pyproject.toml")) {
        ExitAction::Clean => {}
        ExitAction::Print { stdout, code } => {
            if !stdout.is_empty() {
                println!("{stdout}");
            }
            if code != 0 {
                std::process::exit(code);
            }
        }
        ExitAction::ClapError(e) => e.exit(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    const NO_PYPROJECT: &str = "/definitely/does/not/exist/pyproject.toml";

    fn run(argv: &[&str]) -> Result<Option<String>, clap::Error> {
        main_with_config(Some(&args(argv)), Path::new(NO_PYPROJECT))
    }

    #[test]
    fn version_flag_returns_version_string() {
        let result = run(&["--version"]).unwrap();
        assert_eq!(result, Some(VERSION.to_string()));
    }

    #[test]
    fn version_flag_takes_precedence_over_path_scanning() {
        let result = run(&[".", "--version"]).unwrap();
        assert_eq!(result, Some(VERSION.to_string()));
    }

    #[test]
    fn unused_variable_end_to_end() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("foo.py");
        std::fs::write(&file, "unused_var = 1\n").unwrap();
        let result = run(&[file.to_str().unwrap(), "--no-color"]).unwrap();
        let msg = result.unwrap();
        assert!(msg.contains("DC01"));
        assert!(msg.contains("unused_var"));
    }

    #[test]
    fn clean_file_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("foo.py");
        std::fs::write(&file, "used_var = 1\nprint(used_var)\n").unwrap();
        let result = run(&[file.to_str().unwrap(), "--no-color"]).unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn quiet_mode_returns_empty_string_not_none_when_issues_found() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("foo.py");
        std::fs::write(&file, "unused_var = 1\n").unwrap();
        let result = run(&[file.to_str().unwrap(), "--quiet"]).unwrap();
        assert_eq!(result, Some(String::new()));
    }

    #[test]
    fn count_mode_returns_count() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("foo.py");
        std::fs::write(&file, "a = 1\nb = 2\n").unwrap();
        let result = run(&[file.to_str().unwrap(), "--count"]).unwrap();
        assert_eq!(result, Some("2".to_string()));
    }
}
