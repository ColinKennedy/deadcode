//! Port of `deadcode/visitor/module_exports.py`: what `from module import *`
//! brings into scope.

use std::collections::HashSet;

use ruff_python_ast::statement_visitor::{walk_stmt, StatementVisitor};
use ruff_python_ast::{Expr, Stmt};

/// Returns the names a module statically exports, or `None` when they cannot
/// be known (`__all__` is built dynamically, or bound below module level).
///
/// A module-level `__all__ = [...]` list or tuple of string literals is the
/// export list; without any `__all__` binding, every public module-level
/// name is exported.
pub fn collect_module_exports(module: &[Stmt]) -> Option<HashSet<String>> {
    let mut bindings = AllBindings::default();
    bindings.visit_body(module);
    if bindings.found.is_empty() {
        return Some(public_module_level_names(module));
    }

    let mut static_exports = HashSet::new();
    for statement in bindings.found {
        let value = match statement {
            Stmt::Assign(assign) => Some(assign.value.as_ref()),
            Stmt::AnnAssign(assign) => assign.value.as_deref(),
            _ => None,
        };
        let is_module_level = module.iter().any(|top| std::ptr::eq(top, statement));
        let exports = value
            .filter(|_| is_module_level)
            .and_then(static_string_elements)?;
        static_exports = exports;
    }
    Some(static_exports)
}

#[derive(Default)]
struct AllBindings<'a> {
    found: Vec<&'a Stmt>,
}

impl<'a> StatementVisitor<'a> for AllBindings<'a> {
    fn visit_stmt(&mut self, statement: &'a Stmt) {
        let binds_all = match statement {
            Stmt::Assign(assign) => assign.targets.iter().any(is_dunder_all),
            Stmt::AnnAssign(assign) => is_dunder_all(&assign.target),
            Stmt::AugAssign(assign) => is_dunder_all(&assign.target),
            _ => false,
        };
        if binds_all {
            self.found.push(statement);
        }
        walk_stmt(self, statement);
    }
}

fn is_dunder_all(target: &Expr) -> bool {
    matches!(target, Expr::Name(name) if name.id.as_str() == "__all__")
}

/// The strings of a list or tuple made only of string literals.
fn static_string_elements(value: &Expr) -> Option<HashSet<String>> {
    let elements = match value {
        Expr::List(list) => &list.elts,
        Expr::Tuple(tuple) => &tuple.elts,
        _ => return None,
    };
    elements
        .iter()
        .map(|element| match element {
            Expr::StringLiteral(literal) => Some(literal.value.to_str().to_string()),
            _ => None,
        })
        .collect()
}

fn public_module_level_names(module: &[Stmt]) -> HashSet<String> {
    let target_name = |target: &Expr| match target {
        Expr::Name(name) => Some(name.id.to_string()),
        _ => None,
    };
    let mut names = HashSet::new();
    for statement in module {
        match statement {
            Stmt::Assign(assign) => names.extend(assign.targets.iter().filter_map(target_name)),
            Stmt::AnnAssign(assign) => names.extend(target_name(&assign.target)),
            Stmt::AugAssign(assign) => names.extend(target_name(&assign.target)),
            Stmt::FunctionDef(function) => {
                names.insert(function.name.to_string());
            }
            Stmt::ClassDef(class) => {
                names.insert(class.name.to_string());
            }
            Stmt::Import(import) => names.extend(import.names.iter().map(|alias| {
                alias.asname.as_ref().map_or_else(
                    || alias.name.split('.').next().unwrap_or_default().to_string(),
                    ToString::to_string,
                )
            })),
            Stmt::ImportFrom(import) => names.extend(
                import
                    .names
                    .iter()
                    .filter(|alias| alias.name.as_str() != "*")
                    .map(|alias| alias.asname.as_ref().unwrap_or(&alias.name).to_string()),
            ),
            _ => {}
        }
    }
    names.retain(|name| !name.starts_with('_'));
    names
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actions::parse_abstract_syntax_tree::parse_abstract_syntax_tree;

    fn exports(source: &str) -> Option<Vec<String>> {
        let module = parse_abstract_syntax_tree(source).unwrap();
        collect_module_exports(&module).map(|names| {
            let mut names: Vec<String> = names.into_iter().collect();
            names.sort();
            names
        })
    }

    #[test]
    fn a_static_all_is_the_export_list() {
        assert_eq!(
            exports("__all__ = ['a']\ndef a(): pass\ndef b(): pass\n"),
            Some(vec!["a".to_string()])
        );
    }

    #[test]
    fn without_all_public_module_level_names_are_exported() {
        assert_eq!(
            exports("import os.path\nfrom x import y as z\nfrom w import *\ndef f(): pass\n_p = 1\nclass C: pass\n"),
            Some(vec!["C".into(), "f".into(), "os".into(), "z".into()])
        );
    }

    #[test]
    fn a_dynamic_or_nested_all_makes_exports_unknown() {
        assert_eq!(exports("__all__ = ['a']\n__all__ += ['b']\n"), None);
        assert_eq!(exports("def f():\n    __all__ = ['a']\n"), None);
        assert_eq!(exports("__all__ = [name for name in 'ab']\n"), None);
    }
}
