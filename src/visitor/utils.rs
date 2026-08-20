//! Port of the still-relevant part of `deadcode/visitor/utils.py`.
//!
//! `condition_is_always_false`/`condition_is_always_true` (and the
//! `_safe_eval` double-evaluation this session's own profiling work found
//! wasteful) are NOT ported: tracing the actual consumer confirmed
//! `unreachable_code` findings are computed but explicitly excluded from
//! `get_unused_code_items()` (`# TODO: removal of unreachable_code has a lot
//! of edge cases`) and no test suppresses/asserts on them being reported —
//! the whole feature has zero effect on any observable output. Skipping it
//! removes an entire category of AST recursion (double-visiting every
//! `if`/`while`/ternary condition) for free.

use ruff_python_ast::Expr;

/// Port of `get_decorator_name`: builds `@module.attr` (or `@name` for a
/// bare decorator, or the callee's name for a `@deco(...)` call-form
/// decorator — call args never affect the name).
pub fn get_decorator_name(decorator: &Expr) -> String {
    let mut node = decorator;
    if let Expr::Call(call) = node {
        node = &call.func;
    }
    let mut parts: Vec<&str> = Vec::new();
    while let Expr::Attribute(attr) = node {
        parts.push(attr.attr.as_str());
        node = &attr.value;
    }
    if let Expr::Name(name) = node {
        parts.push(name.id.as_str());
    }
    parts.reverse();
    format!("@{}", parts.join("."))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actions::parse_abstract_syntax_tree::parse_abstract_syntax_tree;

    fn decorator_of(src: &str) -> Expr {
        let module = parse_abstract_syntax_tree(src).unwrap();
        match &module[0] {
            ruff_python_ast::Stmt::FunctionDef(f) => f.decorator_list[0].expression.clone(),
            ruff_python_ast::Stmt::ClassDef(c) => c.decorator_list[0].expression.clone(),
            _ => panic!("expected a decorated def"),
        }
    }

    #[test]
    fn bare_name_decorator() {
        let src = "@property\ndef f(): pass\n";
        assert_eq!(get_decorator_name(&decorator_of(src)), "@property");
    }

    #[test]
    fn dotted_attribute_decorator() {
        let src = "@module.my_decorator\ndef f(): pass\n";
        assert_eq!(
            get_decorator_name(&decorator_of(src)),
            "@module.my_decorator"
        );
    }

    #[test]
    fn deeply_dotted_decorator() {
        let src = "@module.my_decorator1.my_decorator2\ndef f(): pass\n";
        assert_eq!(
            get_decorator_name(&decorator_of(src)),
            "@module.my_decorator1.my_decorator2"
        );
    }

    #[test]
    fn call_form_decorator_strips_args() {
        let src = "@module.my_decorator(1, 2)\ndef f(): pass\n";
        assert_eq!(
            get_decorator_name(&decorator_of(src)),
            "@module.my_decorator"
        );
    }
}
