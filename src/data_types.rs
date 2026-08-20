//! Port of `deadcode/data_types.py`.

use clap::Parser;

/// Mirrors the Python `Args` dataclass, with `clap` derive attributes layered
/// directly on top rather than a separate CLI-specific struct. List options
/// accept repeated `--flag a b --flag c` occurrences (`ArgAction::Append`,
/// `num_args(0..)`, mirroring Python's `nargs='*', action='append'`); comma
/// splitting within each value and the `pyproject.toml` config merge are a
/// separate post-processing step (`args.rs`), same as Python's
/// `flatten_lists_of_comma_separated_values` + `parse_pyproject_toml`.
///
/// `--version` is intentionally NOT a field clap populates meaningfully:
/// Python's `main()` intercepts `--version` and returns before
/// `parse_arguments()` is ever called, so `Args.version` is dead in the
/// current implementation too — kept only for structural fidelity.
#[derive(Parser, Debug, Clone, Default)]
#[command(name = "deadcode", disable_version_flag = true)]
pub struct Args {
    /// Paths where to search for python files
    #[arg(required = true, num_args = 1..)]
    pub paths: Vec<String>,

    /// Automatically remove detected unused code expressions from the code base.
    #[arg(long)]
    pub fix: bool,

    /// Show changes which would be made in files with --fix option.
    #[arg(long)]
    pub dry: bool,

    /// Filenames (or path expressions), that will be reflected in the output and modified.
    #[arg(long, num_args = 0.., action = clap::ArgAction::Append)]
    pub only: Vec<String>,

    /// Filenames (or path expressions), which will be completely skipped without being analysed.
    #[arg(long, num_args = 0.., action = clap::ArgAction::Append)]
    pub exclude: Vec<String>,

    /// Ignores definition (including name and body) if a name of an expression matches any of the provided ones.
    #[arg(long, num_args = 0.., action = clap::ArgAction::Append)]
    pub ignore_definitions: Vec<String>,

    /// (No-op today, same as upstream — see FOLLOWUP.local.md.)
    #[arg(long, num_args = 0.., action = clap::ArgAction::Append)]
    pub ignore_definitions_if_decorated_with: Vec<String>,

    /// Ignores definition (including name and body) of a class if it inherits from any of the provided class names.
    #[arg(long, num_args = 0.., action = clap::ArgAction::Append)]
    pub ignore_definitions_if_inherits_from: Vec<String>,

    /// Ignores body of an expression if its name matches any of the provided names.
    #[arg(long, num_args = 0.., action = clap::ArgAction::Append)]
    pub ignore_bodies_of: Vec<String>,

    /// (No-op today, same as upstream.)
    #[arg(long, num_args = 0.., action = clap::ArgAction::Append)]
    pub ignore_bodies_if_decorated_with: Vec<String>,

    /// Ignores body of a class if it inherits from any of the provided class names.
    #[arg(long, num_args = 0.., action = clap::ArgAction::Append)]
    pub ignore_bodies_if_inherits_from: Vec<String>,

    /// (No-op today, same as upstream.)
    #[arg(long, num_args = 0.., action = clap::ArgAction::Append)]
    pub ignore_if_decorated_with: Vec<String>,

    /// (No-op today, same as upstream.)
    #[arg(long, num_args = 0.., action = clap::ArgAction::Append)]
    pub ignore_if_inherits_from: Vec<String>,

    /// Removes provided list of names from the output. Glob patterns may be used.
    #[arg(long, num_args = 0.., action = clap::ArgAction::Append)]
    pub ignore_names: Vec<String>,

    /// (No-op today, same as upstream.)
    #[arg(long, num_args = 0.., action = clap::ArgAction::Append)]
    pub ignore_names_if_decorated_with: Vec<String>,

    /// (No-op today, same as upstream.)
    #[arg(long, num_args = 0.., action = clap::ArgAction::Append)]
    pub ignore_names_if_inherits_from: Vec<String>,

    /// Ignores unused names in files, which filenames match provided path expressions.
    #[arg(long, num_args = 0.., action = clap::ArgAction::Append)]
    pub ignore_names_in_files: Vec<String>,

    /// Does not report unused attributes assigned on objects other than `self`.
    #[arg(long)]
    pub ignore_non_self_attributes: bool,

    /// Does not report unused attributes assigned directly in a class body.
    #[arg(long)]
    pub ignore_class_attributes: bool,

    /// Paths to one or more tach.toml files whose [[interfaces]] mark names as public API.
    #[arg(long, num_args = 0.., action = clap::ArgAction::Append)]
    pub tach_config: Vec<String>,

    /// Turn off colors in the output
    #[arg(long)]
    pub no_color: bool,

    /// Does not output anything. Exit code still reflects whether unused names were found.
    #[arg(long)]
    pub quiet: bool,

    /// Provides the count of the detected unused names instead of printing them all out.
    #[arg(long)]
    pub count: bool,

    /// Shows logs useful for debugging
    #[arg(short = 'v', long)]
    pub verbose: bool,

    /// Shows deadcode version (handled upstream of clap parsing — see module doc comment).
    #[arg(long)]
    pub version: bool,
}

/// A code file region to remove: (line_start, line_end, col_start, col_end).
/// `Ord`/`PartialOrd` derive lexicographic tuple comparison, matching Python
/// tuple comparison semantics used by `sorted(overlaping_file_parts)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Part {
    pub line_start: u32,
    pub line_end: u32,
    pub col_start: u32,
    pub col_end: u32,
}

impl Part {
    pub fn new(line_start: u32, line_end: u32, col_start: u32, col_end: u32) -> Self {
        Part {
            line_start,
            line_end,
            col_start,
            col_end,
        }
    }
}
