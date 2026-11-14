//! Port of the still-relevant part of `deadcode/visitor/utils.py`.
//!
//! Condition analysis lives in `DeadCodeVisitor`, alongside the statement
//! ranges needed to report and safely remove DC09 unreachable bodies.

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
