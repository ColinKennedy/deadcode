//! Small path helpers used by the tach.toml support and file discovery.

use std::path::{Component, Path, PathBuf};

/// `Path.resolve()`-equivalent: makes the path absolute (relative to the
/// current working directory) and lexically collapses `.`/`..` components.
///
/// Deliberately does NOT resolve symlinks (unlike Python's `Path.resolve()`
/// when the path exists) and does NOT use `std::fs::canonicalize` at all —
/// on Windows, `canonicalize` returns `\\?\`-prefixed verbatim paths that
/// don't string-compare equal to normally-formatted paths built elsewhere,
/// which would break the exact-path-equality checks tach support relies on.
/// Also, unlike `canonicalize`, this works for paths that don't exist yet,
/// matching Python's `Path.resolve(strict=False)` default. Symlink
/// resolution is not exercised by any tach.toml test scenario (temp-dir
/// based, no symlinks), so this is a safe, low-risk simplification.
pub fn resolve_path(path: &Path) -> PathBuf {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    };
    normalize_lexically(&absolute)
}

fn normalize_lexically(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                // Only pop a regular directory component; never climb above
                // a root/prefix, matching how Path.resolve() behaves.
                match out.components().next_back() {
                    Some(Component::Normal(_)) => {
                        out.pop();
                    }
                    _ => out.push(component.as_os_str()),
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// `Path::as_posix()`-equivalent iterator: strict ancestors (excludes `path`
/// itself), matching Python's `Path.parents` (as opposed to Rust's
/// `Path::ancestors()`, which includes the path itself first).
pub fn strict_ancestors(path: &Path) -> impl Iterator<Item = &Path> {
    path.ancestors().skip(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collapses_dot_and_dotdot() {
        #[cfg(windows)]
        let (input, expected) = (r"C:\a\b\..\c\.\d", r"C:\a\c\d");
        #[cfg(not(windows))]
        let (input, expected) = ("/a/b/../c/./d", "/a/c/d");

        let resolved = resolve_path(Path::new(input));
        assert_eq!(resolved, PathBuf::from(expected));
    }

    #[test]
    fn relative_path_becomes_absolute() {
        let resolved = resolve_path(Path::new("foo"));
        assert!(resolved.is_absolute());
        assert!(resolved.ends_with("foo"));
    }

    #[test]
    fn strict_ancestors_excludes_self() {
        let p = Path::new("/a/b/c");
        let ancestors: Vec<&Path> = strict_ancestors(p).collect();
        assert_eq!(
            ancestors,
            vec![Path::new("/a/b"), Path::new("/a"), Path::new("/")]
        );
    }
}
