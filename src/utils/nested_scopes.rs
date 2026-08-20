//! Port of `deadcode/utils/nested_scopes.py`, deliberately narrowed in scope
//! (confirmed safe by reading the Python source, not just observed via
//! profiling): the only consumer of `NestedScope` is `get_inherits_from`'s
//! base-class chain walk. `mark_as_used`'s `number_of_uses` increment is
//! write-only in the Python code — `_get_unused_items` decides "is this used"
//! purely from the flat `used_names` set, never from `number_of_uses` — so
//! it has zero effect on any observable output. And since only `ast.ClassDef`
//! nodes ever have a non-`None` `inherits_from`, storing non-class
//! definitions in the scope tree changes nothing either (a lookup landing on
//! a non-class item behaves identically to landing on nothing, because
//! neither has an inherits-from chain to extend). So this port stores only
//! classes, keyed by their scope path, each holding its own already-flattened
//! inherits-from chain — no `number_of_uses`, no shared mutable `CodeItem`,
//! no `Rc<RefCell<_>>`.
//!
//! Also fixes the join-then-immediately-split round trip this session's own
//! profiling work flagged in the Python version (`self.scope` joins
//! `scope_parts` with '.', `NestedScope.get()`/`add()` immediately split it
//! back apart) by taking `&[String]` scope parts directly everywhere.

use std::collections::HashMap;

#[derive(Default)]
struct ScopeNode {
    children: HashMap<String, ScopeNode>,
    classes: HashMap<String, Vec<String>>,
}

#[derive(Default)]
pub struct NestedScope {
    root: ScopeNode,
}

impl NestedScope {
    pub fn new() -> Self {
        NestedScope::default()
    }

    /// Registers a class's (already-flattened) inherits-from chain at the
    /// given scope path.
    pub fn add_class(&mut self, scope_parts: &[String], name: &str, inherits_from: Vec<String>) {
        let mut node = &mut self.root;
        for part in scope_parts {
            node = node.children.entry(part.clone()).or_default();
        }
        node.classes.insert(name.to_string(), inherits_from);
    }

    /// Looks up `name` starting from `scope_parts` and walking up through
    /// enclosing scopes (nearest first), returning the first match's
    /// inherits-from chain.
    ///
    /// Faithfully preserves a real quirk confirmed in the Python original:
    /// the full `scope_parts` path must already exist as scope nodes (each
    /// created as a side effect of some *other* class's `add_class` call
    /// reaching that path) before a lookup from it can walk back up to an
    /// ancestor scope — a lookup does NOT gracefully fall back to the
    /// longest existing prefix. In practice this rarely matters: the
    /// `--ignore-*-if-inherits-from` flags mostly work anyway because
    /// `should_ignore_new_definitions` latches true and cascades to every
    /// nested definition once an outer class matches, independent of
    /// whether a deeper chain lookup would have succeeded.
    pub fn get_inherits_from(&self, scope_parts: &[String], name: &str) -> Option<&Vec<String>> {
        // Build the chain of scope nodes from root down to `scope_parts`,
        // then search nearest-to-farthest (i.e. in reverse).
        let mut chain: Vec<&ScopeNode> = Vec::with_capacity(scope_parts.len() + 1);
        let mut node = &self.root;
        chain.push(node);
        for part in scope_parts {
            node = node.children.get(part)?;
            chain.push(node);
        }
        for scope in chain.iter().rev() {
            if let Some(inherits_from) = scope.classes.get(name) {
                return Some(inherits_from);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parts(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn direct_lookup_in_same_scope() {
        let mut scope = NestedScope::new();
        scope.add_class(&parts(&["mod"]), "Base", vec![]);
        assert_eq!(
            scope.get_inherits_from(&parts(&["mod"]), "Base"),
            Some(&vec![])
        );
    }

    #[test]
    fn lookup_walks_up_to_parent_scope_once_the_path_exists() {
        let mut scope = NestedScope::new();
        scope.add_class(&parts(&["mod"]), "Base", vec![]);
        // Establishes the "mod.Sub" scope path first (as a real add_class call
        // targeting that path would) — matching the confirmed Python-original
        // constraint that the full path must already exist before a lookup FROM
        // it can walk back up to an ancestor scope.
        scope.add_class(&parts(&["mod", "Sub"]), "Other", vec![]);
        assert_eq!(
            scope.get_inherits_from(&parts(&["mod", "Sub"]), "Base"),
            Some(&vec![])
        );
    }

    #[test]
    fn lookup_fails_if_intermediate_scope_path_was_never_established() {
        let mut scope = NestedScope::new();
        scope.add_class(&parts(&["mod"]), "Base", vec![]);
        // Nothing has ever been added AT scope ["mod", "Sub"], so that scope
        // node doesn't exist yet — the lookup must not silently fall back to
        // the "mod" prefix, matching Python's exact (if surprising) behavior.
        assert_eq!(
            scope.get_inherits_from(&parts(&["mod", "Sub"]), "Base"),
            None
        );
    }

    #[test]
    fn missing_name_returns_none() {
        let scope = NestedScope::new();
        assert_eq!(scope.get_inherits_from(&parts(&["mod"]), "Nope"), None);
    }

    #[test]
    fn flattened_multi_level_chain() {
        let mut scope = NestedScope::new();
        scope.add_class(&parts(&["mod"]), "Foo", vec![]);
        scope.add_class(&parts(&["mod"]), "Bar", vec!["Foo".to_string()]);
        scope.add_class(
            &parts(&["mod"]),
            "Spam",
            vec!["Bar".to_string(), "Foo".to_string()],
        );
        assert_eq!(
            scope.get_inherits_from(&parts(&["mod"]), "Spam"),
            Some(&vec!["Bar".to_string(), "Foo".to_string()])
        );
    }
}
